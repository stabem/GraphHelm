//! #2573 source guard: no GraphHelm-launched child opens a console window on Windows.
//!
//! A `detached` child has no console, so every console program it starts later gets a new
//! visible window (`windowsHide` on the parent does not reach it). Every spawn therefore goes
//! through `graphhelm_process::command` / `hide_window`, which sets `CREATE_NO_WINDOW`, and no
//! source file names the detached-process creation flag. This test fails when either rule is broken.

use std::fs;
use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        if path.is_dir() {
            if matches!(
                name.as_str(),
                "target" | ".git" | "fixtures" | "node_modules"
            ) {
                continue;
            }
            rust_files(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            out.push(path);
        }
    }
}

/// The one file allowed to construct a `Command` directly: the helper itself.
const HELPER: &str = "core/process/src/lib.rs";

#[test]
fn no_source_file_uses_detached_process() {
    let mut files = Vec::new();
    rust_files(&repo_root(), &mut files);
    let offenders: Vec<String> = files
        .iter()
        .filter(|path| {
            fs::read_to_string(path)
                .unwrap()
                .contains(concat!("DETACHED", "_PROCESS"))
        })
        .map(|path| path.display().to_string())
        .collect();
    assert!(
        offenders.is_empty(),
        "{} is forbidden (#2573): {offenders:?}",
        concat!("DETACHED", "_PROCESS")
    );
}

#[test]
fn no_source_file_spawns_without_the_no_window_helper() {
    let mut files = Vec::new();
    rust_files(&repo_root(), &mut files);
    let offenders: Vec<String> = files
        .iter()
        .filter(|path| {
            let is_helper = path
                .to_string_lossy()
                .replace(char::from(92), "/")
                .ends_with(HELPER);
            !is_helper
                && fs::read_to_string(path)
                    .unwrap()
                    .contains(concat!("Command", "::new("))
        })
        .map(|path| path.display().to_string())
        .collect();
    assert!(
        offenders.is_empty(),
        "spawn with graphhelm_process::command, not Command::new (#2573): {offenders:?}"
    );
}
