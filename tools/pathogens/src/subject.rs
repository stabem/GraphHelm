//! Locating the binary a black-box measurement will speak to — and REFUSING when the
//! answer would be a lie (M08, step 4's pre-check).
//!
//! An instrument that measures the product is gate machinery, so it lives here rather than
//! beside the code it measures. That has a consequence measured before anything was built:
//! `cargo test -p pathogens` does NOT put the CLI binary in this crate's build graph, so
//! the binary may be missing entirely. And the worse case is not missing — it is a binary
//! that EXISTS and is STALE, because then the counter measures a previous version of the
//! product and reports green. Measuring the wrong thing while looking measured is the
//! defect this milestone has now paid for four times.
//!
//! So both cases are refusals, named:
//! - absent  -> "cannot ask: no binary"
//! - stale   -> "cannot ask: the binary is older than the code it should be"
//! - fresh   -> measure
//!
//! This is the milestone's own principle turned on ourselves: "I do not know" must be
//! representable in the product's surfaces AND in our instruments. Nothing here builds
//! anything: a `cargo build` inside a test that already runs under `cargo test` contends
//! for the build-directory lock, and a gate stage that hangs is worse than any defect the
//! counter could find (Agent B's operational refusal of that route, 2026-08-18).
//!
//! HONEST LIMIT, decided by Agent B and written where it applies: mtime is a heuristic. A
//! checkout that rewrites modification times can produce a spurious refusal. In this
//! direction the error is the right one — a spurious refusal is NOISE, a false green is a
//! LIE — but a reader who hits it deserves to know it was a choice, not an accident.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// Why a measurement cannot be trusted to speak to the current product.
#[derive(Debug, PartialEq, Eq)]
pub enum SubjectRefusal {
    /// No binary at all: `cargo test -p pathogens` never builds one.
    Absent { path: PathBuf },
    /// A binary older than a source it should embody.
    Stale {
        path: PathBuf,
        newer_source: PathBuf,
    },
}

impl std::fmt::Display for SubjectRefusal {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Absent { path } => write!(
                formatter,
                "cannot ask: no binary at {}. Build the workspace first; this measurement \
                 refuses rather than reporting a green it did not earn",
                path.display()
            ),
            Self::Stale { path, newer_source } => write!(
                formatter,
                "cannot ask: the binary at {} is OLDER than {}, so it is not the product \
                 this commit describes. Measuring it would report a previous version as if \
                 it were this one",
                path.display(),
                newer_source.display()
            ),
        }
    }
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("the pathogens crate sits two levels below the workspace root")
        .to_path_buf()
}

/// Every `.rs` under a `src/` directory in the workspace — the sources the binary embodies.
/// Derived by walking, never a hand-written list: a roster guards the roster, and the next
/// file added would escape it silently (the M08 Task 0 lesson, applied here).
fn workspace_sources(root: &Path, into: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if path.is_dir() {
            if name == "target" || name == ".git" || name.starts_with('.') {
                continue;
            }
            workspace_sources(&path, into);
        } else if path.extension().is_some_and(|extension| extension == "rs")
            && path
                .components()
                .any(|component| component.as_os_str() == "src")
        {
            // Only `src/` is embodied by the binary. The first draft collected every `.rs`
            // in the workspace, which meant writing a TEST aged the instrument against
            // itself and it refused forever — a guard that always refuses. The doc comment
            // above already said `src/`; the code did not, and the freshness test caught
            // the disagreement between them.
            into.push(path);
        }
    }
}

/// The binary this crate measures, or a NAMED refusal. Never a silent fallback.
///
/// # Errors
/// [`SubjectRefusal::Absent`] when nothing was built, [`SubjectRefusal::Stale`] when the
/// built binary predates a source it should contain.
pub fn measurable_binary() -> Result<PathBuf, SubjectRefusal> {
    let root = workspace_root();
    let binary = root.join("target").join("debug").join(if cfg!(windows) {
        "graphhelm.exe"
    } else {
        "graphhelm"
    });
    let built_at = match std::fs::metadata(&binary).and_then(|meta| meta.modified()) {
        Ok(modified) => modified,
        Err(_) => return Err(SubjectRefusal::Absent { path: binary }),
    };
    let mut sources = Vec::new();
    workspace_sources(&root, &mut sources);
    let newest = sources
        .into_iter()
        .filter_map(|path| {
            std::fs::metadata(&path)
                .and_then(|meta| meta.modified())
                .ok()
                .map(|modified| (modified, path))
        })
        .max_by_key(|(modified, _)| *modified);
    match newest {
        Some((modified, source)) if modified > built_at => Err(SubjectRefusal::Stale {
            path: binary,
            newer_source: source,
        }),
        // No readable source at all is not a clean bill of health either, but it cannot be
        // distinguished from an empty checkout here; the caller sees the binary and decides.
        _ => Ok(binary),
    }
}

/// The instant a source was last written, for tests that need to reason about freshness.
#[must_use]
pub fn modified_at(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path)
        .and_then(|meta| meta.modified())
        .ok()
}
