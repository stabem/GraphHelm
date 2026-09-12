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

/// `..` resolved lexically, WITHOUT `canonicalize`.
///
/// `Path::canonicalize` on Windows returns a VERBATIM path (`\?\C:\...`), and verbatim paths are
/// taken literally by the OS: a `..` inside one is never resolved. So canonicalising
/// `<root>/apps/cli` and then joining `../../core/events` produces a path that cannot be opened,
/// every dependency edge fails, and the closure silently collapses to the one crate it started
/// from -- measured here as a population of 62 files, all of them `apps/cli`'s own. That collapse
/// is invisible to an absence assertion, which is why the population cells assert presence too.
fn normalised(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::ParentDir => {
                out.pop();
            }
            std::path::Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// The workspace crates the `graphhelm` binary actually embodies, as crate directories.
///
/// WHY THIS IS NOT "EVERY CRATE IN THE WORKSPACE": the staleness question is whether the binary
/// contains today's sources, and a crate the binary never links cannot make it stale. Comparing
/// against the whole workspace answers a wider question than the one asked, and the wider question
/// has a standing false positive in this repository: `ci/gate.ps1`'s canary REWRITES
/// `tools/ci-canary/src/nonce.rs` at the start of every run, and `ci-canary` is not a dependency of
/// `apps/cli`. In a cold run every artifact is rebuilt after that write, so the binary is newer and
/// nothing is noticed. With content-addressed artifact reuse the binary is not rebuilt -- its inputs
/// did not change -- so it keeps the previous run's mtime and the instrument refuses to measure a
/// product that is perfectly current (#1044).
///
/// The same mistake, one level out, is already recorded below: the first draft collected every
/// `.rs` in the workspace, so writing a TEST aged the instrument against itself. `src/` of a crate
/// the binary does not link is that mistake one level further in.
///
/// DERIVED BY WALKING THE MANIFESTS, never a hand-written roster -- a roster guards the roster, and
/// the crate added tomorrow would escape it silently. Dev-dependencies are excluded because they are
/// not linked into the binary; build-dependencies are INCLUDED because a build script's output is.
fn embodied_crate_dirs(root: &Path) -> Vec<PathBuf> {
    let mut seen: Vec<PathBuf> = Vec::new();
    let mut queue: Vec<PathBuf> = vec![root.join("apps").join("cli")];
    while let Some(candidate) = queue.pop() {
        let dir = normalised(&candidate);
        if seen.contains(&dir) {
            continue;
        }
        seen.push(dir.clone());
        let Ok(text) = std::fs::read_to_string(dir.join("Cargo.toml")) else {
            continue;
        };
        for relative in linked_path_dependencies(&text) {
            queue.push(dir.join(relative));
        }
    }
    seen
}

/// The `path = "..."` values of every LINKED dependency declared in a manifest.
///
/// SCANNED, NOT PARSED, AND THAT IS A DELIBERATE CONSTRAINT (#1044, M06 decision 5). `pathogens`
/// is gate machinery: the freeze forbids a change here travelling with anything outside the frozen
/// set, and adding a dependency necessarily moves `Cargo.lock`, which is gated code. The receipt
/// exemption does not cover it -- a receipt is unavoidable for ANY change, while a lockfile line
/// is unavoidable only GIVEN the choice to take a dependency, and that choice is avoidable. Taking
/// `toml` would also have pulled four third-party crates into the judge's surface, reviewed in no
/// pull request. So this reads the manifest itself.
///
/// It is section-aware rather than line-global, which is the whole difficulty: a bare search for
/// `path =` would collect `[dev-dependencies]` too, and dev-dependencies are NOT linked into the
/// binary -- widening the population is the exact defect this module is being changed to fix.
fn linked_path_dependencies(manifest: &str) -> Vec<String> {
    let mut found = Vec::new();
    let mut linked_section = false;
    for raw in manifest.lines() {
        let line = raw.trim();
        if line.starts_with('[') {
            linked_section = is_linked_dependency_header(line);
            continue;
        }
        if linked_section && let Some(value) = path_value(line) {
            found.push(value);
        }
    }
    found
}

/// Does this table header introduce dependencies that are LINKED into the binary?
///
/// `[dependencies]`, `[build-dependencies]`, `[dependencies.foo]` and the
/// `[target.'cfg(...)'.dependencies]` forms all qualify. `dev-dependencies` is tested FIRST and
/// rejected, because it contains the substring `dependencies` and a careless order would admit
/// exactly the table this function exists to exclude.
fn is_linked_dependency_header(header: &str) -> bool {
    let header = header.trim_start_matches('[').trim_end_matches(']');
    if header.contains("dev-dependencies") {
        return false;
    }
    header.contains("dependencies")
}

/// The quoted value of a `path` KEY on this line, or None.
///
/// The character before `path` must not be alphanumeric or `-`/`_`, so `paths`, `xpath` and
/// `search-path` do not match. Both shapes are covered: `foo = { path = "../bar" }` inline, and a
/// bare `path = "../bar"` under `[dependencies.foo]`.
fn path_value(line: &str) -> Option<String> {
    let bytes = line.as_bytes();
    let mut from = 0usize;
    while let Some(offset) = line[from..].find("path") {
        let at = from + offset;
        let before_ok = at == 0
            || !matches!(bytes[at - 1], b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'-' | b'_');
        let rest = &line[at + 4..];
        let after = rest.trim_start();
        if before_ok && after.starts_with('=') {
            let value = after[1..].trim_start();
            if let Some(stripped) = value.strip_prefix('"')
                && let Some(close) = stripped.find('"')
            {
                return Some(stripped[..close].to_owned());
            }
        }
        from = at + 4;
    }
    None
}

/// Every `.rs` under `src/` of a crate the binary embodies -- the sources it actually contains.
fn workspace_sources(root: &Path, into: &mut Vec<PathBuf>) {
    for dir in embodied_crate_dirs(root) {
        collect_rs_files(&dir.join("src"), into);
    }
}

fn collect_rs_files(directory: &Path, into: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_rs_files(&path, into);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            into.push(path);
        }
    }
}

/// The crate directories the binary embodies, for tests that need to reason about the population
/// itself rather than about one file's freshness.
#[must_use]
pub fn embodied_sources() -> Vec<PathBuf> {
    let mut sources = Vec::new();
    workspace_sources(&workspace_root(), &mut sources);
    sources
}

/// The measured failure was a refusal: a stale default-path binary against today's sources. **The
/// quiet direction is the same defect and matters more** — with a *fresh* binary sitting at the
/// default path, the instrument reports a confident green about a product this run never built.
///
/// DECLARED LIMIT: this honours the `CARGO_TARGET_DIR` environment variable only. Cargo also reads
/// `build.target-dir` from `.cargo/config.toml`, and a repository that grows one will reintroduce
/// exactly this divergence. There is no such file here today — checked, not assumed — so the gap
/// is named rather than covered, and the name is what a future reader needs.
fn target_dir(root: &Path) -> PathBuf {
    std::env::var_os("CARGO_TARGET_DIR").map_or_else(|| root.join("target"), PathBuf::from)
}

/// The binary this crate measures, or a NAMED refusal. Never a silent fallback.
///
/// # Errors
/// [`SubjectRefusal::Absent`] when nothing was built, [`SubjectRefusal::Stale`] when the
/// built binary predates a source it should contain.
pub fn measurable_binary() -> Result<PathBuf, SubjectRefusal> {
    let root = workspace_root();
    let binary = target_dir(&root).join("debug").join(if cfg!(windows) {
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
