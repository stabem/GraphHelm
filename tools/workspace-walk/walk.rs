// The workspace-wide file walk shared by guards whose subject is the whole repository rather than
// one crate.
//
// INCLUDED BY VALUE, like `tools/source-invariants/detect.rs`, and for the same reason: an
// integration test cannot depend on another crate's test target, and a workspace-scoped guard needs
// the same POPULATION as every other workspace-scoped guard or their results are not comparable.
//
// WHAT MAKES THIS WORTH SHARING IS THE EXCLUSION SET, NOT THE RECURSION. Two sweeps that must agree
// on what they skip is a duplicated ORACLE and not a duplicated mechanism: add `node_modules` to one
// and not the other and the two disagree in silence, both green, each reporting a clean workspace
// about a different workspace. This repository already sets that bar at two copies -- "The mapping
// lives HERE and nowhere else: a second spelling of it would drift" (`install.rs`).
//
// No `//!` in this file: an `include!`d file cannot carry inner doc comments (E0753).

use std::path::{Path, PathBuf};

/// The workspace root, found by walking UP to the `Cargo.toml` that declares `[workspace]`.
///
/// Not `CARGO_MANIFEST_DIR` plus a fixed number of parents. That form is correct only for an
/// includer at one particular depth, and the whole point of extracting this file is that the next
/// includer sits somewhere else -- `adapters/<name>` and `core/<name>` are both two deep today, and
/// nothing promises the third one is.
#[allow(dead_code)]
fn workspace_root() -> PathBuf {
    let start = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mut candidate = start.as_path();
    loop {
        let manifest = candidate.join("Cargo.toml");
        if std::fs::read_to_string(&manifest)
            .is_ok_and(|text| text.lines().any(|line| line.trim() == "[workspace]"))
        {
            return candidate.to_path_buf();
        }
        candidate = candidate
            .parent()
            .unwrap_or_else(|| panic!("no [workspace] manifest above {}", start.display()));
    }
}

/// The crate directories, read from the workspace `Cargo.toml` at RUN time.
///
/// A guard has two hand-chosen parameters -- its POPULATION and its FORM -- and measuring one
/// leaves the other a guess. Deriving the population removes one of them from the guessing: a crate
/// added to the workspace is swept the day it is added, with nobody remembering to widen a list.
#[allow(dead_code)]
fn workspace_members() -> Vec<PathBuf> {
    let root = workspace_root();
    let text = std::fs::read_to_string(root.join("Cargo.toml")).expect("workspace Cargo.toml");
    let mut inside = false;
    let mut members = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("members") {
            inside = true;
            continue;
        }
        if inside {
            if trimmed.starts_with(']') {
                break;
            }
            if let Some(name) = trimmed.split('"').nth(1) {
                members.push(root.join(name));
            }
        }
    }
    members
}

#[allow(dead_code)]
const WORKSPACE_WALK_MAX_ENTRIES: usize = 8_192;

/// Directories no workspace guard may descend into.
///
/// `.claude` is the one worth writing down: in the primary checkout it holds `worktrees/`, which
/// contains COMPLETE copies of this repository belonging to other sessions. A walk that descended
/// into it would sweep other people's uncommitted work and report findings about files that are not
/// in the change set at all -- and it would do so intermittently, depending on who happened to be
/// mid-edit, which is the worst shape a guard failure can take.
#[allow(dead_code)]
const WORKSPACE_WALK_SKIPPED: [&str; 4] = ["target", ".git", ".claude", "node_modules"];

/// Every file under `start` with one of `extensions`, skipping build and VCS trees.
#[allow(dead_code)]
fn walk(start: &Path, extensions: &[&str], out: &mut Vec<PathBuf>) {
    assert!(
        out.len() <= WORKSPACE_WALK_MAX_ENTRIES,
        "HARNESS-BROKE: the walk passed {WORKSPACE_WALK_MAX_ENTRIES} entries; it is not measuring \
         what it claims"
    );
    let Ok(entries) = std::fs::read_dir(start) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if path.is_dir() {
            if WORKSPACE_WALK_SKIPPED.contains(&name.as_ref()) {
                continue;
            }
            walk(&path, extensions, out);
        } else if extensions
            .iter()
            .any(|extension| path.extension().is_some_and(|found| found == *extension))
        {
            out.push(path);
        }
    }
}

/// A workspace-relative path with forward slashes, so a failure message reads the same on every
/// platform and can be pasted into a `git` command.
#[allow(dead_code)]
fn relative(path: &Path) -> String {
    path.strip_prefix(workspace_root())
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

/// Where this shared file lives, for a failure message that has to send a reader somewhere.
///
/// `file!()` expands at the INCLUDE site's expansion, so this reports the shared file rather than
/// the includer -- which is the whole point: a guard failing on a walk defect should send its
/// reader here and not to whichever test happened to notice.
#[allow(dead_code)]
fn workspace_walk_self_path() -> &'static str {
    file!()
}
