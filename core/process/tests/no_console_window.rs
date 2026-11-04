//! Workspace source guard for the no-console-window rule: every `.rs` file in the workspace
//! spawns through `graphhelm_process::command`, and any creation flags it sets include
//! `CREATE_NO_WINDOW`. Only the helper itself may construct a command directly.

use std::path::{Path, PathBuf};

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        let name = path.file_name().unwrap().to_string_lossy();
        if path.is_dir() {
            // A frozen copy of source read as data by the context-quality tests, never compiled or
            // run; its bytes are hashed, so it keeps the spawn it was copied with (#1374, #1705).
            if path.ends_with("tests/fixtures/context-quality/tree")
                || path.ends_with("tests/fixtures/context-holdout/tree")
                || path.ends_with("tests/fixtures/context-holdout-v2/tree")
            {
                continue;
            }
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

/// Browser launchers and Node child callers must load the shared hidden launcher first.
const HIDE_MARKER: &str = "hidden-node-children.mjs";

fn script_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        if path.is_dir() {
            if !matches!(name.as_str(), "target" | "node_modules" | ".git") {
                script_files(&path, out);
            }
        } else if name.ends_with(".mjs") || name.ends_with(".js") || name.ends_with(".cjs") {
            out.push(path);
        }
    }
}

fn browser_launchers_without_the_hide(root: &Path) -> (usize, Vec<String>) {
    let mut files = Vec::new();
    for dir in ["tools", "docs/journeys", "adapters/model-gateway/tests"] {
        script_files(&root.join(dir), &mut files);
    }
    let launches = [
        concat!("chromium", ".launch("),
        concat!("firefox", ".launch("),
        concat!("webkit", ".launch("),
    ];
    let (mut launchers, mut found) = (0, Vec::new());
    for path in files {
        if path
            .file_name()
            .is_some_and(|name| name == "hidden-node-children.mjs")
        {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let first_launch = launches.iter().filter_map(|needle| text.find(needle)).min();
        let first_child_import = [
            "'node:child_process'",
            "\"node:child_process\"",
            "'child_process'",
            "\"child_process\"",
        ]
        .iter()
        .filter_map(|needle| text.find(needle))
        .min();
        let Some(first_call) = first_launch.into_iter().chain(first_child_import).min() else {
            continue;
        };
        launchers += 1;
        let at = path.strip_prefix(root).unwrap().display().to_string();
        match text.find(HIDE_MARKER) {
            None => found.push(format!("{at} bypasses the shared hidden Node launcher")),
            Some(hide) if hide > first_call => {
                found.push(format!(
                    "{at} loads the hidden launcher only after its first call"
                ));
            }
            Some(_) => {}
        }
    }
    (launchers, found)
}

#[test]
fn every_browser_launching_script_hides_the_browsers_console_window() {
    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let root = crate_dir.join("../..").canonicalize().unwrap();
    let (launchers, found) = browser_launchers_without_the_hide(&root);
    // Non-empty is not a control: a broken walk would find no launcher and pass vacuously. The
    // floor names the current browser and child-process callers under the scanned paths.
    assert!(
        launchers >= 6,
        "expected at least 6 browser or child-process callers, found {launchers}"
    );
    assert!(
        found.is_empty(),
        "use the shared hidden Node launcher:\n{}",
        found.join("\n")
    );
}

#[test]
fn the_guard_catches_a_commonjs_child_launch_without_the_shared_launcher() {
    let root = std::env::temp_dir().join(format!("gh-nowindow-cjs-{}", std::process::id()));
    for dir in ["tools", "docs/journeys", "adapters/model-gateway/tests"] {
        std::fs::create_dir_all(root.join(dir)).unwrap();
    }
    std::fs::write(
        root.join("tools/probe.cjs"),
        "require('node:child_process').spawn('node');\n",
    )
    .unwrap();
    let (launchers, found) = browser_launchers_without_the_hide(&root);
    std::fs::remove_dir_all(&root).unwrap();
    assert_eq!(launchers, 1);
    assert!(
        found.iter().any(|item| item.contains("probe.cjs bypasses")),
        "{found:?}"
    );
}
