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

/// The head this run reports: the fixed synthetic sha unless the STORE carries an override.
///
/// The override exists for the #637 tree-derivation cells, which need the fake to report a
/// COMMIT THAT EXISTS in the cell's fixture repository -- a fixed synthetic sha can never
/// resolve to a tree, so those cells could only ever exercise the "underivable" refusal.
///
/// It travels INSIDE the store (a `head.txt` beside `graph.db`) rather than as an environment
/// variable, because the containment funnel is the point: the child gets a sanitised
/// environment, so a cell's env var never arrives -- measured, the first version of this
/// override used env and the cell watched the fixed sha come back. The store copy is the one
/// channel the funnel deliberately forwards (`CBM_CACHE_DIR`), which also mirrors the real
/// provider: its reported head comes from the store it reads, not from its caller.
fn head_sha() -> String {
    std::env::var("CBM_CACHE_DIR")
        .ok()
        .and_then(|cache| {
            std::fs::read_to_string(std::path::Path::new(&cache).join("head.txt")).ok()
        })
        .map(|text| text.trim().to_owned())
        .filter(|text| !text.is_empty())
        .unwrap_or_else(|| HEAD_SHA.to_owned())
}

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
                                 "text": format!("{{\"git\":{{\"head_sha\":\"{}\"}}}}", head_sha())}],
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
