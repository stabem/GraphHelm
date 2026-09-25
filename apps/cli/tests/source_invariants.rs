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
                    !trimmed.starts_with("//") && line.contains("ClearanceVerifier::Countersign")
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
    // The coordinate moved from :1298 to :1303 when this branch inserted five lines ABOVE the
    // fixture in schema_cli.rs. Nothing about the exemption changed -- the same literal, the same
    // file, the same single canonical-JSON site -- so this guard failed for a reason unrelated to
    // what it guards, which costs a debugging session before it helps. A hand-written line number
    // is a citation, and citations rot on any insertion above them. Filed rather than redesigned
    // here: pinning by CONTENT (one exemption, in this file, at the canonical-JSON literal) is the
    // durable shape, and rewriting another lane's guard inside a 59-commit branch is not this
    // fix's job.
    assert_eq!(canonical_json, ["tests/schema_cli.rs:1310"]);
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

/// A CALLER OF THE USER-SCOPED CONSTRUCTOR MAY NOT APPEND EVENTS (#775, closing the gap #820 filed).
///
/// `execution_cli.rs` says the gap in its own words, and this cell is the assertion it asked for:
///
/// > The structural half -- that `start`, `signal`, `amend` and the driver all reach
/// > `addressable_scope`, and that `events::scope` has only read-path callers -- is an argument in
/// > the PR body, not an assertion here, and **nothing re-reads it when a site is added.**
///
/// **Why that argument is load-bearing.** `list_streams` keys on `(scope, stream_id)`, so two
/// streams may share an id under different scopes and the store is happy to hold both. Three
/// readers locate a stream by id alone and take the first match -- `serve/monitor.rs`, and
/// `serve/wake.rs` twice. They are correct only because that collision cannot exist, and it cannot
/// exist only because **every production writer's scope is DERIVED from the stream id** through
/// `addressable_scope`. The sibling guard below keeps a second hand-written spelling of that rule
/// out. This one keeps the OTHER door shut.
///
/// `events::scope` is the one production constructor that takes the workspace and project from the
/// caller, so it is the only place a scope can be CHOSEN rather than derived. Measured on this
/// tree, it has exactly two callers and neither writes events:
///
/// ```text
/// src/commands/events/verify.rs    reads a repository
/// src/commands/events/rebuild.rs   writes a PROJECTION generation, appends no events
/// ```
///
/// `rebuild` is worth naming rather than filing under "read-only": it does write, to PostgreSQL,
/// and it is safe here for a different reason -- a projection generation is not a stream, so it
/// cannot bring a `(scope, stream_id)` pair into existence. A guard that said "these commands do
/// not write" would be false about it and would be deleted by the first person who checked.
///
/// **Derived, not listed.** The two files above are not named in the assertion. Anything that calls
/// the user-scoped constructor and also builds an append is an offender, so a THIRD caller written
/// tomorrow is judged by the same rule rather than by a list somebody has to widen -- which is the
/// property #577 and #927 both had to fix after a hand-written population went stale.
///
/// **What this does NOT claim.** It does not prove the collision is unreachable; it pins the one
/// structural fact the reachability argument rests on. A writer that obtained a chosen scope some
/// other way -- deserialised, cloned from a foreign row, handed in by a future caller of a future
/// constructor -- is outside it, and #775's decision (ids unique per repository, or every reader
/// takes a scope) is what would make the readers correct by construction instead.
#[test]
fn no_caller_of_the_user_scoped_constructor_appends_events() {
    let scanned = sources();
    let production: Vec<(String, String)> = scanned
        .iter()
        .filter(|(path, _)| path.starts_with("src/"))
        .cloned()
        .collect();

    // ---- THE MATCHER FIRST. Once this lands, `offenders` is empty for as long as the rule holds,
    // and an empty result proves nothing about whether the matcher can still find anything.
    let both = vec![(
        "src/fake.rs".to_owned(),
        "let scope = super::scope(w, p, e)?;\nstore.append_atomic(&request)?;".to_owned(),
    )];
    assert_eq!(
        user_scoped_writer_offenders(&both).len(),
        1,
        "HARNESS-BROKE: a file that both chooses a scope and appends is the whole subject, and the \
         matcher does not see it"
    );

    // Each half alone is legitimate and must not be accused: the constructor's existing callers
    // read, and every event writer in this crate derives its scope instead of choosing one.
    let scope_only = vec![(
        "src/fake.rs".to_owned(),
        "let scope = super::scope(w, p, e)?;\nverify(&scope)".to_owned(),
    )];
    assert!(
        user_scoped_writer_offenders(&scope_only).is_empty(),
        "choosing a scope is what `events verify` legitimately does; alone it is not the defect"
    );
    // THE THIRD SPELLING, added after ISSUES 1 measured that `append_event(` -- eleven
    // production sites, and the helper this file's own doc names -- was invisible to the
    // matcher.
    let via_helper = vec![(
        "src/fake.rs".to_owned(),
        "let scope = super::scope(w, p, e)?;\nappend_event(store, &scope, &stream, event)?;"
            .to_owned(),
    )];
    assert_eq!(
        user_scoped_writer_offenders(&via_helper).len(),
        1,
        "a chosen scope appended through `append_event` is the same defect as one appended \\
         directly, and the matcher missed it until #942's review"
    );

    let append_only = vec![(
        "src/fake.rs".to_owned(),
        "let scope = super::addressable_scope(id)?;\nstore.append_atomic(&request)?;".to_owned(),
    )];
    assert!(
        user_scoped_writer_offenders(&append_only).is_empty(),
        "appending under a DERIVED scope is what every shipped writer does and is the safe case"
    );

    // A commented-out call is prose, and prose about this rule is exactly what the file that
    // defines the constructor is full of.
    let commented = vec![(
        "src/fake.rs".to_owned(),
        "// let scope = super::scope(w, p, e)?;\nstore.append_atomic(&request)?;".to_owned(),
    )];
    assert!(
        user_scoped_writer_offenders(&commented).is_empty(),
        "a call inside a comment is not a call"
    );

    // ---- and only now the real tree.
    assert!(
        !production.is_empty(),
        "precondition: the walk must have returned production sources, or the absence asserted \
         below is the absence of a SCAN"
    );

    // PRESENCE, not just population. "Zero offenders" is also what a scope pattern that matches
    // NOTHING produces, and that failure is invisible from the result. This asserts the scan finds
    // the constructor's real callers, so the emptiness below is about the second half of the rule.
    let callers: Vec<&String> = production
        .iter()
        .filter(|(_, text)| calls_user_scoped_constructor(text))
        .map(|(path, _)| path)
        .collect();
    assert!(
        !callers.is_empty(),
        "HARNESS-BROKE: no production file appears to call the user-scoped constructor, so the \
         first half of this rule matches nothing and the guard is vacuous"
    );

    let offenders = user_scoped_writer_offenders(&production);
    assert!(
        offenders.is_empty(),
        "these choose a RepositoryScope from caller-supplied workspace/project AND append events \
         under it. That is the one way two streams can come to share a stream_id under different \
         scopes, and three readers in `serve` locate a stream by id alone and take the first \
         match, so they would then answer about a different execution than the /v1 verbs do \
         (#775). Derive the scope from the stream id through \
         `super::execution::addressable_scope` instead:\n{}",
        offenders.join("\n")
    );
}

/// Whether this source CALLS the constructor that takes workspace and project from its caller.
///
/// Both spellings, because the existing callers sit inside the `events` module and reach it as
/// `super::scope`, while anything written outside it would reach it as `events::scope`. Matching
/// one spelling would make a future caller in a new module invisible, which is the population this
/// guard exists for rather than the one it already knows about.
fn calls_user_scoped_constructor(text: &str) -> bool {
    text.lines()
        .filter(|line| !line.trim_start().starts_with("//"))
        .any(|line| line.contains("super::scope(") || line.contains("events::scope("))
}

/// Whether this source builds an event append.
///
/// **`append_event(` is the third spelling and it was missing** (found by ISSUES 1 reviewing #942).
/// It is the helper `execution/mod.rs` exports, with eleven call sites across `cancel.rs`,
/// `driver.rs` and `mod.rs` -- and this file's own doc names it, so a guard that could not see it
/// was stricter in prose than in code. Measured across `apps/cli/src` at the time:
///
/// ```text
/// PreparedAppend::new(   19   matched
/// .append_atomic(        19   matched
/// append_event(          11   NOT matched   <- the gap
/// .append(                3   OpenOptions::append(true) -- file I/O, correctly out
/// ```
///
/// **This is still a list, and the list is the weakness.** Deriving it -- every `fn` whose body
/// reaches `append_atomic` -- is the version that stops needing maintenance, and it is bigger than
/// this guard. Named here so the next person widening it knows they are paying interest rather
/// than fixing something.
fn appends_events(text: &str) -> bool {
    text.lines()
        .filter(|line| !line.trim_start().starts_with("//"))
        .any(|line| {
            line.contains("append_atomic(")
                || line.contains("PreparedAppend::new(")
                || line.contains("append_event(")
        })
}

fn user_scoped_writer_offenders(sources: &[(String, String)]) -> Vec<String> {
    sources
        .iter()
        .filter(|(_, text)| calls_user_scoped_constructor(text) && appends_events(text))
        .map(|(path, _)| path.clone())
        .collect()
}

/// The addressing rule has ONE home, and a production `RepositoryScope` must not spell it out
/// (#820, enforcing the rule #560 recorded).
///
/// `addressable_scope` (`src/commands/execution/mod.rs`) says it in its own doc: **"Every consumer
/// of this rule must call THIS function (#560)."** That rule was written after the rule had been
/// restated where `resolve_stream` needed it and `execution list` enumerated the repository
/// without it, so the index offered rows the rest of the API could not answer for. The rule was
/// stated and never enforced, which is why this cell exists rather than a second comment.
///
/// **Why the addressing rule is load-bearing and not a formatting preference.** Three readers
/// locate a stream by id alone and take the first match -- `serve/monitor.rs`, and `serve/wake.rs`
/// twice. They are correct only because no two streams can share a `stream_id`, and that holds
/// only because the scope is DERIVED from the id rather than chosen: `(scope, stream_id)` then
/// carries no more information than `stream_id`. A second, hand-written spelling of the rule is a
/// duplicated ORACLE. It does not diverge loudly the way a duplicated mechanism does; it agrees
/// until the day the constants move, and then one writer addresses streams the readers cannot find.
///
/// **The subject is CONSTRUCTION, not appending, and that is a correction to this ticket's first
/// framing.** The obvious sweep -- every `append_atomic` site takes its scope from
/// `addressable_scope` -- cannot be written: measured across the tree, the production append sites
/// are 15 across five crates, and every one outside `apps/cli` takes its scope from a parameter or
/// a struct field (`core/runtime/src/driver.rs`, `core/governor/src/apply.rs`,
/// `core/events/src/sweep.rs`, `core/simulation/src/engine.rs`). "Comes from a parameter" is also
/// true of a site that chose its own scope and passed it down, so as an assertion it accepts
/// exactly what it exists to reject. Appends PROPAGATE a scope; only a construction CHOOSES one.
///
/// **Keying on the type is what makes the one earned exclusion free.** `src/commands/development.rs`
/// builds a `DevelopmentScope` from the same two literals and is not a violation: different type,
/// no repository, nothing persisted -- its own doc says so. A guard keyed on the literals alone
/// accuses it. Keyed on `RepositoryScope::new`, it never sees it, and no special case has to be
/// written down and maintained.
///
/// **The rule's own home is NOT excluded, and an earlier version of this cell excluded it.**
/// `addressable_scope` builds from the named constants, so it never matches the pattern -- which
/// means the exclusion removed nothing and cost the coverage of the one file where a second
/// hand-spelling is most likely to be written, by someone who assumes being at the rule's home
/// makes them exempt. Measured both ways in review: identical, zero offenders either side. The
/// literals live there only as the `const` declarations at the top, 144 lines from the nearest
/// construction, and the test-module construction below them uses different values entirely.
///
/// The precondition that stood under that exclusion is the instructive part. It asserted the home
/// was IN the population -- proving the SCAN reached it -- and passed happily while the exclusion
/// excluded nothing, because being scanned and being matched are different properties. A control
/// one step to the side of the property it is guarding reads exactly like the real thing.
#[test]
fn no_production_repository_scope_spells_the_addressing_rule_by_hand() {
    let scanned = sources();
    let production: Vec<(String, String)> = scanned
        .iter()
        .filter(|(path, _)| path.starts_with("src/"))
        .cloned()
        .collect();

    // ---- THE MATCHER FIRST, because after this lands it is the only thing left that can fail.
    //
    // The two preconditions below prove the SCAN ran. Neither proves the matcher can still FIND a
    // violation, and once `simulate.rs` is fixed `offenders` is empty forever: a window that is too
    // small, an edited literal and a spelling that no longer matches all produce the same green as
    // a clean tree. Raised in review, and it is the same hole I had to declare on a sibling guard --
    // a hand-chosen window that today's single offender never exercised.
    let at_one = vec![sample("src/fake.rs", 1)];
    assert_eq!(
        addressing_rule_offenders(&at_one).len(),
        1,
        "HARNESS-BROKE: the matcher does not recognise a literal on the line after the construction"
    );

    // SIX AND SEVEN ARE WRITTEN OUT, NOT DERIVED FROM `ARGUMENT_LINES`. Computed from the
    // constant under test they move with it: shrinking the window to 2 kept both canaries green,
    // measured. An expectation built from the value it is meant to pin is a mirror.
    let at_window_edge = vec![sample("src/fake.rs", 6)];
    assert_eq!(
        addressing_rule_offenders(&at_window_edge).len(),
        1,
        "HARNESS-BROKE: the matcher misses a literal on the LAST line it claims to read, so the window is smaller than {ARGUMENT_LINES}"
    );

    // DECLARED LIMIT, asserted rather than described: one line past the window is INVISIBLE. The
    // real offender put its literals at +1 and +2, so no value between 2 and 6 was ever
    // distinguished by this tree -- the number is chosen, not measured. Errs wide on purpose: a
    // window too NARROW fails open and silently, and this cell says exactly where the edge is.
    let past_the_window = vec![sample("src/fake.rs", 7)];
    assert!(
        addressing_rule_offenders(&past_the_window).is_empty(),
        "the window is {ARGUMENT_LINES} lines and a literal beyond it is not seen; if this now fails the window grew and the comment above is stale"
    );

    // TYPE KEYING, converted from a paragraph into an assertion. `src/commands/development.rs`
    // builds a `DevelopmentScope` from these same two literals and is not a violation -- different
    // type, no repository, nothing persisted. This is why the guard keys on `RepositoryScope::new`
    // rather than on the literals, and why that exclusion needs no maintained special case.
    let other_type = vec![(
        "src/fake.rs".to_owned(),
        "let scope = DevelopmentScope {\nworkspace_id: WorkspaceId::parse(\"workspace-local\"),\nproject_id: ProjectId::parse(\"project-local\"),\n};".to_owned(),
    )];
    assert!(
        addressing_rule_offenders(&other_type).is_empty(),
        "a DevelopmentScope built from the same literals is a different type and must not be accused"
    );

    // And the RULE'S OWN HOME passes for the right reason rather than by its path exclusion:
    // `addressable_scope` builds from the named constants, so it never matches the pattern.
    let via_constants = vec![(
        "src/fake.rs".to_owned(),
        "RepositoryScope::new(\nWorkspaceId::parse(WORKSPACE),\nProjectId::parse(PROJECT),\n)"
            .to_owned(),
    )];
    assert!(
        addressing_rule_offenders(&via_constants).is_empty(),
        "a construction from the named constants is the rule being CALLED, not restated"
    );

    // ---- and only now the real tree.

    // Presence control for an absence guard: an empty population makes the assertion below pass
    // while measuring nothing.
    assert!(
        !production.is_empty(),
        "precondition: the walk must have returned production sources, or the absence asserted below is the absence of a SCAN"
    );

    let offenders = addressing_rule_offenders(&production);

    assert!(
        offenders.is_empty(),
        "these build a RepositoryScope from the addressing rule's literals instead of calling `addressable_scope`, which is a second oracle for the rule that makes three id-only stream lookups correct. Call `super::execution::addressable_scope` instead:\n{}",
        offenders.join("\n")
    );
}

const ARGUMENT_LINES: usize = 6;

/// The matcher, lifted out of the cell so it can be fed text and shown to still work.
///
/// A guard whose only input is the repository can only be observed on the day it fires. This one
/// is meant never to fire again, so its own health has to be measurable without one.
fn addressing_rule_offenders(sources: &[(String, String)]) -> Vec<String> {
    const LITERALS: [&str; 2] = ["\"workspace-local\"", "\"project-local\""];

    sources
        .iter()
        .flat_map(|(path, text)| {
            let lines: Vec<&str> = text.lines().collect();
            let mut hits = Vec::new();
            for (number, line) in lines.iter().enumerate() {
                if line.trim_start().starts_with("//") || !line.contains("RepositoryScope::new(") {
                    continue;
                }
                let end = (number + 1 + ARGUMENT_LINES).min(lines.len());
                let arguments = lines[number..end].join("\n");
                if LITERALS.iter().any(|literal| arguments.contains(literal)) {
                    hits.push(format!("{path}:{}: {}", number + 1, line.trim_start()));
                }
            }
            hits
        })
        .collect()
}

/// A construction whose literal sits `offset` lines below it, for exercising the window.
fn sample(path: &str, offset: usize) -> (String, String) {
    let mut text = String::from("let scope = RepositoryScope::new(\n");
    for _ in 1..offset {
        text.push_str("// filler that carries no literal\n");
    }
    text.push_str("WorkspaceId::parse(\"workspace-local\"),\n);");
    (path.to_owned(), text)
}

/// THE MATCHER READS TWO SPELLINGS, AND THIS IS WHY THAT IS ENOUGH (#942's review).
///
/// `calls_user_scoped_constructor` looks for `super::scope(` and `events::scope(`. Neither sees the
/// ordinary Rust idiom -- `use crate::commands::events::scope;` and then a bare `scope(...)`.
///
/// ISSUES 1 raised it and proposed two answers: widen the matcher to a word-boundary match on
/// `scope(`, or assert the import does not exist. The first is riskier than the thing it guards --
/// `scope(` is a common word in this crate, and a matcher that also fired on `addressable_scope(`
/// or `wake_scope(` would cry wolf until somebody deleted it. The second is smaller and fully
/// derivable, and it is what this cell does.
///
/// So the pair is honest about its shape: the matcher reads the two qualified spellings, and this
/// asserts the third does not arise. If it ever does, this fails and names the file, and whoever
/// adds the import decides then whether to qualify the call or widen the matcher -- with the
/// trade-off already written down rather than re-derived.
#[test]
fn no_production_source_imports_the_user_scoped_constructor_by_bare_name() {
    let scanned = sources();
    let production: Vec<(String, String)> = scanned
        .iter()
        .filter(|(path, _)| path.starts_with("src/"))
        .cloned()
        .collect();
    assert!(
        !production.is_empty(),
        "precondition: the walk must have returned production sources"
    );

    // A matcher control first: this is meant never to fire, so its health cannot be read off a
    // green run over a clean tree.
    let planted = vec![(
        "src/fake.rs".to_owned(),
        "use crate::commands::events::scope;".to_owned(),
    )];
    assert_eq!(
        bare_scope_imports(&planted).len(),
        1,
        "HARNESS-BROKE: a bare import of the constructor is the subject and the matcher missed it"
    );
    let qualified = vec![(
        "src/fake.rs".to_owned(),
        "use crate::commands::events;".to_owned(),
    )];
    assert!(
        bare_scope_imports(&qualified).is_empty(),
        "importing the MODULE is the spelling the matcher already reads and must not be accused"
    );

    let offenders = bare_scope_imports(&production);
    assert!(
        offenders.is_empty(),
        "these import the user-scoped constructor by bare name, which the matcher above cannot see, so a writer in them would go unjudged (#775, #942): {offenders:?}"
    );
}

/// Sources importing `scope` itself rather than the module that holds it.
fn bare_scope_imports(sources: &[(String, String)]) -> Vec<String> {
    sources
        .iter()
        .filter(|(_, text)| {
            text.lines()
                .filter(|line| !line.trim_start().starts_with("//"))
                .any(|line| {
                    let trimmed = line.trim_start();
                    trimmed.starts_with("use ")
                        && trimmed.contains("events::")
                        && (trimmed.contains("::scope;")
                            || trimmed.contains("::scope,")
                            || trimmed.contains("{scope")
                            || trimmed.contains(" scope,")
                            || trimmed.contains(" scope}"))
                })
        })
        .map(|(path, _)| path.clone())
        .collect()
}
