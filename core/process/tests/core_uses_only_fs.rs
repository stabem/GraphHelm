//! #1673 (follow-up to #1338): `core/schema`, `core/events` and `core/extension-host` depend on this
//! crate only for its dependency-free `fs` module. AGENTS.md forbids executing commands during
//! validation, lint, replay or simulation, so those crates must never reach this crate's spawn API.
//! Any mention of the crate other than a `graphhelm_process::fs::` path is refused: a direct call,
//! a braced `use` list, a glob or an alias all hide what is used.

use std::path::{Path, PathBuf};

const CORE_CRATES: [&str; 3] = [
    "core/schema/src",
    "core/events/src",
    "core/extension-host/src",
];
const CRATE: &str = concat!("graphhelm", "_process");

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            out.push(path);
        }
    }
}

/// Every use of the crate in `text` that is not a `graphhelm_process::fs::` path, as `line: text`.
fn offending(text: &str) -> Vec<String> {
    let allowed = format!("{CRATE}::fs::");
    let mut found = Vec::new();
    for (index, line) in text.lines().enumerate() {
        let mut rest = line;
        while let Some(at) = rest.find(CRATE) {
            if !rest[at..].starts_with(&allowed) {
                found.push(format!("{}: {}", index + 1, line.trim()));
                break;
            }
            rest = &rest[at + allowed.len()..];
        }
    }
    found
}

#[test]
fn core_crates_use_only_the_fs_module_of_the_process_crate() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut found = Vec::new();
    for dir in CORE_CRATES {
        let mut files = Vec::new();
        rust_files(&root.join(dir), &mut files);
        assert!(
            !files.is_empty(),
            "{dir} holds no Rust source: the guard would scan nothing"
        );
        for path in files {
            let text = std::fs::read_to_string(&path).unwrap();
            for hit in offending(&text) {
                found.push(format!(
                    "{}:{hit}",
                    path.strip_prefix(&root).unwrap().display()
                ));
            }
        }
    }
    assert!(
        found.is_empty(),
        "core crates may use only {CRATE}::fs, never its spawn API:\n{}",
        found.join("\n")
    );
}

#[test]
fn the_scan_refuses_every_form_but_an_fs_path() {
    for bad in [
        format!("let c = {CRATE}::command(\"git\");"),
        format!("use {CRATE}::{{fs, command}};"),
        format!("use {CRATE} as p;"),
        format!("use {CRATE}::*;"),
        format!("{CRATE}::fs::is_reparse_point(m) && {CRATE}::hide_window(c)"),
    ] {
        assert_eq!(offending(&bad).len(), 1, "{bad}");
    }
    let good = format!("{CRATE}::fs::is_reparse_point(m) || {CRATE}::fs::is_reparse_point(n)");
    assert!(offending(&good).is_empty(), "{good}");
}
