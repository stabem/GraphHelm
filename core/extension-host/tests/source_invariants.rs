//! Enforces a property of this crate's SOURCE that no runtime test can see.
//!
//! A documented control which no test enforces is not a control. Operator-facing strings are such
//! a control: nothing renders them except a human, so a defect in them leaves every test green.
//!
//! The defect: a run of spaces inside a string literal, which a reader gets verbatim. It arrives
//! when a `\` continuation is lost BEFORE the file is written — a code generator consuming the
//! escape, which is MEASURED (#440); any other upstream rewriter is conjecture — so what lands is
//! one line with the indentation already inside it.
//!
//! **Neither `rustc` nor `cargo fmt` can produce it, and the sentence this replaces named both**:
//! the continuation escape consumes the next line's indentation, and `format_strings` is absent
//! from this repository's `rustfmt.toml` (default `false`), so rustfmt never edits inside a
//! literal. Both measured in #440. If you write Rust through a script this is your defect, and the
//! explanation that stood here pointed away from it. The source reads plausibly
//! while the reader of a failure gets `matching one empty              digest against another`.
//!
//! **The walk covers `tests/` as well as `src/`.** A repository-wide census at the threshold that
//! actually enforces found every `src/` tree in the workspace already clean and every instance of
//! this class living in `tests/`. A guard scoped to `src/` would have been green on arrival and
//! protected nothing.
//!
//! Scanning `tests/` means this file scans itself, which is deliberate: exempting its own file
//! would leave a whole file unguarded and the exemption would be invisible from the failure
//! message. The cost is that every string here obeys the rule it enforces, and the `\`
//! continuation idiom below is what makes that possible.

include!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../tools/source-invariants/detect.rs"
));

use std::path::{Path, PathBuf};

const WALK_LIMITS: WalkLimits = WalkLimits {
    max_depth: 32,
    max_entries: 4_096,
    max_rust_files: 1_024,
};

#[derive(Clone, Copy)]
struct WalkLimits {
    max_depth: usize,
    max_entries: usize,
    max_rust_files: usize,
}

#[derive(Clone, Copy, Debug)]
struct MetadataSnapshot {
    is_symlink: bool,
    is_dir: bool,
    is_file: bool,
}

trait SourceFs {
    fn symlink_metadata(&mut self, path: &Path) -> Result<MetadataSnapshot, String>;
    fn read_dir(
        &mut self,
        path: &Path,
        limit: usize,
    ) -> Result<Vec<Result<PathBuf, String>>, String>;
    fn read_to_string(&mut self, path: &Path) -> Result<String, String>;
}

type MetadataFn = fn(&Path) -> std::io::Result<std::fs::Metadata>;

fn no_follow_metadata(path: &Path) -> std::io::Result<std::fs::Metadata> {
    std::fs::symlink_metadata(path)
}

struct RealFs {
    metadata_fn: MetadataFn,
}

impl Default for RealFs {
    fn default() -> Self {
        Self {
            metadata_fn: no_follow_metadata,
        }
    }
}

impl SourceFs for RealFs {
    fn symlink_metadata(&mut self, path: &Path) -> Result<MetadataSnapshot, String> {
        let metadata = (self.metadata_fn)(path)
            .map_err(|e| format!("cannot inspect {}: {e}", path.display()))?;
        let file_type = metadata.file_type();
        Ok(MetadataSnapshot {
            is_symlink: file_type.is_symlink(),
            is_dir: file_type.is_dir(),
            is_file: file_type.is_file(),
        })
    }

    fn read_dir(
        &mut self,
        path: &Path,
        limit: usize,
    ) -> Result<Vec<Result<PathBuf, String>>, String> {
        let entries =
            std::fs::read_dir(path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
        let mut paths = Vec::new();
        for entry in entries {
            if paths.len() >= limit {
                return Err("HARNESS-BROKE: authored Rust walk exceeded entry limit".to_owned());
            }
            paths.push(
                entry
                    .map(|entry| entry.path())
                    .map_err(|e| format!("cannot read an entry in {}: {e}", path.display())),
            );
        }
        Ok(paths)
    }

    fn read_to_string(&mut self, path: &Path) -> Result<String, String> {
        std::fs::read_to_string(path).map_err(|e| format!("cannot read {}: {e}", path.display()))
    }
}

/// Every `.rs` file under `src/` AND `tests/`, discovered by WALKING the directories.
///
/// The population is the directory, not a list: a file ADDED to the crate must be scanned without
/// anyone remembering to name it. The risk that trades against, a walk that silently returns
/// almost nothing, is answered by the floor and the two root assertions below.
fn sources() -> Vec<(String, String)> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    sources_with_fs(&mut RealFs::default(), root, WALK_LIMITS).unwrap_or_else(|e| panic!("{e}"))
}

fn sources_with_fs<F: SourceFs>(
    fs: &mut F,
    root: &Path,
    limits: WalkLimits,
) -> Result<Vec<(String, String)>, String> {
    fn walk<F: SourceFs>(
        fs: &mut F,
        dir: &Path,
        depth: usize,
        entries_seen: &mut usize,
        found: &mut Vec<PathBuf>,
        limits: WalkLimits,
    ) -> Result<(), String> {
        let metadata = fs.symlink_metadata(dir)?;
        if metadata.is_symlink {
            return Err(format!(
                "HARNESS-BROKE: symlink is not allowed: {}",
                dir.display()
            ));
        }
        if !metadata.is_dir {
            return Err(format!(
                "HARNESS-BROKE: authored Rust root is not a directory: {}",
                dir.display()
            ));
        }
        if depth > limits.max_depth {
            return Err(format!(
                "HARNESS-BROKE: authored Rust walk exceeded depth {} at {}",
                limits.max_depth,
                dir.display()
            ));
        }

        let remaining = limits.max_entries.saturating_sub(*entries_seen);
        let entries = fs.read_dir(dir, remaining)?;
        *entries_seen += entries.len();
        let mut paths = entries.into_iter().collect::<Result<Vec<_>, _>>()?;
        paths.sort();
        for path in paths {
            let metadata = fs.symlink_metadata(&path)?;
            if metadata.is_symlink {
                return Err(format!(
                    "HARNESS-BROKE: symlink is not allowed: {}",
                    path.display()
                ));
            }
            if metadata.is_dir {
                walk(fs, &path, depth + 1, entries_seen, found, limits)?;
            } else if metadata.is_file && path.extension().is_some_and(|ext| ext == "rs") {
                if found.len() >= limits.max_rust_files {
                    return Err(format!(
                        "HARNESS-BROKE: authored Rust walk exceeded Rust file limit {}",
                        limits.max_rust_files
                    ));
                }
                found.push(path);
            } else if !metadata.is_file {
                return Err(format!(
                    "HARNESS-BROKE: unexpected filesystem entry: {}",
                    path.display()
                ));
            }
        }
        Ok(())
    }

    let mut found = Vec::new();
    let mut entries_seen = 0;
    for fixed_root in [root.join("src"), root.join("tests")] {
        walk(fs, &fixed_root, 0, &mut entries_seen, &mut found, limits)?;
    }
    found.sort();
    found
        .into_iter()
        .map(|path| {
            let text = fs.read_to_string(&path)?;
            let shown = path
                .strip_prefix(root)
                .unwrap_or(&path)
                .display()
                .to_string()
                .replace('\\', "/");
            Ok((shown, text))
        })
        .collect()
}

/// This crate's exemption, composed on top of the shared detection.
///
/// The detection lives in `tools/source-invariants/detect.rs` and is included by value; only the
/// EXEMPTION is a property of a crate, which is why the two are separate functions there.
///
/// This crate needs only the shared one. Every candidate the census turned up here is authored
/// prose in an assertion or `expect` message; none of the DATA species that force a role exemption
/// elsewhere occurs in this crate. That is a measurement rather than an expectation, and if one
/// ever lands here the exemption belongs beside it, by ROLE and never per-file.
fn offends(line: &str) -> bool {
    !is_line_comment(line) && has_run_in_literal(line)
}

#[test]
fn authored_strings_carry_no_collapsed_indentation() {
    let offenders: Vec<String> = sources()
        .iter()
        .flat_map(|(path, text)| {
            text.lines()
                .enumerate()
                .filter(|(_, line)| offends(line))
                .map(move |(number, line)| format!("{path}:{}: {}", number + 1, line.trim_start()))
        })
        .collect();

    assert!(
        offenders.is_empty(),
        "these string literals carry runs of whitespace, which a reader of the failure gets verbatim. A continued literal keeps the next line indentation inside it: end the line with a backslash so Rust drops the newline and the indentation, or put the string on one line.\n{}",
        offenders.join("\n")
    );
}

/// The walk must actually reach the crate, and it has TWO roots to reach.
///
/// The floor is the REAL count rather than a number chosen to sit comfortably under it: a floor
/// with slack tolerates exactly the silent shrinkage it exists to catch. Its cost is that a
/// legitimate removal now edits this number, which is the plausible-looking edit a floor is
/// supposed to resist, so the rule beside it: **lower this only in the same commit as the removal
/// that caused it, and name the removed file.**
///
/// **The roots are asserted by PREFIX, and in THIS crate that is not a stylistic preference.**
/// Naming a file to stand for a root only distinguishes the roots while that name is unique across
/// them, which is an accident of a crate rather than a property of a guard. Here the accident does
/// not hold at all: `activation.rs`, `discovery.rs` and `staging.rs` each exist under **both**
/// roots, so every available landmark name is satisfied by a walk that reached only `src/`. A
/// named form would have arrived already unable to make the distinction it claims to make, and it
/// would have looked correct. `tests/` is where every instance of this class lives, so that
/// failure would leave the scan above silently green.
#[test]
fn the_scan_covers_both_roots_of_the_crate() {
    let found = sources();
    // ORDER IS LOAD-BEARING HERE, and it is the reverse of the obvious one -- so this note sits
    // at the site that ARMS it, because moving the cheap numeric check back to the top is exactly
    // the kind of tidying that looks like an improvement.
    //
    // The floor is this crate's EXACT file count, so losing either root drops the count below it.
    // With the floor first, it answers every lost-root case and these two assertions never run:
    // the file would credit a check that cannot fire, and nobody would learn whether it worked.
    // (Found by N, reviewing #391 and #392, after my own sabotage cells had to LOWER the floor
    // before a root assertion could be heard -- which I had read as staging rather than as the
    // symptom it was.)
    //
    // Roots first, so the specific diagnosis wins: a lost root says which root, instead of a
    // count the reader has to work backwards from. A walk that reached both roots but shrank
    // still fails the floor below, exactly as before.
    assert!(
        found.iter().any(|(path, _)| path.starts_with("src")),
        "HARNESS-BROKE: the walk reached no file under src/ at all"
    );
    assert!(
        found.iter().any(|(path, _)| path.starts_with("tests")),
        "HARNESS-BROKE: the walk reached no file under tests/, so it is covering only one of its \
         two roots -- and tests/ is where every instance of this class lives, so the scan above \
         would be silently green"
    );
    assert!(
        found.len() >= 8,
        "HARNESS-BROKE: the walk found only {} source files, so the scan above reads far less \
         than this crate",
        found.len()
    );
}

#[derive(Clone, Copy)]
enum FakeKind {
    Directory,
    File,
    Symlink,
    Other,
}

#[derive(Default)]
struct FakeFs {
    nodes: std::collections::BTreeMap<PathBuf, FakeKind>,
    read_dir_failure: Option<PathBuf>,
    entry_failure: Option<PathBuf>,
    metadata_failure: Option<PathBuf>,
    reads: Vec<PathBuf>,
}

impl FakeFs {
    fn seeded() -> Self {
        let mut fs = Self::default();
        fs.nodes.insert("crate/src".into(), FakeKind::Directory);
        fs.nodes.insert("crate/tests".into(), FakeKind::Directory);
        fs.nodes.insert("crate/src/lib.rs".into(), FakeKind::File);
        fs.nodes
            .insert("crate/tests/guard.rs".into(), FakeKind::File);
        fs
    }
}

impl SourceFs for FakeFs {
    fn symlink_metadata(&mut self, path: &Path) -> Result<MetadataSnapshot, String> {
        if self.metadata_failure.as_deref() == Some(path) {
            return Err("metadata sentinel".to_owned());
        }
        let kind = self
            .nodes
            .get(path)
            .ok_or_else(|| format!("missing fake path {}", path.display()))?;
        Ok(MetadataSnapshot {
            is_symlink: matches!(kind, FakeKind::Symlink),
            is_dir: matches!(kind, FakeKind::Directory),
            is_file: matches!(kind, FakeKind::File),
        })
    }

    fn read_dir(
        &mut self,
        path: &Path,
        limit: usize,
    ) -> Result<Vec<Result<PathBuf, String>>, String> {
        if self.read_dir_failure.as_deref() == Some(path) {
            return Err("read-dir sentinel".to_owned());
        }
        let mut children = self
            .nodes
            .keys()
            .filter(|candidate| candidate.parent() == Some(path))
            .cloned()
            .collect::<Vec<_>>();
        children.reverse();
        if children.len() > limit {
            return Err("entry limit sentinel".to_owned());
        }
        let mut entries = children.into_iter().map(Ok).collect::<Vec<_>>();
        if self.entry_failure.as_deref() == Some(path) {
            entries.push(Err("entry sentinel".to_owned()));
        }
        Ok(entries)
    }

    fn read_to_string(&mut self, path: &Path) -> Result<String, String> {
        self.reads.push(path.to_path_buf());
        Ok(format!("// {}", path.display()))
    }
}

fn limits(depth: usize, entries: usize, rust_files: usize) -> WalkLimits {
    WalkLimits {
        max_depth: depth,
        max_entries: entries,
        max_rust_files: rust_files,
    }
}

#[test]
fn hardened_walker_sorts_and_reaches_both_fixed_roots() {
    let mut fs = FakeFs::seeded();
    let found = sources_with_fs(&mut fs, Path::new("crate"), limits(4, 8, 4)).unwrap();
    assert_eq!(
        found
            .iter()
            .map(|(path, _)| path.as_str())
            .collect::<Vec<_>>(),
        ["src/lib.rs", "tests/guard.rs"]
    );
}

#[test]
fn hardened_walker_refuses_a_missing_fixed_root() {
    let mut fs = FakeFs::seeded();
    fs.nodes.remove(Path::new("crate/tests"));
    let error = sources_with_fs(&mut fs, Path::new("crate"), limits(4, 8, 4)).unwrap_err();
    assert!(error.contains("missing fake path"), "{error}");
    assert!(error.contains("tests"), "{error}");
    assert!(fs.reads.is_empty());
}

#[test]
fn hardened_walker_rejects_symlinks_and_unexpected_entries() {
    let mut root_link = FakeFs::seeded();
    root_link
        .nodes
        .insert("crate/src".into(), FakeKind::Symlink);
    let error = sources_with_fs(&mut root_link, Path::new("crate"), limits(4, 8, 4)).unwrap_err();
    assert!(error.contains("symlink"), "{error}");
    assert!(root_link.reads.is_empty());

    let mut child_link = FakeFs::seeded();
    child_link
        .nodes
        .insert("crate/src/escape.rs".into(), FakeKind::Symlink);
    let error = sources_with_fs(&mut child_link, Path::new("crate"), limits(4, 8, 4)).unwrap_err();
    assert!(error.contains("symlink"), "{error}");
    assert!(child_link.reads.is_empty());

    let mut special = FakeFs::seeded();
    special
        .nodes
        .insert("crate/src/device".into(), FakeKind::Other);
    let error = sources_with_fs(&mut special, Path::new("crate"), limits(4, 8, 4)).unwrap_err();
    assert!(error.contains("unexpected"), "{error}");
    assert!(special.reads.is_empty());
}

#[test]
fn hardened_walker_enforces_all_limits_before_reading_content() {
    let mut depth = FakeFs::seeded();
    depth
        .nodes
        .insert("crate/src/nested".into(), FakeKind::Directory);
    let error = sources_with_fs(&mut depth, Path::new("crate"), limits(0, 8, 4)).unwrap_err();
    assert!(error.contains("depth"), "{error}");
    assert!(depth.reads.is_empty());

    let mut entries = FakeFs::seeded();
    let error = sources_with_fs(&mut entries, Path::new("crate"), limits(4, 1, 4)).unwrap_err();
    assert!(error.contains("entry limit"), "{error}");
    assert!(entries.reads.is_empty());

    let mut rust_files = FakeFs::seeded();
    let error = sources_with_fs(&mut rust_files, Path::new("crate"), limits(4, 8, 1)).unwrap_err();
    assert!(error.contains("Rust file"), "{error}");
    assert!(rust_files.reads.is_empty());
}

#[test]
fn hardened_walker_propagates_directory_entry_and_metadata_errors() {
    let src = PathBuf::from("crate/src");
    let lib = PathBuf::from("crate/src/lib.rs");

    let mut read_dir = FakeFs::seeded();
    read_dir.read_dir_failure = Some(src.clone());
    let error = sources_with_fs(&mut read_dir, Path::new("crate"), limits(4, 8, 4)).unwrap_err();
    assert_eq!(error, "read-dir sentinel");

    let mut entry = FakeFs::seeded();
    entry.entry_failure = Some(src);
    let emitted = entry.read_dir(Path::new("crate/src"), 8).unwrap();
    assert!(emitted.first().is_some_and(Result::is_ok));
    assert!(emitted.get(1).is_some_and(Result::is_err));
    let error = sources_with_fs(&mut entry, Path::new("crate"), limits(4, 8, 4)).unwrap_err();
    assert_eq!(error, "entry sentinel");

    let mut metadata = FakeFs::seeded();
    metadata.metadata_failure = Some(lib);
    let error = sources_with_fs(&mut metadata, Path::new("crate"), limits(4, 8, 4)).unwrap_err();
    assert_eq!(error, "metadata sentinel");
}

#[test]
fn real_adapter_is_bound_to_no_follow_metadata() {
    fn sentinel(_: &Path) -> std::io::Result<std::fs::Metadata> {
        Err(std::io::Error::other("metadata seam sentinel"))
    }

    let real = RealFs::default();
    assert!(std::ptr::fn_addr_eq(
        real.metadata_fn,
        no_follow_metadata as MetadataFn
    ));

    let own_source = sources()
        .into_iter()
        .find(|(_, text)| text.contains("fn real_adapter_is_bound_to_no_follow_metadata"))
        .map(|(_, text)| text)
        .expect("the authored-source walk must include this test file");
    let indent = " ".repeat(4);
    let binding = format!(
        "fn no_follow_metadata(path: &Path) -> std::io::Result<std::fs::Metadata> {{\n\
         {indent}std::fs::symlink_metadata(path)\n\
         }}"
    );
    assert!(own_source.contains(&binding), "{binding}");

    let mut injected = RealFs {
        metadata_fn: sentinel,
    };
    let error = injected.symlink_metadata(Path::new("unused")).unwrap_err();
    assert!(error.contains("metadata seam sentinel"), "{error}");
}
