//! A provider stub for the generator's ARMING-SITE cells: newline-delimited JSON-RPC on stdio,
//! answering `initialize`, `index_status` with a FIXED head_sha, and `search_graph` with an
//! empty page.
//!
//! It lives in this crate rather than reusing the adapter's `fake_mcp_server` because
//! `CARGO_BIN_EXE_*` is only defined for binaries the SAME crate declares — a dev-dependency does
//! not carry binaries. The alternative was deriving a sibling crate's target path by hand, which
//! is exactly the kind of address that works until someone runs `-p` on one crate.
//!
//! The sha is obviously synthetic on purpose: a cell that declares any other value must see a
//! refusal naming THIS one, so the guard is about the comparison and not about a plausible value.

use std::io::{BufRead, Write};

pub const HEAD_SHA: &str = "1111111111111111111111111111111111111111";

fn main() {
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        let Ok(message) = serde_json::from_str::<serde_json::Value>(&line) else {
            continue;
        };
        let Some(id) = message.get("id").cloned() else {
            continue; // notifications get no reply
        };
        let tool = message
            .pointer("/params/name")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default();
        let reply = match (
            message.get("method").and_then(serde_json::Value::as_str),
            tool,
        ) {
            (Some("initialize"), _) => serde_json::json!({
                "jsonrpc": "2.0", "id": id,
                "result": {"protocolVersion": "2024-11-05", "capabilities": {"tools": {}},
                            "serverInfo": {"name": "fake-index-provider", "version": "1"}}
            }),
            (Some("tools/call"), "index_status") => serde_json::json!({
                "jsonrpc": "2.0", "id": id,
                "result": {
                    "content": [{"type": "text",
                                 "text": format!("{{\"git\":{{\"head_sha\":\"{HEAD_SHA}\"}}}}")}],
                    "isError": false
                }
            }),
            (Some("tools/call"), _) => serde_json::json!({
                "jsonrpc": "2.0", "id": id,
                "result": {
                    "structuredContent": {"total": 0, "cols": ["file"], "rows": [],
                                           "has_more": false},
                    "isError": false
                }
            }),
            _ => serde_json::json!({
                "jsonrpc": "2.0", "id": id,
                "error": {"code": -32601, "message": "method not found"}
            }),
        };
        let _ = stdout.write_all(reply.to_string().as_bytes());
        let _ = stdout.write_all(b"\n");
        let _ = stdout.flush();
    }
}
