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
            // CANONICAL here, at the one boundary that produces a root, so every consumer below
            // compares and walks the same form (Codex, #578). The previous change canonicalized in
            // exactly one place -- `walk_all`'s deduplication key -- and left the traversal, the
            // member paths and `relative`'s prefix on the raw form. A normalization that exists and
            // is used at one of the six sites that need it is worse than none: it reads as solved.
            return std::fs::canonicalize(candidate).unwrap_or_else(|error| {
                panic!("the workspace root {} must resolve: {error}", candidate.display())
            });
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
    member_names_from_manifest(&text)
        .into_iter()
        .flat_map(|name| expand_member_pattern(&root, &name))
        .map(|path| member_root_inside(&root, &path))
        .collect()
}

/// A member path resolved to its canonical form, refused if it leaves the workspace.
///
/// Cargo accepts a member whose directory is a symlink, and `read_dir` on a symlinked ROOT follows
/// it — the per-entry `file_type` refusal below never sees a root, because a root is never an entry
/// (Codex, #578). So a member could point outside the checkout and this guard would traverse
/// unrelated files and quote their source lines in its diagnostics.
///
/// Refused rather than skipped, for the reason the size bound gives: a skip is an exemption
/// anybody can grant themselves, and it would shrink the swept population in silence.
#[allow(dead_code)]
fn member_root_inside(root: &Path, path: &Path) -> PathBuf {
    let canonical = std::fs::canonicalize(path).unwrap_or_else(|error| {
        panic!(
            "HARNESS-BROKE: the member {} does not resolve: {error}; the sweep cannot claim to \
             cover a path it cannot reach",
            path.display()
        )
    });
    assert!(
        canonical.starts_with(root),
        "HARNESS-BROKE: the member {} resolves to {}, which is outside the workspace {}; a guard \
         that walks there is reporting on files that are not in the change set",
        path.display(),
        canonical.display(),
        root.display()
    );
    canonical
}

/// Cargo's glob member form, expanded for the shape Cargo actually resolves.
///
/// `members = ["core/*"]` is valid and this parser returned the literal `core/*`, so the sweep
/// walked a path that does not exist and the set-equality cell compared a wildcard against real
/// directories — the gate rejecting a lawful layout (Codex, #578).
///
/// Only a TRAILING `/*` is expanded, which is the form Cargo documents and the only one seen in
/// practice; a pattern with `*` anywhere else is refused loudly rather than silently mis-resolved,
/// because a wrong population is exactly what this file exists to prevent. Our own manifest uses no
/// glob at all today (measured), so this path is for the manifest somebody writes next.
#[allow(dead_code)]
fn expand_member_pattern(root: &Path, name: &str) -> Vec<PathBuf> {
    let Some(prefix) = name.strip_suffix("/*") else {
        assert!(
            !name.contains('*'),
            "HARNESS-BROKE: the member pattern {name:?} is a glob this parser does not expand; \
             only a trailing `/*` is supported, and guessing at the rest would hand every sweep a \
             population the workspace never declared"
        );
        return vec![root.join(name)];
    };
    let parent = root.join(prefix);
    let Ok(entries) = std::fs::read_dir(&parent) else {
        return Vec::new();
    };
    let mut expanded: Vec<PathBuf> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.join("Cargo.toml").is_file())
        .collect();
    // `read_dir` order is filesystem order, which AGENTS.md forbids assertions from depending on.
    expanded.sort();
    expanded
}

/// The `members` entries of a workspace manifest, as written.
///
/// Split out from the path-building above so the PARSE can be exercised against a manifest a test
/// composes, rather than only against the one on disk. A guard whose whole claim is "a crate is
/// covered the day it enters the members list" has to be able to demonstrate that on a members list
/// containing a crate that does not exist yet, and it cannot do that while the only manifest it can
/// read is the real one.
#[allow(dead_code)]
fn member_names_from_manifest(text: &str) -> Vec<String> {
    // Scans from the `members` KEY to the matching `]`, taking every quoted string on the way --
    // including the ones on the key's own line.
    //
    // The first version set a flag on the `members` line and then `continue`d past it, so the
    // valid Cargo form `members = ["a", "b"]` produced an EMPTY population, and a partially inline
    // array silently dropped its first entries (found by Codex on #578). The empty case fails
    // loudly against every sweep's non-vacuity floor; the PARTIAL case is the dangerous one --
    // twenty-two members of twenty-four still clears the floor, so the population shrinks in
    // silence, which is the defect these guards exist to prevent.
    // The key is ANCHORED to its own line, not found as a substring. `text.find("members")`
    // matches inside `default-members`, so a manifest declaring that key first parsed ITS array
    // instead -- found by L on #578, and measured: `default-members = ["core/protocols"]` above
    // `members = ["a","b","c"]` yielded one entry where three were declared.
    //
    // That was the fourth shape of one defect -- adoption, then directory, then inline array, then
    // substring -- and each fix only moved the question a floor down. The set-equality cell in the
    // sweep is what ends the series, because it does not care HOW the parse goes wrong.
    // And the key must belong to `[workspace]` (Codex, #578). Anchoring to a line stopped
    // `default-members` from matching, but it still searched the WHOLE document with no notion of
    // table boundaries -- so a legal manifest carrying `[workspace.metadata.something]` or
    // `[package.metadata.something]` with its own `members = [...]` above `[workspace]` handed this
    // parse the wrong array, and the gate would then reject a lawful layout. The table is tracked.
    //
    // `starts_with("[workspace]")` is exact by construction: `[workspace.metadata]` begins
    // `[workspace.`, so it cannot pass, while `[workspace] # a comment` can.
    let mut in_workspace_table = false;
    let Some(rest) = text.lines().enumerate().find_map(|(index, line)| {
        let trimmed = line.trim_start();
        if trimmed.starts_with('[') {
            in_workspace_table = trimmed.starts_with("[workspace]");
            return None;
        }
        (in_workspace_table && trimmed.starts_with("members") && line.contains('=')).then(|| {
            let offset: usize = text
                .lines()
                .take(index)
                .map(|earlier| earlier.len() + 1)
                .sum();
            &text[offset.min(text.len())..]
        })
    }) else {
        return Vec::new();
    };
    let Some(open) = rest.find('[') else {
        return Vec::new();
    };
    let body = &rest[open + 1..];
    let end = body.find(']').unwrap_or(body.len());
    body[..end]
        .split('"')
        .skip(1)
        .step_by(2)
        .map(str::to_owned)
        .filter(|name| !name.is_empty())
        .collect()
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

/// The deepest directory nesting a walk will descend.
///
/// Measured rather than guessed: the deepest member root in this workspace nests **3** levels under
/// its own root, so this is roughly ten times the observed shape. Recursion that runs away aborts
/// the process with a stack overflow and no diagnostic, which is the worst failure a guard can
/// have — it says nothing about the tree it died in (Codex, #578).
#[allow(dead_code)]
const WORKSPACE_WALK_MAX_DEPTH: usize = 32;

/// Every file under `start` with one of `extensions`, skipping build and VCS trees.
#[allow(dead_code)]
fn walk(start: &Path, extensions: &[&str], out: &mut Vec<PathBuf>) {
    let mut visited = 0_usize;
    walk_bounded(start, extensions, out, &mut visited, 0);
}

/// Every file under EVERY root, with **one** budget across the whole sweep and each root visited
/// once.
///
/// `walk` resets its counter per call, so a sweep calling it once per member bounded each root and
/// left the AGGREGATE unbounded. And `Cargo.toml` may legally repeat a member path: the membership
/// witness deduplicates with a `BTreeSet` while the walk did not, so the same tree was traversed
/// twice and paid for twice while every individual walk stayed under the bound (Codex, #578).
///
/// Roots are deduplicated by their CANONICAL path where the filesystem answers, so two spellings
/// of one member — `core/protocols` and `./core/protocols`, which Cargo treats as the same member —
/// collapse to one traversal. A root that cannot be canonicalized (it does not exist) falls back to
/// its written form, which is the shape the set-equality cell is there to catch.
#[allow(dead_code)]
fn walk_all(roots: &[PathBuf], extensions: &[&str]) -> Vec<PathBuf> {
    let mut visited = 0_usize;
    let mut out = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    for root in roots {
        // The canonical form is the key AND the path walked. Keying on one form and walking
        // another is how the previous version let a symlinked root through: `read_dir` on a
        // symlinked root follows it, and a root is never a `DirEntry`, so the per-entry refusal
        // below never sees one (Codex, #578).
        let canonical = std::fs::canonicalize(root).unwrap_or_else(|_| root.clone());
        if !seen.insert(canonical.clone()) {
            continue;
        }
        walk_bounded(&canonical, extensions, &mut out, &mut visited, 0);
    }
    out
}

/// The bounded walk. Both bounds count what the WALK does, not what it keeps.
///
/// The first version asserted on `out.len()` inside the extension-filtered arm, so it bounded the
/// RESULT while `read_dir` and the descent still visited every other entry: a directory of a
/// million non-matching files, or an arbitrarily deep tree, passed a cap that never fired. A cap
/// that does not cap is the exact defect this file exists to make impossible, and it was in the
/// file (Codex, #578).
///
/// Measured before choosing the numbers, because a bound tighter than the subject turns every
/// sweep red on a legitimate checkout: the heaviest member root visits **113** entries and the
/// whole workspace visits **646**, against a bound of 8 192.
#[allow(dead_code)]
fn walk_bounded(
    start: &Path,
    extensions: &[&str],
    out: &mut Vec<PathBuf>,
    visited: &mut usize,
    depth: usize,
) {
    assert!(
        depth <= WORKSPACE_WALK_MAX_DEPTH,
        "HARNESS-BROKE: the walk descended past {WORKSPACE_WALK_MAX_DEPTH} levels at {}; it is \
         not measuring what it claims",
        start.display()
    );
    let Ok(entries) = std::fs::read_dir(start) else {
        return;
    };
    for entry in entries.flatten() {
        // Counted for EVERY entry the walk looks at, matching or not, directory or file. This is
        // the difference between bounding the traversal and bounding the answer.
        *visited += 1;
        assert!(
            *visited <= WORKSPACE_WALK_MAX_ENTRIES,
            "HARNESS-BROKE: the walk visited more than {WORKSPACE_WALK_MAX_ENTRIES} entries, \
             reaching {}; it is not measuring what it claims",
            entry.path().display()
        );
        // The type is read from the DIRECTORY ENTRY, which does not follow links, rather than from
        // `Path::is_dir`, which does. A repository-controlled symlink pointing at an ancestor made
        // the walk re-enter the tree, and one pointing outside made it sweep files that are not in
        // the change set at all (found by Codex on #578). The per-crate guards already refuse
        // symlinks -- `hardened_walker_rejects_symlinks_and_unexpected_entries` -- so this walk
        // was the one place in the family that did not.
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if kind.is_symlink() {
            continue;
        }
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if kind.is_dir() {
            if WORKSPACE_WALK_SKIPPED.contains(&name.as_ref()) {
                continue;
            }
            walk_bounded(&path, extensions, out, visited, depth + 1);
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
