//! Enforces source properties which runtime tests cannot observe.

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

fn read_bounded<R: std::io::Read>(reader: R, limit: usize) -> Result<Vec<u8>, String> {
    use std::io::Read;

    let mut bytes = Vec::with_capacity(limit.min(64 * 1024));
    reader
        .take(limit as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| format!("cannot read authored Rust file: {e}"))?;
    if bytes.len() > limit {
        return Err(format!(
            "HARNESS-BROKE: authored Rust file changed while scanning and exceeds {limit} bytes"
        ));
    }
    Ok(bytes)
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
        let file = std::fs::File::open(path)
            .map_err(|e| format!("cannot open {}: {e}", path.display()))?;
        let bytes = read_bounded(file, limit)
            .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
        String::from_utf8(bytes)
            .map_err(|e| format!("cannot decode {} as UTF-8: {e}", path.display()))
    }
}

fn require_directory(path: &Path, metadata: MetadataSnapshot) -> Result<(), String> {
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

fn authored_sources_with_fs<F: AuthoredFs>(
    fs: &mut F,
    crate_root: &Path,
) -> Result<Vec<(String, String)>, String> {
    fn walk<F: AuthoredFs>(
        fs: &mut F,
        dir: &Path,
        depth: usize,
        entries_seen: &mut usize,
        found: &mut Vec<PathBuf>,
    ) -> Result<(), String> {
        let metadata = fs.symlink_metadata(dir)?;
        require_directory(dir, metadata)?;
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
                walk(fs, &path, depth + 1, entries_seen, found)?;
            } else if metadata.is_file && path.extension().is_some_and(|ext| ext == "rs") {
                if metadata.len > MAX_FILE_BYTES {
                    return Err(format!(
                        "HARNESS-BROKE: {} is {} bytes and exceeds {MAX_FILE_BYTES} bytes",
                        path.display(),
                        metadata.len
                    ));
                }
                if found.len() >= MAX_RUST_FILES {
                    return Err(format!(
                        "HARNESS-BROKE: authored Rust walk exceeded {MAX_RUST_FILES} Rust files"
                    ));
                }
                found.push(path);
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

fn sources() -> Vec<(String, String)> {
    authored_sources_with_fs(&mut RealFs, Path::new(env!("CARGO_MANIFEST_DIR")))
        .unwrap_or_else(|e| panic!("{e}"))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Exemption {
    Comment,
    HtmlFixture,
    DetectorFixture,
}

fn detector_fixture_source_lines() -> Vec<String> {
    let three = " ".repeat(3);
    let four = " ".repeat(4);
    let eight = " ".repeat(8);
    let ten = " ".repeat(10);
    let nineteen = " ".repeat(19);
    vec![
        format!(r##"let aligned = r#"let s = "ok"; //{three}aligned trailing comment"#;"##),
        format!("assert!(aligned.contains(\"{three}\"));"),
        format!(r##"let between = r#"let a = "x";{eight}let b = "y";"#;"##),
        format!("assert!(between.contains(\"{three}\"));"),
        format!(r##"let comment = r#"//{three}let s = "a{ten}b";"#;"##),
        format!(r##"let exempt_by_role = r#"{eight}html: "<main>x</a>{nineteen}<section>","#;"##),
        format!(r##"r#"{four}"a real defect with{ten}collapsed indent","#"##),
    ]
}

fn exemption(path: &str, line: &str) -> Option<Exemption> {
    if is_line_comment(line) {
        Some(Exemption::Comment)
    } else if line.trim_start_matches(' ').starts_with("html:") {
        Some(Exemption::HtmlFixture)
    } else if path == "tests/source_invariants.rs"
        && detector_fixture_source_lines()
            .iter()
            .any(|fixture| fixture == line.trim_start())
    {
        Some(Exemption::DetectorFixture)
    } else {
        None
    }
}

fn offends(path: &str, line: &str) -> bool {
    has_run_in_literal(line) && exemption(path, line).is_none()
}

#[test]
fn refusal_strings_carry_no_collapsed_indentation() {
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
        "these string literals carry runs of whitespace, which a reader gets verbatim. The `\\` continuation that would have removed them was lost before the file was written -- commonly a generator consuming the escape (#440). Remove the run here; if a script wrote this file, fix the script too or it comes back.\n{}",
        offenders.join("\n")
    );
}

#[derive(Clone, Debug)]
enum FakeNode {
    Directory,
    File(Vec<u8>),
    Symlink(PathBuf),
    Other,
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

    fn crate_with_one_file_in_each_authored_root() -> Self {
        let mut fs = Self::default();
        fs.insert("crate/src", FakeNode::Directory);
        fs.insert("crate/tests", FakeNode::Directory);
        fs.insert(
            "crate/src/lib.rs",
            FakeNode::File(b"pub fn x() {}".to_vec()),
        );
        fs.insert(
            "crate/tests/guard.rs",
            FakeNode::File(b"#[test] fn x() {}".to_vec()),
        );
        fs
    }

    fn resolved_node(&self, path: &Path) -> Result<&FakeNode, String> {
        let direct = self
            .nodes
            .get(path)
            .ok_or_else(|| format!("fake path is absent: {}", path.display()))?;
        match direct {
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
                FakeNode::Directory | FakeNode::Symlink(_) | FakeNode::Other => 0,
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
fn authored_walker_reaches_tests_through_the_injected_filesystem() {
    let mut fs = FakeFs::crate_with_one_file_in_each_authored_root();
    let found = authored_sources_with_fs(&mut fs, Path::new("crate")).unwrap();
    assert_eq!(
        found
            .iter()
            .map(|(path, _)| path.as_str())
            .collect::<Vec<_>>(),
        ["src/lib.rs", "tests/guard.rs"]
    );
}

#[test]
fn walker_skips_regular_non_rust_files_but_refuses_special_entries() {
    let mut fs = FakeFs::crate_with_one_file_in_each_authored_root();
    fs.insert("crate/src/notes.txt", FakeNode::File(b"notes".to_vec()));
    let found = authored_sources_with_fs(&mut fs, Path::new("crate")).unwrap();
    assert_eq!(found.len(), 2);
    assert!(
        !fs.operations
            .contains(&Operation::ReadFile(PathBuf::from("crate/src/notes.txt")))
    );

    fs.insert("crate/src/device", FakeNode::Other);
    let error = authored_sources_with_fs(&mut fs, Path::new("crate")).unwrap_err();
    assert!(error.contains("unexpected filesystem entry"), "{error}");
}

#[test]
fn walker_rejects_a_root_symlink_before_reading_or_opening_it() {
    let mut fs = FakeFs::default();
    fs.insert("crate/src", FakeNode::Symlink(PathBuf::from("outside")));
    fs.insert("outside", FakeNode::Directory);
    let error = authored_sources_with_fs(&mut fs, Path::new("crate")).unwrap_err();
    assert!(error.contains("symlinks are not allowed"), "{error}");
    assert_eq!(
        fs.operations,
        [Operation::Inspect(PathBuf::from("crate/src"))]
    );
}

#[test]
fn walker_rejects_a_child_symlink_before_reading_or_opening_it() {
    let mut fs = FakeFs::crate_with_one_file_in_each_authored_root();
    fs.insert(
        "crate/src/linked.rs",
        FakeNode::Symlink(PathBuf::from("outside.rs")),
    );
    fs.insert("outside.rs", FakeNode::File(b"secret".to_vec()));
    let error = authored_sources_with_fs(&mut fs, Path::new("crate")).unwrap_err();
    assert!(error.contains("symlinks are not allowed"), "{error}");
    let linked = PathBuf::from("crate/src/linked.rs");
    assert!(!fs.operations.contains(&Operation::ReadFile(linked.clone())));
    assert!(!fs.operations.contains(&Operation::ReadDirectory(linked)));
}

#[test]
fn walker_checks_metadata_size_before_opening_a_rust_file() {
    let mut fs = FakeFs::crate_with_one_file_in_each_authored_root();
    let oversized = PathBuf::from("crate/src/oversized.rs");
    fs.insert(
        &oversized,
        FakeNode::File(vec![b'x'; (MAX_FILE_BYTES + 1) as usize]),
    );
    let error = authored_sources_with_fs(&mut fs, Path::new("crate")).unwrap_err();
    assert!(error.contains("exceeds"), "{error}");
    assert!(!fs.operations.contains(&Operation::ReadFile(oversized)));
}

#[test]
fn real_bounded_reader_accepts_the_limit_and_rejects_growth() {
    let exact = vec![b'x'; MAX_FILE_BYTES as usize];
    assert_eq!(
        read_bounded(std::io::Cursor::new(&exact), MAX_FILE_BYTES as usize)
            .unwrap()
            .len(),
        MAX_FILE_BYTES as usize
    );
    let grown = vec![b'x'; MAX_FILE_BYTES as usize + 1];
    let error = match read_bounded(std::io::Cursor::new(grown), MAX_FILE_BYTES as usize) {
        Err(error) => error,
        Ok(bytes) => panic!("bounded reader accepted {} bytes after growth", bytes.len()),
    };
    assert!(error.contains("changed while scanning"), "{error}");
}

struct IsolatedTempDirectory {
    path: PathBuf,
}

impl IsolatedTempDirectory {
    fn create() -> Result<Self, String> {
        let root = std::env::temp_dir();
        for attempt in 0..128 {
            let path = root.join(format!(
                "graphhelm-pathogens-source-invariants-{}-{attempt}",
                std::process::id()
            ));
            match std::fs::create_dir(&path) {
                Ok(()) => return Ok(Self { path }),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => {
                    return Err(format!(
                        "cannot create isolated temporary directory {}: {error}",
                        path.display()
                    ));
                }
            }
        }
        Err("cannot reserve an isolated temporary directory after 128 attempts".to_owned())
    }

    fn remove(self) -> Result<PathBuf, String> {
        let path = self.path.clone();
        std::fs::remove_dir_all(&path)
            .map_err(|e| format!("cannot remove isolated directory {}: {e}", path.display()))?;
        std::mem::forget(self);
        Ok(path)
    }
}

impl Drop for IsolatedTempDirectory {
    fn drop(&mut self) {
        if self.path.parent() == Some(std::env::temp_dir().as_path()) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }
}

#[test]
fn real_fs_rechecks_the_bound_after_metadata_before_accepting_growth() {
    use std::io::Write;

    let directory = IsolatedTempDirectory::create().unwrap();
    let path = directory.path.join("boundary.rs");
    std::fs::write(&path, vec![b'x'; MAX_FILE_BYTES as usize]).unwrap();

    let mut fs = RealFs;
    let metadata = fs.symlink_metadata(&path).unwrap();
    assert_eq!(metadata.len, MAX_FILE_BYTES);
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
        Ok(text) => panic!("real adapter accepted {} bytes after growth", text.len()),
    };
    assert!(error.contains("changed while scanning"), "{error}");

    let removed = directory.remove().unwrap();
    assert!(
        !removed.exists(),
        "temporary proof directory was not removed"
    );
}

#[test]
fn the_scan_covers_the_whole_crate() {
    let found = sources();
    assert!(
        found.len() >= 13,
        "HARNESS-BROKE: the walk found only {} authored Rust files; lower 13 only with a named removal",
        found.len()
    );
    for expected in [
        "src/lib.rs",
        "src/jpd.rs",
        "src/retry_lineage.rs",
        "tests/generic_harness.rs",
        "tests/source_invariants.rs",
    ] {
        assert!(
            found.iter().any(|(path, _)| path == expected),
            "HARNESS-BROKE: known file {expected} is absent from the walk"
        );
    }
    assert!(found.iter().all(|(path, _)| !path.contains('\\')));
}

#[test]
fn the_html_exemption_actually_suppresses_something() {
    let suppressed: Vec<String> = sources()
        .iter()
        .flat_map(|(path, text)| {
            text.lines()
                .enumerate()
                .filter(|(_, line)| {
                    line.trim_start_matches(' ').starts_with("html:") && has_run_in_literal(line)
                })
                .map(move |(number, _)| format!("{path}:{}", number + 1))
        })
        .collect();
    assert!(!suppressed.is_empty(), "HTML exemption suppresses nothing");
    assert!(suppressed.iter().all(|item| item.starts_with("src/")));
}

#[test]
fn detector_fixture_exemptions_are_load_bearing_and_bounded() {
    let suppressed: Vec<String> = sources()
        .iter()
        .flat_map(|(path, text)| {
            text.lines()
                .enumerate()
                .filter(|(_, line)| has_run_in_literal(line))
                .filter(|(_, line)| exemption(path, line) == Some(Exemption::DetectorFixture))
                .map(move |(number, _)| format!("{path}:{}", number + 1))
        })
        .collect();

    assert_eq!(
        suppressed.len(),
        7,
        "unexpected fixture set: {suppressed:?}"
    );
    assert!(
        suppressed
            .iter()
            .all(|location| location.starts_with("tests/source_invariants.rs:")),
        "fixture exemptions escaped their exact owning path: {suppressed:?}"
    );
}

#[test]
fn the_predicate_ignores_ordinary_rust_and_still_catches_the_defect() {
    let aligned = r#"let s = "ok"; //   aligned trailing comment"#;
    assert!(aligned.contains("   "));
    assert!(!offends("fixture.rs", aligned));

    let between = r#"let a = "x";        let b = "y";"#;
    assert!(between.contains("   "));
    assert!(!offends("fixture.rs", between));

    let comment = r#"//   let s = "a          b";"#;
    assert!(has_run_in_literal(comment));
    assert!(!offends("fixture.rs", comment));

    let exempt_by_role = r#"        html: "<main>x</a>                   <section>","#;
    assert!(has_run_in_literal(exempt_by_role));
    assert!(!offends("fixture.rs", exempt_by_role));

    assert!(offends(
        "fixture.rs",
        r#"    "a real defect with          collapsed indent","#
    ));
}
