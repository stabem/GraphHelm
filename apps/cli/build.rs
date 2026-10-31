//! Records the git commit this binary is built from, so a proof can tell a build of the head
//! from an old install. Reads the repository's files and starts no process; with no repository
//! (a source archive) the commit is `unknown`.

use std::fs;
use std::path::{Path, PathBuf};

fn main() {
    let found = std::env::var_os("CARGO_MANIFEST_DIR")
        .map(PathBuf::from)
        .and_then(|manifest| build_commit(&manifest));
    match found {
        Some((commit, watched)) => {
            println!("cargo:rustc-env=GRAPHHELM_BUILD_COMMIT={commit}");
            for path in watched {
                println!("cargo:rerun-if-changed={}", path.display());
            }
        }
        None => {
            println!("cargo:rustc-env=GRAPHHELM_BUILD_COMMIT=unknown");
            println!("cargo:rerun-if-changed=build.rs");
        }
    }
}

/// The commit `HEAD` names and the files whose change moves it, for a checkout or a linked worktree.
fn build_commit(start: &Path) -> Option<(String, Vec<PathBuf>)> {
    let dot_git = start
        .ancestors()
        .map(|dir| dir.join(".git"))
        .find(|path| path.exists())?;
    let git_dir = if dot_git.is_file() {
        let text = fs::read_to_string(&dot_git).ok()?;
        let target = text.trim().strip_prefix("gitdir:")?.trim();
        dot_git.parent()?.join(target)
    } else {
        dot_git
    };
    let common_dir = match fs::read_to_string(git_dir.join("commondir")) {
        Ok(text) => git_dir.join(text.trim()),
        Err(_) => git_dir.clone(),
    };
    let head_file = git_dir.join("HEAD");
    let head = fs::read_to_string(&head_file).ok()?;
    let mut watched = vec![head_file];
    let Some(reference) = head.trim().strip_prefix("ref:").map(str::trim) else {
        return as_commit(head.trim()).map(|commit| (commit, watched));
    };
    for dir in [&git_dir, &common_dir] {
        let loose = dir.join(reference);
        if let Ok(text) = fs::read_to_string(&loose) {
            watched.push(loose);
            return as_commit(text.trim()).map(|commit| (commit, watched));
        }
        // A packed ref is rewritten as a loose file on the next commit: watch the nearest existing
        // directory that file will appear in, so its creation reruns this script (#2277).
        if let Some(dir) = loose.ancestors().skip(1).find(|path| path.is_dir()) {
            watched.push(dir.to_path_buf());
        }
    }
    let packed = common_dir.join("packed-refs");
    let text = fs::read_to_string(&packed).ok()?;
    watched.push(packed);
    text.lines()
        .filter_map(|line| line.split_once(' '))
        .find(|(_, name)| *name == reference)
        .and_then(|(sha, _)| as_commit(sha))
        .map(|commit| (commit, watched))
}

fn as_commit(text: &str) -> Option<String> {
    let valid = text.len() == 40
        && text
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
    valid.then(|| text.to_owned())
}
