//! ADR-034's ceiling, enforced (#628).
//!
//! ADR-028 requires a maintained MCP SDK for live provider sessions and lists *expanding
//! GraphHelm's hand-rolled MCP server into a client/session implementation* among its rejected
//! alternatives. The producer speaks the wire by hand, and the divergence was real (#628).
//!
//! ADR-034 resolves it by BOUNDING rather than permitting: a fixed, closed request set —
//! `initialize`, `notifications/initialized`, one `tools/call` — over stdio, with no negotiated
//! capabilities and no second tool, is not the session implementation ADR-026 rejected. It cannot
//! grow into one without this file going red.
//!
//! **The ceiling is the whole argument.** The reason one-shot framing is acceptable is precisely
//! that it CANNOT grow: a fourth message, a negotiated capability, a second tool, or a
//! non-stdio transport turns a bounded exchange into a client, and the ADR's acceptance stops
//! applying at exactly that point. So the acceptance is only honest if something breaks when the
//! ceiling is crossed — otherwise the next person adds the fourth message with everything green,
//! which is how the SDK clause survived a merge and every review in the first place.
//!
//! Subject: `ContainedIndexProvider::request_lines` in `src/provider.rs`. Read as SOURCE on
//! purpose: the property is "how many messages does this construct", which no runtime assertion
//! over one response can see.

const ADR: &str = "ADR-034 bounds this exchange to a closed request set: initialize, \
                   notifications/initialized, one tools/call, over stdio, with no negotiated \
                   capabilities and no second tool. Crossing that ceiling makes it the client \
                   session ADR-026 rejected, so the acceptance no longer covers it — REOPEN the \
                   decision (see #628) rather than widening this function.";

fn request_lines_source() -> String {
    let source = include_str!("../src/provider.rs");
    let start = source
        .find("fn request_lines(")
        .expect("the subject function exists; if it was renamed, this guard must follow it");
    let rest = &source[start..];
    // The function ends at the first line that closes it at the impl's indentation level.
    //
    // The run of spaces in this needle is the POINT, not a defect: four is that indentation level
    // and it is the value being searched for, never a message anyone reads. It is BUILT here
    // rather than written as a literal, which is the method the sweep's own exemption list
    // names. Exempting the file would also silence the sweep, and this crate has no per-crate
    // source guard, so that exemption would leave this file scanned by NOTHING; constructing the
    // run keeps every other authored string here under the sweep.
    let impl_indent = " ".repeat(4);
    let needle = format!("\n{impl_indent}}}\n");
    let end = rest.find(&needle).expect("the subject function has a body");
    // BOUNDARY CHECK, not just a match (#671). The needle matches the first close at this
    // indentation, and that can be wrong in EITHER direction: a nested block one level deeper (a
    // `for`, a `match` arm) can share the same run of spaces and stop the extraction too EARLY,
    // swallowing only that block's own open; searched at too shallow an indentation, the needle
    // can instead stop at an ENCLOSING scope's close -- the impl block's own -- too LATE,
    // swallowing the function's own real closing brace along with everything up to that later
    // one. Measured on the real subject: a sabotaged 8-space needle stops 24 bytes early; a
    // sabotaged 0-space needle stops 6 bytes late. Content assertions cannot see either, because
    // the truncated OR over-extended body still carries all three messages.
    //
    // Two earlier versions of this check each caught one direction and missed the other. Column
    // zero (Codex P2's finding) is brittle: nothing requires `request_lines` to be the LAST
    // method in its impl, so a sibling method added after it, sitting at the impl's own
    // indentation rather than column zero, would report HARNESS-BROKE on an otherwise-valid
    // refactor. Indentation-LEVEL comparison (this file's own second version) fixed that but is
    // blind to the late-close direction: past a too-late close, what follows (`impl
    // StructuralCodeIndex for ...`) is ALSO at or below the declared indentation, so the check
    // is satisfied by an extraction that ran past the real end (H, PR #689 review).
    //
    // BALANCE is the property of the EXTRACTION itself, not of what surrounds it, and it
    // separates all three shapes at once: the correct body carries exactly the function's own
    // opening `{`, matched inside by everything that opens also closing, with the needle finding
    // the one brace that would close it -- balance 1. Stopped early, an inner block's `{` has no
    // matching `}` inside the extract -- balance 2. Stopped late, the function's own real `}` is
    // now INSIDE the extract, cancelling its own opener -- balance 0. A sibling method, an
    // attribute, or the function moving in the file cannot move this number, because none of
    // them are part of the extracted text.
    let extracted = &rest[..end];
    let balance = brace_balance(extracted);
    assert_eq!(
        balance, 1,
        "HARNESS-BROKE: the extracted body's braces balance to {balance}, not 1, so the needle \
         did not stop at request_lines's real close (extracted through byte {end} of the \
         subject). Balance > 1 means it stopped INSIDE a nested block and swallowed only that \
         block's own opening brace; balance <= 0 means it ran PAST the real close and swallowed \
         the function's own closing brace too, stopping at some later, shallower one instead \
         (the enclosing impl's own close, say). (Skips `//` line comments and double-quoted \
         strings; a raw string or a block comment in this subject would fool it -- neither \
         appears here today.)"
    );
    extracted.to_owned()
}

/// `{`/`}` in `text`, skipping `//` line comments, double-quoted string literals, and
/// char/byte literals (`'x'`, `'\n'`, `b'\n'` -- Codex P2, PR #689 review: the subject already
/// contains `b'\n'`, so this is not a hypothetical, and an added `b'}'` would otherwise be
/// counted as a real brace) -- close enough for this one Rust source file, not a real tokenizer.
/// A raw string (`r#"..."#`) or a block comment (`/* */`) would still fool it; neither appears in
/// `request_lines` today. Deliberately not hardened further: the cost of a false pass from a
/// future raw string is a debate this file's own doc comment already names, and the cheap
/// version is what was measured (H, PR #689 review).
///
/// A char literal is matched by a bounded lookahead (`'x'` or `'\x'`) rather than scanning to the
/// next `'` unconditionally, so a lifetime (`'static`, `'a`) -- which has no closing quote in
/// range -- falls through untouched instead of swallowing everything up to some LATER quote.
///
/// A double-quoted string that never closes before the end of `text` is a HARNESS-BROKE panic
/// here, not a silently-skipped run to EOF (Codex P2, PR #689 review): a needle that stops
/// mid-string -- a multiline string literal in the subject happening to contain a content line
/// that reads exactly like the needle -- truncates the extraction inside that string, and
/// whatever braces existed AFTER the truncation point (including the function's own real close)
/// are simply absent from the count rather than miscounted. If everything before the cut already
/// balanced back to the function's own opening brace, the result reads as a clean `1` -- a false
/// pass. An open string at the scan's own end is exactly the signature of that truncation, and it
/// is checked unconditionally, not only when it would otherwise happen to produce `1`: the point
/// is to name the failure mode, not to catch it only when it happens to also fool the balance.
fn brace_balance(text: &str) -> i64 {
    let chars: Vec<char> = text.chars().collect();
    let mut balance = 0i64;
    let mut index = 0;
    while index < chars.len() {
        match chars[index] {
            '/' if chars.get(index + 1) == Some(&'/') => {
                while index < chars.len() && chars[index] != '\n' {
                    index += 1;
                }
            }
            '"' => {
                index += 1;
                let mut closed = false;
                while index < chars.len() {
                    if chars[index] == '\\' {
                        index += 2;
                        continue;
                    }
                    closed = chars[index] == '"';
                    index += 1;
                    if closed {
                        break;
                    }
                }
                assert!(
                    closed,
                    "HARNESS-BROKE: a double-quoted string literal never closed before the end \
                     of the extracted text -- the boundary needle most likely stopped INSIDE a \
                     string (a content line that happens to read like the function's own close), \
                     not at request_lines's real end. The extraction is truncated regardless of \
                     what the brace balance would otherwise read."
                );
            }
            '\'' => {
                let escaped = chars.get(index + 1) == Some(&'\\');
                let close_at = if escaped { index + 3 } else { index + 2 };
                if chars.get(close_at) == Some(&'\'') {
                    index = close_at + 1;
                } else {
                    // Not a char/byte literal in range -- a lifetime, most likely. The quote
                    // itself carries no brace meaning, so just step past it.
                    index += 1;
                }
            }
            '{' => {
                balance += 1;
                index += 1;
            }
            '}' => {
                balance -= 1;
                index += 1;
            }
            _ => index += 1,
        }
    }
    balance
}

/// THE CEILING: exactly three messages leave this producer.
#[test]
fn the_producer_sends_exactly_three_messages() {
    let body = request_lines_source();
    let messages = body.matches("\"jsonrpc\": \"2.0\"").count();

    assert_eq!(
        messages, 3,
        "the producer constructs {messages} JSON-RPC messages, not three.\n\n{ADR}"
    );
}

/// The closed method set. A method outside it is a different exchange, whatever its count.
#[test]
fn the_producer_speaks_only_the_three_named_methods() {
    let body = request_lines_source();
    for method in ["initialize", "notifications/initialized", "tools/call"] {
        assert!(
            body.contains(&format!("\"{method}\"")),
            "the producer no longer sends {method}.\n\n{ADR}"
        );
    }
    for forbidden in [
        "tools/list",
        "resources/",
        "prompts/",
        "completion/",
        "logging/",
        "notifications/cancelled",
        "ping",
    ] {
        assert!(
            !body.contains(&format!("\"{forbidden}")),
            "the producer sends {forbidden}, which is outside the closed set.\n\n{ADR}"
        );
    }
}

/// ONE tool. A second tool is a client choosing between capabilities, which is the thing the
/// rejected alternative names.
#[test]
fn the_producer_calls_exactly_one_tool() {
    let body = request_lines_source();
    let named = body.matches("\"name\": \"search_graph\"").count();
    let any_name = body.matches("\"name\":").count();

    assert_eq!(
        named, 1,
        "search_graph is not called exactly once.\n\n{ADR}"
    );
    // `clientInfo` carries the other `name`, so two is the floor and three is a second tool.
    assert!(
        any_name <= 2,
        "the producer names {any_name} things; a second tool call is a client.\n\n{ADR}"
    );
}

/// NO negotiated capabilities. An empty object is a declaration that nothing is negotiated; a
/// populated one is negotiation, which is what an SDK exists to do properly.
#[test]
fn the_producer_negotiates_no_capabilities() {
    let body = request_lines_source();
    assert!(
        body.contains("\"capabilities\": {}"),
        "the producer's capabilities object is no longer empty, so it negotiates.\n\n{ADR}"
    );
}

/// STDIO only. The transport is the session's other half: a socket or an HTTP endpoint is a
/// connection with a lifetime, which one-shot framing by definition is not.
#[test]
fn the_producer_uses_no_transport_but_stdio() {
    let source = include_str!("../src/provider.rs");
    for transport in [
        "TcpStream",
        "UnixStream",
        "reqwest",
        "hyper",
        "http://",
        "https://",
        "ws://",
        "connect(",
        "bind(",
    ] {
        assert!(
            !source.contains(transport),
            "the provider mentions {transport}: the exchange is stdio-only.\n\n{ADR}"
        );
    }
}
