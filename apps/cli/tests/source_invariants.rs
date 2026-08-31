//! Enforces a property of this crate's SOURCE that no runtime test can see.
//!
//! A documented control which no test enforces is not a control. Operator-facing strings are such
//! a control: nothing renders them except a human, so a defect in them leaves every test green --
//! confirmed when eight of them were corrected here and not one test noticed.
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
//! while the operator reads `a waiter waits on its OWN                  lease`.

include!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../tools/source-invariants/detect.rs"
));

use std::path::{Path, PathBuf};

const MAX_DEPTH: usize = 32;
const MAX_ENTRIES: usize = 4_096;
const MAX_RUST_FILES: usize = 1_024;
const MAX_FILE_BYTES: u64 = 1024 * 1024;

#[derive(Clone, Copy, Debug)]
struct MetadataSnapshot {
    is_symlink: bool,
    is_dir: bool,
    is_file: bool,
    len: u64,
}

trait AuthoredFs {
    fn symlink_metadata(&mut self, path: &Path) -> Result<MetadataSnapshot, String>;
    fn read_dir(&mut self, path: &Path, limit: usize) -> Result<Vec<PathBuf>, String>;
    fn read_to_string(&mut self, path: &Path, limit: usize) -> Result<String, String>;
}

struct RealFs;

impl AuthoredFs for RealFs {
    fn symlink_metadata(&mut self, path: &Path) -> Result<MetadataSnapshot, String> {
        let metadata = std::fs::symlink_metadata(path)
            .map_err(|e| format!("cannot inspect {}: {e}", path.display()))?;
        let kind = metadata.file_type();
        Ok(MetadataSnapshot {
            is_symlink: kind.is_symlink(),
            is_dir: metadata.is_dir(),
            is_file: metadata.is_file(),
            len: metadata.len(),
        })
    }

    fn read_dir(&mut self, path: &Path, limit: usize) -> Result<Vec<PathBuf>, String> {
        let entries =
            std::fs::read_dir(path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
        let mut paths = Vec::new();
        for entry in entries {
            if paths.len() >= limit {
                return Err(format!(
                    "HARNESS-BROKE: authored Rust walk exceeded {MAX_ENTRIES} entries"
                ));
            }
            paths.push(
                entry
                    .map_err(|e| format!("cannot read an entry in {}: {e}", path.display()))?
                    .path(),
            );
        }
        Ok(paths)
    }

    fn read_to_string(&mut self, path: &Path, limit: usize) -> Result<String, String> {
        use std::io::Read;

        let file = std::fs::File::open(path)
            .map_err(|e| format!("cannot open {}: {e}", path.display()))?;
        let mut bytes = Vec::new();
        file.take(limit as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
        if bytes.len() > limit {
            return Err(format!(
                "HARNESS-BROKE: {} changed while scanning and exceeds file-size limit of \
                 {MAX_FILE_BYTES} bytes",
                path.display()
            ));
        }
        String::from_utf8(bytes)
            .map_err(|e| format!("cannot decode {} as UTF-8: {e}", path.display()))
    }
}

fn require_authored_rust_directory(path: &Path, metadata: MetadataSnapshot) -> Result<(), String> {
    if metadata.is_symlink {
        return Err(format!(
            "HARNESS-BROKE: symlinks are not allowed in authored Rust roots: {}",
            path.display()
        ));
    }
    if !metadata.is_dir {
        return Err(format!(
            "HARNESS-BROKE: authored Rust root is not a directory: {}",
            path.display()
        ));
    }
    Ok(())
}

/// Every authored `.rs` file under `src/` and `tests/`, discovered by WALKING the directories.
///
/// **The population is the directory, not a list.** An earlier version named three files with
/// `include_str!`, which bought one property -- a moved path breaks the BUILD instead of silently
/// shrinking the scan -- and quietly gave up a bigger one: a file ADDED to the crate was not
/// scanned, and nothing said so. Guarding against a moved file while blind to a new file is the
/// weaker half of the trade. (Found by L.)
///
/// The shrinkage risk that `include_str!` covered is handled by the floor in
/// `the_scan_covers_the_whole_crate`: a walk that returns almost nothing fails loudly.
fn sources() -> Vec<(String, String)> {
    let crate_root = Path::new(env!("CARGO_MANIFEST_DIR"));
    authored_sources_with_fs(&mut RealFs, crate_root).unwrap_or_else(|e| panic!("{e}"))
}

fn authored_sources_with_fs<F: AuthoredFs>(
    fs: &mut F,
    crate_root: &Path,
) -> Result<Vec<(String, String)>, String> {
    fn walk<F: AuthoredFs>(
        fs: &mut F,
        dir: &Path,
        depth: usize,
        entries_seen: &mut usize,
        out: &mut Vec<PathBuf>,
    ) -> Result<(), String> {
        let metadata = fs.symlink_metadata(dir)?;
        require_authored_rust_directory(dir, metadata)?;
        if depth > MAX_DEPTH {
            return Err(format!(
                "HARNESS-BROKE: authored Rust walk exceeded depth {MAX_DEPTH} at {}",
                dir.display()
            ));
        }
        let remaining = MAX_ENTRIES.saturating_sub(*entries_seen);
        let mut paths = fs.read_dir(dir, remaining)?;
        *entries_seen += paths.len();
        paths.sort();
        for path in paths {
            let metadata = fs.symlink_metadata(&path)?;
            if metadata.is_symlink {
                return Err(format!(
                    "HARNESS-BROKE: symlinks are not allowed in authored Rust roots: {}",
                    path.display()
                ));
            }
            if metadata.is_dir {
                walk(fs, &path, depth + 1, entries_seen, out)?;
            } else if metadata.is_file && path.extension().is_some_and(|ext| ext == "rs") {
                if metadata.len > MAX_FILE_BYTES {
                    return Err(format!(
                        "HARNESS-BROKE: {} is {} bytes and exceeds file-size limit of \
                         {MAX_FILE_BYTES} bytes",
                        path.display(),
                        metadata.len
                    ));
                }
                if out.len() >= MAX_RUST_FILES {
                    return Err(format!(
                        "HARNESS-BROKE: authored Rust walk exceeded {MAX_RUST_FILES} Rust files"
                    ));
                }
                out.push(path);
            } else if !metadata.is_file {
                return Err(format!(
                    "HARNESS-BROKE: unexpected filesystem entry in authored Rust roots: {}",
                    path.display()
                ));
            }
        }
        Ok(())
    }

    let mut found = Vec::new();
    let mut entries_seen = 0;
    for root in [crate_root.join("src"), crate_root.join("tests")] {
        walk(fs, &root, 0, &mut entries_seen, &mut found)?;
    }
    found.sort();
    found
        .into_iter()
        .map(|path| {
            let text = fs.read_to_string(&path, MAX_FILE_BYTES as usize)?;
            let shown = path
                .strip_prefix(crate_root)
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
/// The detection lives in `tools/source-invariants/detect.rs` and is included by value; only
/// the EXEMPTION is a property of this crate, which is why the two are separate functions
/// there. Composing them here rather than importing a bundled predicate is what lets the
/// exemption be asserted to be doing work, instead of merely having a subject.
///
/// Adopting the shared file also replaces this crate's own copy, which split on every `"`
/// and therefore mis-read escapes: `\"` shifted the parity of everything after it, and the
/// obvious repair for that broke the mirror case. Those defects were fixed once, in one
/// place, and this crate inherits the fix rather than needing it applied a second time --
/// which is the entire argument for the shared file, and is exactly what the twin pointer
/// deleted here predicted would go wrong.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Exemption {
    Comment,
    CanonicalJsonFixture,
    DetectorFixture,
}

fn canonical_json_fixture_source_line() -> String {
    let two = " ".repeat(2);
    let four = " ".repeat(4);
    format!(
        "\"{{\\n{two}\\\"a\\\": {{\\n{four}\\\"x\\\": 3,\\n{four}\\\"y\\\": 2\\n{two}}},\\n{two}\\\"b\\\": 1\\n}}\","
    )
}

fn detector_fixture_source_lines() -> Vec<String> {
    let three = " ".repeat(3);
    let four = " ".repeat(4);
    let eight = " ".repeat(8);
    let ten = " ".repeat(10);
    vec![
        format!("let aligned = r#\"let s = \"ok\"; //{three}aligned trailing comment\"#;"),
        format!("aligned.contains(\"{three}\"),"),
        format!("let between = r#\"let a = \"x\";{eight}let b = \"y\";\"#;"),
        format!("between.contains(\"{three}\"),"),
        format!("let commented = r#\"//{three}let s = \"a{ten}b\";\"#;"),
        format!("r#\"{four}\"a real defect with{ten}collapsed indent\",\"#"),
    ]
}

fn exemption(path: &str, line: &str) -> Option<Exemption> {
    if is_line_comment(line) {
        return Some(Exemption::Comment);
    }
    let trimmed = line.trim_start();
    if path == "tests/schema_cli.rs" && trimmed == canonical_json_fixture_source_line() {
        return Some(Exemption::CanonicalJsonFixture);
    }
    if path == "tests/source_invariants.rs"
        && detector_fixture_source_lines()
            .iter()
            .any(|fixture| fixture == trimmed)
    {
        return Some(Exemption::DetectorFixture);
    }
    None
}

fn offends(path: &str, line: &str) -> bool {
    has_run_in_literal(line) && exemption(path, line).is_none()
}

#[test]
fn operator_strings_carry_no_collapsed_indentation() {
    let offenders: Vec<String> = sources()
        .iter()
        .flat_map(|(path, text)| {
            text.lines()
                .enumerate()
                .filter(|(_, line)| offends(path, line))
                .map(move |(number, line)| format!("{path}:{}: {}", number + 1, line.trim_start()))
        })
        .collect();

    assert!(
        offenders.is_empty(),
        "these string literals carry runs of whitespace, which the operator reads verbatim. A continued literal keeps the next line indentation inside it: put the string on one line, or concatenate explicitly.\n{}",
        offenders.join("\n")
    );
}

/// #161's declared gap, pinned at the place it would be violated: no production surface in this
/// crate may append a `Countersign` clearance while nothing can verify one.
///
/// MEASURED rather than assumed. `ClearanceVerifier::Countersign` carries `identity` and
/// `key_fingerprint` and **no signature**. Its own doc says the cryptographic verification
/// "happens at append time" -- but nothing in the workspace appends a clearance at any layer
/// (`CompletionCleared` appears only in the fold and the protocol declarations), and there is
/// nothing on the event to verify even if it did. The fold compares identity and fingerprint
/// against the registry, and BOTH are public journal data, so anyone able to append can name any
/// registered identity.
///
/// `MachineReplay` is deliberately NOT covered: since #161 the fold re-derives its evidence
/// digest, so producing one is safe. Covering both would refuse a correct future change.
///
/// WHAT THIS WATCHES, AND WHAT IT DOES NOT (D's review of #527). This walks `apps/cli/src` only —
/// one crate of several that append events. Measured on `origin/main`: of 90 append sites across
/// production `src/`, 20 are here; `core/events` (34), `core/runtime` (15) and `core/governor` (3)
/// are outside this walk, and `core/runtime`/`core/governor` are where an AUTOMATIC clearance would
/// most plausibly land. (D counted 22 of 51 under a narrower predicate; the totals differ, the
/// conclusion does not.)
///
/// The obvious remedy is the WRONG one: copying this guard into six crates would duplicate an
/// ORACLE, and six copies drift into six meanings. The right shape is the workspace-wide walk
/// already on main from #530, and adopting it is follow-up rather than something to improvise here.
/// Until then the name says `apps_cli` so the scope is in the assertion's own title, not only in
/// this paragraph.
///
/// This is a TRAP, not a prohibition. The day a surface appends a countersignature this goes red,
/// and whoever adds it must land the verification -- or the `signature_unverifiable` refusal the
/// issue names -- in the SAME change, instead of discovering the gap afterwards.
#[test]
fn no_apps_cli_surface_appends_an_unverifiable_countersignature() {
    let scanned = sources();
    let production: Vec<_> = scanned
        .iter()
        .filter(|(path, _)| path.starts_with("src/"))
        .collect();

    // Presence control for an absence guard: an empty population would make the assertion below
    // pass while measuring nothing at all.
    assert!(
        !production.is_empty(),
        "precondition: the walk must have returned production sources, or the absence asserted below is the absence of a SCAN, not of a countersignature"
    );

    let offenders: Vec<String> = production
        .iter()
        .flat_map(|(path, text)| {
            text.lines()
                .enumerate()
                .filter(|(_, line)| {
                    // Comments are excluded on purpose. A guard that reddens when someone
                    // DOCUMENTS the gap would teach the next reader to delete the sentence
                    // rather than fix the code, and the message it prints would be wrong.
                    // The exemption is UNQUALIFIED: it exempts by line SHAPE, never by what the
                    // comment says, so commented-out construction is exempt too (D). Accepted —
                    // commented-out code appends nothing.
                    let trimmed = line.trim_start();
                    !trimmed.starts_with("//")
                        && line.contains("ClearanceVerifier::Countersign")
                })
                .map(move |(number, line)| format!("{path}:{}: {}", number + 1, line.trim()))
        })
        .collect();

    assert!(
        offenders.is_empty(),
        "a production surface builds a Countersign clearance, but nothing can verify one: the event carries no signature, and the fold checks only journal-public identity and fingerprint. Land the verification, or refuse the append with signature_unverifiable (#161), in the SAME change.
{}",
        offenders.join("
")
    );
}

/// Complete in-memory model of the filesystem subset the walker consumes.
///
/// Directory children come from the node map. Metadata preserves both a link's identity and its
/// target's file/directory shape, making it possible to prove that the walker checks the identity
/// first. Opening a link follows its target, like the operating system would, and every inspect or
/// open is recorded before the operation so the tests can prove a forbidden open never happened.
#[derive(Clone, Debug)]
enum FakeNode {
    Directory,
    File(Vec<u8>),
    Symlink(PathBuf),
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum Operation {
    Inspect(PathBuf),
    ReadDirectory(PathBuf),
    ReadFile(PathBuf),
}

#[derive(Default)]
struct FakeFs {
    nodes: std::collections::BTreeMap<PathBuf, FakeNode>,
    operations: Vec<Operation>,
}

impl FakeFs {
    fn insert(&mut self, path: impl Into<PathBuf>, node: FakeNode) {
        self.nodes.insert(path.into(), node);
    }

    fn resolved_node(&self, path: &Path) -> Result<&FakeNode, String> {
        let node = self
            .nodes
            .get(path)
            .ok_or_else(|| format!("fake path is absent: {}", path.display()))?;
        match node {
            FakeNode::Symlink(target) => self
                .nodes
                .get(target)
                .ok_or_else(|| format!("fake link target is absent: {}", target.display())),
            other => Ok(other),
        }
    }
}

impl AuthoredFs for FakeFs {
    fn symlink_metadata(&mut self, path: &Path) -> Result<MetadataSnapshot, String> {
        self.operations.push(Operation::Inspect(path.to_path_buf()));
        let direct = self
            .nodes
            .get(path)
            .ok_or_else(|| format!("fake path is absent: {}", path.display()))?;
        let target = self.resolved_node(path)?;
        Ok(MetadataSnapshot {
            is_symlink: matches!(direct, FakeNode::Symlink(_)),
            is_dir: matches!(target, FakeNode::Directory),
            is_file: matches!(target, FakeNode::File(_)),
            len: match target {
                FakeNode::File(bytes) => bytes.len() as u64,
                FakeNode::Directory | FakeNode::Symlink(_) => 0,
            },
        })
    }

    fn read_dir(&mut self, path: &Path, limit: usize) -> Result<Vec<PathBuf>, String> {
        self.operations
            .push(Operation::ReadDirectory(path.to_path_buf()));
        if !matches!(self.resolved_node(path)?, FakeNode::Directory) {
            return Err(format!("fake path is not a directory: {}", path.display()));
        }
        let resolved = match self.nodes.get(path) {
            Some(FakeNode::Symlink(target)) => target,
            Some(_) => path,
            None => return Err(format!("fake path is absent: {}", path.display())),
        };
        let mut children = self
            .nodes
            .keys()
            .filter(|candidate| candidate.parent() == Some(resolved))
            .cloned()
            .collect::<Vec<_>>();
        children.sort();
        if children.len() > limit {
            return Err(format!(
                "fake directory exceeds entry limit: {}",
                path.display()
            ));
        }
        Ok(children)
    }

    fn read_to_string(&mut self, path: &Path, limit: usize) -> Result<String, String> {
        self.operations
            .push(Operation::ReadFile(path.to_path_buf()));
        match self.resolved_node(path)? {
            FakeNode::File(bytes) if bytes.len() <= limit => String::from_utf8(bytes.clone())
                .map_err(|e| format!("fake file is not UTF-8: {}: {e}", path.display())),
            FakeNode::File(_) => Err(format!("fake file exceeds read limit: {}", path.display())),
            _ => Err(format!("fake path is not a file: {}", path.display())),
        }
    }
}

#[test]
fn real_reader_rechecks_the_limit_after_metadata_before_allocating_more() {
    use std::io::Write;

    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("boundary.rs");
    std::fs::write(&path, vec![b'x'; MAX_FILE_BYTES as usize]).unwrap();
    let mut fs = RealFs;
    let accepted_metadata = fs.symlink_metadata(&path).unwrap();
    assert_eq!(accepted_metadata.len, MAX_FILE_BYTES);
    assert_eq!(
        fs.read_to_string(&path, MAX_FILE_BYTES as usize)
            .unwrap()
            .len(),
        MAX_FILE_BYTES as usize
    );

    std::fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap()
        .write_all(b"x")
        .unwrap();
    let error = match fs.read_to_string(&path, MAX_FILE_BYTES as usize) {
        Err(error) => error,
        Ok(text) => panic!("real reader accepted {} bytes after growth", text.len()),
    };

    assert!(error.contains("changed while scanning"), "{error}");
}

#[test]
fn real_walker_rejects_a_root_symlink_before_opening_it() {
    let crate_root = PathBuf::from("crate");
    let src = crate_root.join("src");
    let outside = PathBuf::from("outside");
    let mut fs = FakeFs::default();
    fs.insert(&src, FakeNode::Symlink(outside.clone()));
    fs.insert(&outside, FakeNode::Directory);

    let error = match authored_sources_with_fs(&mut fs, &crate_root) {
        Err(error) => error,
        Ok(found) => panic!("walker accepted {} source files", found.len()),
    };

    assert!(error.contains("symlinks are not allowed"), "{error}");
    assert_eq!(fs.operations, [Operation::Inspect(src)]);
}

#[test]
fn real_walker_rejects_a_child_symlink_before_opening_it() {
    let crate_root = PathBuf::from("crate");
    let src = crate_root.join("src");
    let tests = crate_root.join("tests");
    let linked = src.join("linked");
    let outside = PathBuf::from("outside");
    let mut fs = FakeFs::default();
    fs.insert(&src, FakeNode::Directory);
    fs.insert(&tests, FakeNode::Directory);
    fs.insert(&linked, FakeNode::Symlink(outside.clone()));
    fs.insert(&outside, FakeNode::Directory);

    let error = match authored_sources_with_fs(&mut fs, &crate_root) {
        Err(error) => error,
        Ok(found) => panic!("walker accepted {} source files", found.len()),
    };

    assert!(error.contains("symlinks are not allowed"), "{error}");
    assert_eq!(
        fs.operations,
        [
            Operation::Inspect(src.clone()),
            Operation::ReadDirectory(src),
            Operation::Inspect(linked),
        ]
    );
}

#[test]
fn real_walker_rejects_an_oversized_rust_file_before_reading_it() {
    let crate_root = PathBuf::from("crate");
    let src = crate_root.join("src");
    let tests = crate_root.join("tests");
    let oversized = src.join("oversized.rs");
    let mut fs = FakeFs::default();
    fs.insert(&src, FakeNode::Directory);
    fs.insert(&tests, FakeNode::Directory);
    fs.insert(
        &oversized,
        FakeNode::File(vec![b'x'; (MAX_FILE_BYTES + 1) as usize]),
    );

    let error = match authored_sources_with_fs(&mut fs, &crate_root) {
        Err(error) => error,
        Ok(found) => panic!("walker accepted {} source files", found.len()),
    };

    assert!(error.contains("exceeds file-size limit"), "{error}");
    assert_eq!(
        fs.operations,
        [
            Operation::Inspect(src.clone()),
            Operation::ReadDirectory(src),
            Operation::Inspect(oversized),
        ]
    );
}

#[test]
fn real_walker_accepts_a_rust_file_at_the_size_boundary() {
    let crate_root = PathBuf::from("crate");
    let src = crate_root.join("src");
    let tests = crate_root.join("tests");
    let boundary = src.join("boundary.rs");
    let mut fs = FakeFs::default();
    fs.insert(&src, FakeNode::Directory);
    fs.insert(&tests, FakeNode::Directory);
    fs.insert(
        &boundary,
        FakeNode::File(vec![b'x'; MAX_FILE_BYTES as usize]),
    );

    let found = authored_sources_with_fs(&mut fs, &crate_root).unwrap();

    assert_eq!(found.len(), 1);
    assert_eq!(found[0].0, "src/boundary.rs");
    assert_eq!(found[0].1.len(), MAX_FILE_BYTES as usize);
    assert_eq!(fs.operations.last(), Some(&Operation::ReadFile(boundary)));
}

/// The walk must actually reach the crate.
///
/// Without this, a `read_dir` that returned almost nothing would satisfy the assertion above while
/// scanning nothing -- the vacuous pass that the previous `include_str!` list was chosen to avoid.
/// The floor is the replacement for that property, and it is the current count rather than a
/// number chosen to be comfortably under it. A floor with slack tolerates the silent shrinkage it
/// exists to catch.
///
/// This crate currently has 100 authored Rust files. The cost is that a legitimate removal now
/// edits this number, which is the plausible-looking edit
/// a floor is supposed to resist. So the rule beside it: **lower this only in the same commit as
/// the removal that caused it, and name the removed file in this comment.** The landmarks below and this count then
/// fail on different work, which is the whole reason for keeping both -- lowering a threshold is a
/// plausible edit, deleting a named assertion is a visible one.
#[test]
fn the_scan_covers_the_whole_crate() {
    let found = sources();
    assert!(
        found.len() >= 100,
        "HARNESS-BROKE: the walk found only {} authored Rust files, so the scan above reads far less \
         than this crate",
        found.len()
    );
    for landmark in [
        "src/main.rs",
        "src/commands/wake_wait.rs",
        "tests/schema_cli.rs",
        "tests/source_invariants.rs",
    ] {
        assert!(
            found.iter().any(|(path, _)| path == landmark),
            "HARNESS-BROKE: known file {landmark} is absent from the walk"
        );
    }
}

#[test]
fn every_content_exemption_is_load_bearing_and_bounded() {
    let mut canonical_json = Vec::new();
    let mut detector_fixtures = Vec::new();
    for (path, text) in sources() {
        for (number, line) in text.lines().enumerate() {
            if !has_run_in_literal(line) {
                continue;
            }
            let location = format!("{path}:{}", number + 1);
            match exemption(&path, line) {
                Some(Exemption::CanonicalJsonFixture) => canonical_json.push(location),
                Some(Exemption::DetectorFixture) => detector_fixtures.push(location),
                Some(Exemption::Comment) | None => {}
            }
        }
    }
    assert_eq!(canonical_json, ["tests/schema_cli.rs:1298"]);
    assert_eq!(detector_fixtures.len(), 6);
    assert!(
        detector_fixtures
            .iter()
            .all(|location| location.starts_with("tests/source_invariants.rs:")),
        "detector fixture exemptions escaped their owning test file: {detector_fixtures:?}"
    );
}

/// The predicate itself, because it is the part that decides what everything else means.
///
/// The false-negative case matters as much as the false positives: a guard tuned until it stops
/// complaining is a guard that stops working.
#[test]
fn the_predicate_ignores_ordinary_rust_and_still_catches_the_defect() {
    // PRECONDITION for the two cases below, and it is not ceremony. Their fixture property
    // is "this line CARRIES a run of three or more spaces, outside any literal". Lose one
    // space to an edit and `!offends(..)` collapses to `!false` and passes having measured
    // nothing -- and it would keep passing with the even-segment bug back in place, which is
    // the exact defect these two cells exist to catch. The pair is tight in both directions:
    // if the run vanished the precondition fails, and if it moved INSIDE a literal `offends`
    // becomes true and the assertion fails.
    let aligned = r#"let s = "ok"; //   aligned trailing comment"#;
    assert!(
        aligned.contains("   "),
        "the aligned-comment fixture stopped carrying a run, so the assertion below measures nothing"
    );
    assert!(
        !offends("fixture.rs", aligned),
        "a trailing aligned comment after a literal is ordinary Rust"
    );

    let between = r#"let a = "x";        let b = "y";"#;
    assert!(
        between.contains("   "),
        "the between-literals fixture stopped carrying a run, so the assertion below measures nothing"
    );
    assert!(
        !offends("fixture.rs", between),
        "spacing between two literals is ordinary Rust"
    );
    // PRECONDITION for the comment case, and it guards a correction rather than a fixture this
    // branch wrote. The fixture below was already repaired once, after a sabotage showed the
    // exemption was never reached. Nothing asserted it stays repaired: tidy the run out of its
    // inner literal and `has_run_in_literal` answers false again, the exemption goes unconsulted,
    // and the assertion passes for exactly the reason the repair removed -- with the comment above
    // it still explaining why that cannot happen.
    //
    // It composes `has_run_in_literal` instead of re-splitting on quotes. A precondition that
    // re-implements the predicate it validates inherits that predicate's blind spots by
    // construction, and this one would have inherited the escaped-quote bug the shared version
    // was cured of: in `let s = "a \" b   c";` a naive split flips the parity and reads the run
    // as being outside the literal. A second opinion assembled from the first opinion's parts is
    // not a second opinion.
    let commented = r#"//   let s = "a          b";"#;
    assert!(
        has_run_in_literal(commented),
        "the comment fixture stopped carrying a run INSIDE a literal, so the exemption below is \
         never reached and the assertion passes whether the exemption works or not -- which is the \
         defect this fixture was already corrected for once"
    );
    assert!(
        // The comment exemption must be REACHED to be observed. This fixture was
        // `"///   a doc comment whose indent is an intentional list"` -- a line with no
        // string literal in it at all, so `has_run_in_literal` answered `false` before the
        // exemption was ever consulted and the assertion passed whether the exemption
        // worked or not. Found by sabotaging `is_line_comment` against the pathogens copy
        // of this same fixture and watching nothing go red.
        !offends("fixture.rs", commented),
        "comments are excluded: their indentation is often deliberate"
    );
    assert!(
        offends(
            "fixture.rs",
            r#"    "a real defect with          collapsed indent","#
        ),
        "the defect itself must still be caught"
    );
}
