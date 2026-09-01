//! A deterministic MCP-shaped provider for the producer's cells: newline-delimited JSON-RPC on
//! stdio, answering `initialize` and one `tools/call` from a canned fixture. Modes via argv:
//! `fixture` answers the search fixture; `garbage` answers bytes that parse as nothing;
//! `echo-cache` answers with the bytes of `$CBM_CACHE_DIR/graph.bin` — the real provider reads
//! its store from `CBM_CACHE_DIR` (measured against codebase-memory-mcp 0.10.8), so this mode
//! is how a cell observes WHICH bytes the funnel actually serves there.

use std::io::{BufRead, Write};

fn main() {
    let mode = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "fixture".to_owned());
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
        let reply = match message.get("method").and_then(|method| method.as_str()) {
            Some("initialize") => serde_json::json!({
                "jsonrpc": "2.0", "id": id,
                "result": {"protocolVersion": "2024-11-05", "capabilities": {"tools": {}},
                            "serverInfo": {"name": "fake-mcp-server", "version": "1"}}
            }),
            Some("tools/call") if mode == "echo-cache" => {
                let cache_bytes = std::env::var("CBM_CACHE_DIR")
                    .ok()
                    .and_then(|dir| {
                        std::fs::read_to_string(std::path::Path::new(&dir).join("graph.bin")).ok()
                    })
                    .unwrap_or_else(|| "MISSING".to_owned());
                serde_json::json!({
                    "jsonrpc": "2.0", "id": id,
                    "result": {"structuredContent": {"cache_bytes": cache_bytes}, "isError": false}
                })
            }
            Some("tools/call") if mode == "garbage" => {
                let _ = stdout.write_all(b"not json at all\n");
                let _ = stdout.flush();
                continue;
            }
            // The real provider's measured contract (codebase-memory-mcp 0.10.8): without
            // `"format":"json"` in the arguments, search_graph answers a human-readable text
            // block in `content` and NO structuredContent — which the house decoder refuses.
            // The fake answers the same way, so a producer that forgets the argument goes red
            // here instead of green against a fake more generous than the real thing.
            Some("tools/call")
                if message
                    .pointer("/params/arguments/format")
                    .and_then(|value| value.as_str())
                    != Some("json") =>
            {
                serde_json::json!({
                    "jsonrpc": "2.0", "id": id,
                    "result": {
                        "content": [{"type": "text",
                                     "text": "total: 2\nresults: 2  (cols: qn label file lines rank)\n"}],
                        "isError": false
                    }
                })
            }
            Some("tools/call") => serde_json::json!({
                "jsonrpc": "2.0", "id": id,
                "result": {
                    "structuredContent": {
                        "total": 2,
                        "cols": ["qn", "label", "file", "lines", "rank"],
                        "rows": [
                            ["p.core.events.src.local.open_failure", "Function",
                             "core/events/src/local.rs", "3546-3552", -20.0],
                            ["p.core.events.src.store.code", "Function",
                             "core/events/src/store.rs", "60-80", -18.0]
                        ],
                        "has_more": false
                    },
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
