//! `graphhelm journey report <id> --html <out>` (#1448): one self-contained page for a journey,
//! so the owner can read it in a chat or a browser without the Studio. Each step of the main path
//! shows its real frame (inlined), the action that led to it, what the screen must show with what
//! the run saw, and the run's overall result. It reads what `journey flows` and `journey preview
//! --read` read, in this process; it runs no browser and starts no preview.
use std::fmt::Write as _;

use serde_json::{Value, json};

use crate::args::JourneyReportArgs;
use crate::output::Outcome;

const COMMAND: &str = "journey.report";

pub(crate) fn run(args: &JourneyReportArgs) -> Outcome {
    let project = args.project.clone().unwrap_or_else(|| ".".into());
    let project = match project.canonicalize() {
        Ok(project) => project,
        Err(_) => {
            return refuse(
                "preview.project_invalid",
                "the project directory cannot be read",
            );
        }
    };
    let (flow, kept) = match super::journey_preview::kept(&project, &args.id) {
        Ok(found) => found,
        Err(failed) => return refuse(failed.0, "the journey flow cannot be read"),
    };
    if kept["state"] == "none" {
        return refuse(
            "report.preview_missing",
            "no kept preview for this flow and commit; run `graphhelm journey preview <id>` first",
        );
    }
    let (page, frames) = render(&flow, &kept, &|screen| {
        super::journey_preview::screen_frame(&project, &args.id, screen).map(|frame| frame.0)
    });
    if super::atomic::atomic_write(&args.html, page.as_bytes()).is_err() {
        return refuse("report.write_failed", "the report file cannot be written");
    }
    Outcome::success(
        COMMAND,
        json!({"id": args.id, "html": args.html, "bytes": page.len(), "frames": frames,
            "state": kept["state"], "result": kept["result"]}),
    )
}

fn refuse(code: &'static str, message: &str) -> Outcome {
    Outcome::application(
        COMMAND,
        graphhelm_protocols::Diagnostic::error(code, message, "/id", "journey-cli"),
    )
}

/// The page is meant to be shared, so the launcher's stderr loses user-home paths (this
/// machine's home and any drive `:\Users\x`, `/home/x`, `/Users/x`) and token-shaped words (#1543).
fn mask_shared(text: &str) -> String {
    let mut text = text.to_owned();
    for key in ["USERPROFILE", "HOME"] {
        if let Some(home) = std::env::var(key).ok().filter(|home| home.len() > 3) {
            text = text
                .replace(&home, "~")
                .replace(&home.replace('\\', "/"), "~");
        }
    }
    let chars: Vec<char> = text.chars().collect();
    let sep = |c: char| c == '/' || c == '\\';
    let word =
        |c: char| !(c.is_whitespace() || sep(c) || matches!(c, '"' | '\'' | ':' | ',' | ';'));
    let starts = |at: usize, prefix: &str| {
        prefix.chars().enumerate().all(|(k, p)| {
            chars
                .get(at + k)
                .is_some_and(|c| c.eq_ignore_ascii_case(&p))
        })
    };
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < chars.len() {
        let skip = if starts(i, "/home/") {
            6
        } else if starts(i, "/users/") {
            7
        } else if chars[i].is_ascii_alphabetic()
            && chars.get(i + 1) == Some(&':')
            && chars.get(i + 2).copied().is_some_and(sep)
            && starts(i + 3, "users")
            && chars.get(i + 8).copied().is_some_and(sep)
        {
            9
        } else {
            0
        };
        if skip > 0 && chars.get(i + skip).copied().is_some_and(word) {
            out.push('~');
            i += skip;
            while i < chars.len() && word(chars[i]) {
                i += 1;
            }
            continue;
        }
        if chars[i].is_ascii_alphanumeric() && (i == 0 || !chars[i - 1].is_ascii_alphanumeric()) {
            let end = (i..chars.len())
                .find(|&j| {
                    !(chars[j].is_ascii_alphanumeric() || matches!(chars[j], '_' | '-' | '+' | '='))
                })
                .unwrap_or(chars.len());
            let run = &chars[i..end];
            if run.len() >= 32
                && run.iter().any(char::is_ascii_digit)
                && run.iter().any(char::is_ascii_alphabetic)
            {
                out.push_str("***");
                i = end;
                continue;
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            c => out.push(c),
        }
    }
    out
}

fn text(value: &Value) -> &str {
    value.as_str().unwrap_or_default()
}

fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0u32, |n, (i, b)| n | u32::from(*b) << (16 - 8 * i));
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(char::from(ALPHABET[(n >> (18 - 6 * i) & 63) as usize]));
            } else {
                out.push('=');
            }
        }
    }
    out
}

fn colour(result: &str) -> &'static str {
    match result {
        "pass" | "ready" => "#1D9E75",
        "drift" | "failed" => "#BA7517",
        _ => "#888780",
    }
}

/// The screens of the `main` path in order, and the edge that leads to each after the first.
fn main_order<'a>(flow: &'a Value) -> Vec<(&'a Value, Option<&'a Value>)> {
    let edges = flow["edges"].as_array().map(Vec::as_slice).unwrap_or(&[]);
    let find = |id: &str, list: &'a [Value]| list.iter().find(|item| text(&item["id"]) == id);
    let screens = flow["screens"].as_array().map(Vec::as_slice).unwrap_or(&[]);
    let path = flow["paths"]["main"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    let mut order = Vec::new();
    for (i, edge_id) in path.iter().enumerate() {
        let Some(edge) = find(text(edge_id), edges) else {
            continue;
        };
        if i == 0
            && let Some(first) = find(text(&edge["from"]), screens)
        {
            order.push((first, None));
        }
        if let Some(screen) = find(text(&edge["to"]), screens) {
            order.push((screen, Some(edge)));
        }
    }
    order
}

fn acts(edge: &Value) -> String {
    edge["acts"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|act| format!("{} \"{}\"", text(&act["kind"]), text(&act["name"])))
        .collect::<Vec<_>>()
        .join(", ")
}

/// The page and how many frames it inlines. `frame` answers a screen's PNG bytes.
fn render(flow: &Value, kept: &Value, frame: &dyn Fn(&str) -> Option<Vec<u8>>) -> (String, usize) {
    let mut cards = String::new();
    let mut inlined = 0;
    for (i, (screen, edge)) in main_order(flow).into_iter().enumerate() {
        let id = text(&screen["id"]);
        let run = &kept["screens"][id];
        let result = run["result"].as_str().unwrap_or("not run");
        let mut pill = escape(result);
        if let Some(reason) = run["reason"].as_str() {
            let _ = write!(pill, " · {}", escape(reason));
        }
        let mut card = format!(
            "<section><div class=\"hd\"><span class=\"n\">{}</span><b>{}</b><span class=\"pill\" style=\"background:{}\">{pill}</span></div>",
            i + 1,
            escape(text(&screen["title"])),
            colour(result),
        );
        if let Some(edge) = edge {
            let ran = &kept["edges"][text(&edge["id"])];
            let outcome = ran["result"].as_str().unwrap_or("not run");
            let _ = write!(
                card,
                "<div class=\"act\">Action: {} <span style=\"color:{}\">&rarr; {}{}</span></div>",
                escape(&acts(edge)),
                colour(outcome),
                escape(outcome),
                ran["reason"]
                    .as_str()
                    .map(|r| format!(" ({})", escape(r)))
                    .unwrap_or_default(),
            );
        }
        match frame(id) {
            Some(bytes) => {
                inlined += 1;
                let _ = write!(
                    card,
                    "<img alt=\"{}\" src=\"data:image/png;base64,{}\">",
                    escape(text(&screen["title"])),
                    base64(&bytes)
                );
            }
            None => card.push_str("<div class=\"nof\">No screenshot recorded for this step</div>"),
        }
        let seen = run["checks"].as_array().filter(|checks| !checks.is_empty());
        let items: String = match seen {
            Some(checks) => checks
                .iter()
                .map(|c| {
                    format!(
                        "<li>{} {}{} \"{}\"{}</li>",
                        if c["seen"] == true {
                            "&#10003;"
                        } else {
                            "&#10007;"
                        },
                        if c["absent"] == true { "no " } else { "" },
                        escape(text(&c["role"])),
                        escape(text(&c["name"])),
                        // #1781: a text check says what it wanted and what the element held.
                        if c["text"].is_string() {
                            format!(
                                " saying \"{}\" (held \"{}\")",
                                escape(text(&c["text"])),
                                escape(text(&c["actual"]))
                            )
                        } else {
                            String::new()
                        }
                    )
                })
                .collect(),
            None => screen["expect"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|c| {
                    format!(
                        "<li>&middot; {}{} \"{}\"</li>",
                        if c["absent"] == true { "no " } else { "" },
                        escape(text(&c["role"])),
                        escape(text(&c["name"]))
                    )
                })
                .collect(),
        };
        let _ = write!(
            card,
            "<div class=\"ex\"><div class=\"lbl\">What the screen must show</div><ul>{items}</ul></div></section>"
        );
        cards.push_str(&card);
    }
    let overall = kept["result"]
        .as_str()
        .or(kept["state"].as_str())
        .unwrap_or("unknown");
    let mut why = String::new();
    if let Some(code) = kept["detail"]["code"].as_str().or(kept["reason"].as_str()) {
        let _ = write!(why, " &middot; {}", escape(code));
    }
    let log = kept["launchLog"]["tail"]
        .as_str()
        .map(|tail| format!("<pre>{}</pre>", escape(&mask_shared(tail))))
        .unwrap_or_default();
    let title = flow["title"].as_str().unwrap_or_else(|| text(&flow["id"]));
    let page = format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\"><title>Journey {id}</title><style>{STYLE}</style></head><body><h1>{title}</h1><p class=\"p\">{purpose}<br>Overall: <b style=\"color:{colour}\">{overall}</b>{why} &middot; ran {ran} &middot; commit {commit}</p>{log}{cards}</body></html>",
        id = escape(text(&flow["id"])),
        title = escape(title),
        purpose = escape(text(&flow["purpose"])),
        colour = colour(overall),
        overall = escape(overall),
        ran = escape(text(&kept["ranAt"])),
        commit = escape(text(&kept["commit"]).get(..8).unwrap_or_default()),
    );
    (page, inlined)
}

const STYLE: &str = "body{background:#0b0b0b;color:#e8e6e1;font-family:system-ui,sans-serif;margin:0;padding:16px;max-width:1000px}h1{font-size:20px;margin:0 0 4px}.p{color:#a3a19a;font-size:14px;margin:0 0 16px}section{border:1px solid #2c2c2a;border-radius:12px;margin:0 0 16px;overflow:hidden;background:#141413}.hd{display:flex;gap:10px;align-items:center;padding:10px 14px;border-bottom:1px solid #2c2c2a}.n{background:#2c2c2a;border-radius:50%;width:24px;height:24px;display:inline-flex;align-items:center;justify-content:center;font-size:12px}.pill{margin-left:auto;font-size:12px;padding:2px 10px;border-radius:8px;color:#fff}img{display:block;width:100%}.nof{padding:40px;text-align:center;color:#888780}.ex{padding:10px 14px;font-size:14px}.lbl{color:#888780;font-size:12px}ul{margin:6px 0 0;padding-left:18px}.act{padding:8px 14px;font-size:13px;color:#c9c7c0;border-bottom:1px solid #2c2c2a}pre{background:#141413;border:1px solid #2c2c2a;border-radius:8px;padding:10px;font-size:12px;white-space:pre-wrap;overflow:auto}";
