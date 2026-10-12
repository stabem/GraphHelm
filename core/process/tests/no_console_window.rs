//! Workspace source guard for the no-console-window rule: every `.rs` file in the workspace
//! spawns through `graphhelm_process::command`, and any creation flags it sets include
//! `CREATE_NO_WINDOW`. Only the helper itself may construct a command directly.

use std::path::{Path, PathBuf};

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        let name = path.file_name().unwrap().to_string_lossy();
        if path.is_dir() {
            if !matches!(
                name.as_ref(),
                "target" | "node_modules" | ".git" | ".claude"
            ) {
                rust_files(&path, out);
            }
        } else if name.ends_with(".rs") {
            out.push(path);
        }
    }
}

fn offenders(root: &Path, helper: &Path) -> Vec<String> {
    let mut files = Vec::new();
    rust_files(root, &mut files);
    let construct = concat!("Command", "::new(");
    let flags = concat!("creation", "_flags(");
    let mut found = Vec::new();
    for path in files {
        if path == helper {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let lines: Vec<&str> = text.lines().collect();
        for (index, line) in lines.iter().enumerate() {
            let at = format!(
                "{}:{}",
                path.strip_prefix(root).unwrap().display(),
                index + 1
            );
            if line.contains(construct) {
                found.push(format!(
                    "{at} constructs a command; use graphhelm_process::command"
                ));
            }
            if line.contains(flags) {
                let call: String = lines[index..lines.len().min(index + 3)].concat();
                if !call.contains("CREATE_NO_WINDOW") {
                    found.push(format!("{at} sets creation flags without CREATE_NO_WINDOW"));
                }
            }
        }
    }
    found
}

#[test]
fn every_workspace_spawn_site_hides_its_console_window() {
    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let root = crate_dir.join("../..").canonicalize().unwrap();
    let helper = crate_dir.join("src/lib.rs").canonicalize().unwrap();
    let found = offenders(&root, &helper);
    assert!(
        found.is_empty(),
        "spawn through graphhelm_process::command:\n{}",
        found.join("\n")
    );
}

#[test]
fn the_guard_catches_a_bare_spawn_and_bare_flags() {
    let dir = std::env::temp_dir().join(format!("gh-nowindow-guard-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let bad = format!(
        "fn f() {{ std::process::{}\"git\"); c.{}0x4); }}\n",
        concat!("Command", "::new("),
        concat!("creation", "_flags(")
    );
    std::fs::write(dir.join("bad.rs"), bad).unwrap();
    let found = offenders(&dir, &dir.join("none.rs"));
    std::fs::remove_dir_all(&dir).unwrap();
    assert_eq!(found.len(), 2, "{found:?}");
}
