//! Minimal newline-delimited JSON-RPC 2.0 over stdio — the MCP stdio transport's framing (one
//! UTF-8 message per line, no Content-Length headers). Hand-rolled per ADR-026: the surface
//! 05e needs is five methods and a closed tool list; the SDK's value begins with resources,
//! streaming transports and sampling, none of which ship here.
//!
//! The contract, pinned by the conformance tests below: a request with an `id` always gets
//! exactly one reply (the `id` echoed verbatim, string or number); a notification never gets
//! any; a line larger than [`MAX_LINE_BYTES`] is refused without buffering it whole.

use std::io::{BufRead, BufReader, Read, Write};

/// The framing bound: one message per line, and a line larger than this is refused without
/// ever being buffered whole (the reader caps via bounded reads).
pub(crate) const MAX_LINE_BYTES: usize = 1024 * 1024;

pub(crate) const PARSE_ERROR: i64 = -32700;
pub(crate) const INVALID_REQUEST: i64 = -32600;
pub(crate) const METHOD_NOT_FOUND: i64 = -32601;
pub(crate) const INVALID_PARAMS: i64 = -32602;

/// What a session handler decides for one request. The rpc layer owns framing and the
/// JSON-RPC envelope; the handler owns the method table — including `-32601` for a method it
/// does not serve and `-32602` for params it refuses.
pub(crate) enum HandlerOutcome {
    Result(serde_json::Value),
    Error { code: i64, message: String },
}

fn reply_result(id: &serde_json::Value, result: serde_json::Value) -> serde_json::Value {
    serde_json::json!({"jsonrpc": "2.0", "id": id, "result": result})
}

fn reply_error(id: &serde_json::Value, code: i64, message: &str) -> serde_json::Value {
    serde_json::json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}})
}

fn write_line(writer: &mut impl Write, value: &serde_json::Value) -> std::io::Result<()> {
    let mut line = serde_json::to_vec(value).expect("a reply serializes");
    line.push(b'\n');
    writer.write_all(&line)?;
    writer.flush()
}

/// One bounded line: reads up to `MAX_LINE_BYTES` looking for `\n`. `Ok(Some((bytes,
/// oversized)))` — when `oversized`, the rest of the line was consumed and discarded so the
/// loop resyncs on the next line; `Ok(None)` is EOF.
fn read_bounded_line(reader: &mut impl BufRead) -> std::io::Result<Option<(Vec<u8>, bool)>> {
    let mut buffer = Vec::new();
    let read = reader
        .by_ref()
        .take((MAX_LINE_BYTES + 1) as u64)
        .read_until(b'\n', &mut buffer)?;
    if read == 0 {
        return Ok(None);
    }
    if buffer.last() == Some(&b'\n') {
        buffer.pop();
        if buffer.last() == Some(&b'\r') {
            buffer.pop();
        }
        return Ok(Some((buffer, false)));
    }
    if buffer.len() > MAX_LINE_BYTES {
        // Discard the remainder of the oversized line in bounded chunks: read_until under a
        // take() consumes exactly through the newline, never past it, so the next line stays
        // intact for the resync.
        let mut scratch = Vec::new();
        loop {
            scratch.clear();
            let taken = reader.by_ref().take(8192).read_until(b'\n', &mut scratch)?;
            if taken == 0 || scratch.last() == Some(&b'\n') {
                break;
            }
        }
        return Ok(Some((Vec::new(), true)));
    }
    Ok(Some((buffer, false)))
}

/// The dispatch loop: reads newline-delimited messages until EOF, calls `handler` for every
/// well-formed request or notification, and writes exactly one reply per `id`-carrying
/// request — never one for a notification.
pub(crate) fn run<R: Read, W: Write, S>(
    reader: R,
    writer: &mut W,
    handler: impl Fn(&str, &serde_json::Value, &serde_json::Value, &mut S) -> HandlerOutcome,
    state: &mut S,
) -> std::io::Result<()> {
    let mut reader = BufReader::new(reader);
    loop {
        let Some((line, oversized)) = read_bounded_line(&mut reader)? else {
            return Ok(());
        };
        if oversized {
            write_line(
                writer,
                &reply_error(
                    &serde_json::Value::Null,
                    PARSE_ERROR,
                    &format!("a message line may not exceed {MAX_LINE_BYTES} bytes"),
                ),
            )?;
            continue;
        }
        if line.iter().all(u8::is_ascii_whitespace) {
            continue;
        }
        let Ok(message) = serde_json::from_slice::<serde_json::Value>(&line) else {
            write_line(
                writer,
                &reply_error(
                    &serde_json::Value::Null,
                    PARSE_ERROR,
                    "the line is not valid JSON",
                ),
            )?;
            continue;
        };
        let id = message.get("id").cloned();
        let marker_ok = message.get("jsonrpc").and_then(|v| v.as_str()) == Some("2.0");
        let method = message.get("method").and_then(|v| v.as_str());
        let (Some(method), true) = (method, marker_ok) else {
            // An invalid request still gets its one reply when it carried an id; a
            // malformed notification gets nothing (there is nothing to correlate).
            if let Some(id) = &id {
                write_line(
                    writer,
                    &reply_error(id, INVALID_REQUEST, "not a JSON-RPC 2.0 request"),
                )?;
            }
            continue;
        };
        let params = message
            .get("params")
            .cloned()
            .unwrap_or(serde_json::Value::Null);
        let outcome = handler(
            method,
            &params,
            id.as_ref().unwrap_or(&serde_json::Value::Null),
            state,
        );
        let Some(id) = &id else {
            continue; // a notification never gets a reply
        };
        match outcome {
            HandlerOutcome::Result(result) => write_line(writer, &reply_result(id, result))?,
            HandlerOutcome::Error { code, message } => {
                write_line(writer, &reply_error(id, code, &message))?;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    /// A test handler knowing exactly `ping` (empty result) and `tools/call` (requires a
    /// string `name` param) — the shapes the conformance tests exercise.
    fn handler(
        method: &str,
        params: &serde_json::Value,
        _id: &serde_json::Value,
        _state: &mut (),
    ) -> HandlerOutcome {
        match method {
            "ping" => HandlerOutcome::Result(serde_json::json!({})),
            "notifications/initialized" => HandlerOutcome::Result(serde_json::json!({})),
            "tools/call" => {
                if params.get("name").and_then(|name| name.as_str()).is_none() {
                    return HandlerOutcome::Error {
                        code: INVALID_PARAMS,
                        message: "tools/call requires a string \"name\"".to_owned(),
                    };
                }
                HandlerOutcome::Result(serde_json::json!({}))
            }
            _ => HandlerOutcome::Error {
                code: METHOD_NOT_FOUND,
                message: format!("method {method:?} is not part of this server"),
            },
        }
    }

    fn drive(input: &str) -> Vec<serde_json::Value> {
        let mut output = Vec::new();
        run(
            Cursor::new(input.as_bytes().to_vec()),
            &mut output,
            handler,
            &mut (),
        )
        .expect("the loop runs to EOF");
        String::from_utf8(output)
            .expect("replies are UTF-8")
            .lines()
            .map(|line| serde_json::from_str(line).expect("each reply line is one JSON value"))
            .collect()
    }

    #[test]
    fn a_request_round_trips_and_the_id_echoes_including_string_ids() {
        let replies = drive(
            "{\"jsonrpc\":\"2.0\",\"id\":\"abc\",\"method\":\"ping\"}\n{\"jsonrpc\":\"2.0\",\"id\":7,\"method\":\"ping\"}\n",
        );
        assert_eq!(replies.len(), 2, "one reply per request, exactly");
        assert_eq!(replies[0]["id"], serde_json::json!("abc"));
        assert_eq!(replies[0]["jsonrpc"], "2.0");
        assert!(replies[0].get("result").is_some());
        assert_eq!(replies[1]["id"], serde_json::json!(7));
    }

    #[test]
    fn malformed_json_is_32700_and_unknown_method_is_32601_and_bad_params_is_32602() {
        let replies = drive("{not json\n");
        assert_eq!(replies[0]["error"]["code"], -32700);
        assert_eq!(replies[0]["id"], serde_json::Value::Null);

        let replies = drive("{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"no/such\"}\n");
        assert_eq!(replies[0]["error"]["code"], -32601);
        assert_eq!(replies[0]["id"], serde_json::json!(1));

        let replies = drive(
            "{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"tools/call\",\"params\":{\"arguments\":{}}}\n",
        );
        assert_eq!(replies[0]["error"]["code"], -32602);
        assert_eq!(replies[0]["id"], serde_json::json!(2));
    }

    #[test]
    fn a_notification_never_gets_a_reply() {
        let replies = drive("{\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\"}\n");
        assert!(replies.is_empty(), "a notification never gets a reply");
    }

    #[test]
    fn a_missing_jsonrpc_marker_is_32600() {
        let replies = drive("{\"id\":3,\"method\":\"ping\"}\n");
        assert_eq!(replies[0]["error"]["code"], -32600);
        assert_eq!(replies[0]["id"], serde_json::json!(3));
    }

    #[test]
    fn an_oversized_line_is_refused_without_reading_it_whole() {
        // A line larger than the bound: the reader must cap via bounded reads, never buffer
        // the whole line. The error names the bound; the loop resyncs and still serves the
        // next request.
        let mut input = String::with_capacity(MAX_LINE_BYTES + 64);
        input.push_str("{\"jsonrpc\":\"2.0\",\"id\":9,\"method\":\"ping\",\"params\":\"");
        input.push_str(&"x".repeat(MAX_LINE_BYTES + 8));
        input.push_str("\"}\n{\"jsonrpc\":\"2.0\",\"id\":10,\"method\":\"ping\"}\n");
        let replies = drive(&input);
        assert_eq!(
            replies.len(),
            2,
            "the oversized error plus the next request's reply"
        );
        assert_eq!(replies[0]["error"]["code"], -32700);
        assert_eq!(replies[0]["id"], serde_json::Value::Null);
        assert!(
            replies[0]["error"]["message"]
                .as_str()
                .unwrap()
                .contains(&MAX_LINE_BYTES.to_string()),
            "the refusal names the bound"
        );
        assert_eq!(replies[1]["id"], serde_json::json!(10), "the loop resyncs");
    }
}
