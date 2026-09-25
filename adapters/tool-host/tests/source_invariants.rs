//! This crate's own collapsed-run guard, the thirteenth of the per-crate guards (#1132).
//!
//! WHY THIS CRATE NEEDED ONE AT ALL. `core/protocols/tests/authored_strings_across_the_workspace.rs`
//! sweeps every member, and it exempts nine files BY FILE. Six of those nine are guard files whose
//! own crate's per-crate `source_invariants.rs` still scans them, which is what keeps the by-file
//! shape affordable: the exemption removes a file from the central sweep, not from every sweep.
//! `adapters/tool-host/tests/cancellation_record_asymmetry.rs` was the exception -- a Species B
//! entry in that list with no per-crate guard behind it -- so authored prose anywhere in its 1807
//! lines was outside EVERY sweep. Lane S measured it on #1026: a planted ten-space run in that
//! file's `OBSERVER_MISSING` prose stayed silent workspace-wide.
//!
//! WHAT IS DIFFERENT HERE, and it is the only interesting part. The other twelve guards exempt a
//! fixture by matching a LINE. This one cannot: the census fixtures are forty lines, they change
//! whenever a census cell is added, and a forty-line roster is the hand-typed population the sweep
//! was built to dissolve. So the exemption is per-LITERAL and by ROLE -- a literal that spells the
//! census subject (`CapturedProcess` or `tree_kill`) is census input whose indentation IS the value
//! it asserts, and it is blanked; everything else on the same line is still read. A prose literal
//! sitting beside a fixture literal is therefore covered, which a line-level exemption cannot do.
//!
//! **THE RESIDUAL, NAMED.** Two, both narrower than the hole they replace:
//!
//! 1. A run inside a literal that itself names `CapturedProcess` or `tree_kill` is exempt wherever
//!    it sits in this crate's census file -- including in a refusal message that happens to mention
//!    the record. That is the price of a role test a reader can check by eye, and it is one
//!    identifier away from the defect rather than one file away.
//! 2. This guard does NOT blank whitespace-valued literals (a literal that is only spaces) the way
//!    the workspace sweep does. Nothing in this crate carries one today (measured: 0 lines across
//!    37 files), so copying that helper here would have added a branch no cell exercises. The cost
//!    is stated rather than hidden: the day this crate wants a literal that is only spaces, this
//!    guard reds where the central sweep does not, and the remedy is to adopt that helper, never
//!    to widen the role test above.

use std::path::{Path, PathBuf};

include!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../tools/source-invariants/detect.rs"
));

/// The census file, spelled once, relative to the crate root and with forward slashes.
const CENSUS_FIXTURE_FILE: &str = "tests/cancellation_record_asymmetry.rs";

/// The identifiers that make a literal census INPUT rather than authored prose.
///
/// The census is about one record and one field. A fixture literal is Rust source handed to the
/// projection under test, so it names the record or the field by construction -- all forty of the
/// runs in that file do. Authored prose in the same file (the `OBSERVER_MISSING` refusals, the
/// harness messages) does not.
const CENSUS_SUBJECT: [&str; 2] = ["CapturedProcess", "tree_kill"];

/// Blank the BODY of every literal that is census input, keeping the quotes in place.
///
/// The quotes are kept for the same reason `without_whitespace_valued_literals` keeps them in the
/// workspace sweep: `has_run_in_literal` tracks whether it is inside a literal by counting quote
/// transitions, so removing a quote pair shifts the parity of the rest of the line and the guard
/// would then read code as string and string as code.
///
/// Escape pairs are consumed left to right while scanning for the closing quote, which is the same
/// correctness argument the shared predicate makes: splitting on a quote alone ends the literal at
/// an escaped one, and these fixtures are full of escaped quotes and newlines.
fn without_census_fixture_literals(line: &str) -> String {
    let characters: Vec<char> = line.chars().collect();
    let mut out = String::with_capacity(line.len());
    let mut index = 0;
    while index < characters.len() {
        if characters[index] != '"' {
            out.push(characters[index]);
            index += 1;
            continue;
        }
        let mut end = index + 1;
        let mut body = String::new();
        while end < characters.len() && characters[end] != '"' {
            if characters[end] == '\\' {
                body.push(characters[end]);
                end += 1;
                if end < characters.len() {
                    body.push(characters[end]);
                    end += 1;
                }
                continue;
            }
            body.push(characters[end]);
            end += 1;
        }
        if end >= characters.len() {
            // An unterminated literal on this line: leave the rest untouched rather than guess.
            out.extend(characters[index..].iter());
            break;
        }
        if CENSUS_SUBJECT.iter().any(|name| body.contains(name)) {
            out.push_str("\"\"");
        } else {
            out.push('"');
            out.push_str(&body);
            out.push('"');
        }
        index = end + 1;
    }
    out
}

/// One line's verdict, with the role exemption applied only inside the file that earns it.
fn offends(path: &str, line: &str) -> bool {
    if is_line_comment(line) {
        return false;
    }
    if path.replace('\\', "/") == CENSUS_FIXTURE_FILE {
        return has_run_in_literal(&without_census_fixture_literals(line));
    }
    has_run_in_literal(line)
}

/// Largest authored Rust file this guard will read, and the bound is on the READ rather than on a
/// number the directory entry reported. The census file is the biggest in this crate at 1807
/// lines; a megabyte is two orders of magnitude of headroom and still a ceiling.
const MAX_FILE_BYTES: u64 = 1 << 20;

/// Read one authored file with the READ itself bounded.
///
/// `metadata.len()` is a fact about the directory entry at the instant it was read, so checking it
/// and then calling `read_to_string` is a check followed by an operation that can invalidate it.
/// The bound therefore lives on the reader: `take(MAX + 1)` cannot allocate past the ceiling, and
/// the one extra byte is what distinguishes a file that EXCEEDED the ceiling from one that exactly
/// filled it.
fn read_bounded(path: &Path) -> Result<String, String> {
    use std::io::Read;

    let file = std::fs::File::open(path)
        .map_err(|error| format!("cannot open {}: {error}", path.display()))?;
    let mut buffer = Vec::new();
    file.take(MAX_FILE_BYTES + 1)
        .read_to_end(&mut buffer)
        .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
    if buffer.len() as u64 > MAX_FILE_BYTES {
        return Err(format!(
            "HARNESS-BROKE: authored Rust scan refuses an oversized file: {} exceeds {MAX_FILE_BYTES} bytes",
            path.display()
        ));
    }
    String::from_utf8(buffer).map_err(|error| format!("{} is not UTF-8: {error}", path.display()))
}

/// Every authored `.rs` file under the given roots, DERIVED by walking -- never a list.
///
/// Both of this crate's roots, because the defect this guard exists for lives under `tests/`.
///
/// PARAMETERISED over `base` and `roots`, and that is not generality for its own sake: every
/// refusal below is reachable only by a tree this crate's own checkout does not contain, so
/// without a planted root no cell could observe one. An unobserved refusal is indistinguishable
/// from an absent one. Refusals are RETURNED rather than panicked for the same reason -- a cell
/// reads the text and asserts on it; `authored_rust_files` turns one back into a panic at the top.
fn scan_rust_files(base: &Path, roots: &[PathBuf]) -> Result<Vec<(String, String)>, String> {
    const MAX_DEPTH: usize = 8;
    const MAX_ENTRIES: usize = 512;
    const MAX_FILES: usize = 128;

    fn walk(
        directory: &Path,
        depth: usize,
        entries_seen: &mut usize,
        files: &mut Vec<PathBuf>,
    ) -> Result<(), String> {
        if depth > MAX_DEPTH {
            return Err(format!(
                "HARNESS-BROKE: authored Rust scan exceeded depth {MAX_DEPTH}"
            ));
        }
        let entries = std::fs::read_dir(directory)
            .map_err(|error| format!("cannot read {}: {error}", directory.display()))?;
        for entry in entries {
            let entry = entry.map_err(|error| {
                format!(
                    "cannot read an entry under {}: {error}",
                    directory.display()
                )
            })?;
            *entries_seen += 1;
            if *entries_seen > MAX_ENTRIES {
                return Err(format!(
                    "HARNESS-BROKE: authored Rust scan exceeded {MAX_ENTRIES} directory entries"
                ));
            }
            let path = entry.path();
            let metadata = std::fs::symlink_metadata(&path)
                .map_err(|error| format!("cannot inspect {}: {error}", path.display()))?;
            if metadata.file_type().is_symlink() {
                return Err(format!(
                    "HARNESS-BROKE: authored Rust scan refuses a linked entry: {}",
                    path.display()
                ));
            }
            if metadata.is_dir() {
                walk(&path, depth + 1, entries_seen, files)?;
            } else if path.extension().is_some_and(|extension| extension == "rs") {
                // A REGULAR file, decided before anything opens it. A FIFO or a device node named
                // `*.rs` would otherwise be opened by the reader, and a FIFO with no writer blocks
                // the gate forever -- a hang has no colour.
                if !metadata.is_file() {
                    return Err(format!(
                        "HARNESS-BROKE: authored Rust scan refuses a non-regular file: {}",
                        path.display()
                    ));
                }
                files.push(path);
                if files.len() > MAX_FILES {
                    return Err(format!(
                        "HARNESS-BROKE: authored Rust scan exceeded {MAX_FILES} Rust files"
                    ));
                }
            }
        }
        Ok(())
    }

    let mut paths = Vec::new();
    let mut entries_seen = 0;
    for root in roots {
        // EACH STARTING ROOT IS INSPECTED BEFORE IT IS OPENED. A link met during the walk was
        // always refused, but a link handed in AS a root was followed, because `walk` opened the
        // directory before anything asked what the directory was. `strip_prefix(base)` cannot
        // catch that: the planted root IS the base, so every foreign path under it strips cleanly
        // and the scan reports another tree as this crate's own.
        let metadata = std::fs::symlink_metadata(root)
            .map_err(|error| format!("cannot inspect {}: {error}", root.display()))?;
        if metadata.file_type().is_symlink() {
            return Err(format!(
                "HARNESS-BROKE: authored Rust scan refuses a linked root: {}",
                root.display()
            ));
        }
        if !metadata.is_dir() {
            return Err(format!(
                "HARNESS-BROKE: authored Rust scan refuses a root that is not a directory: {}",
                root.display()
            ));
        }
        walk(root, 0, &mut entries_seen, &mut paths)?;
    }
    paths.sort();
    paths
        .into_iter()
        .map(|path| {
            let shown = path
                .strip_prefix(base)
                .map_err(|_| format!("{} escaped the scanned base", path.display()))?
                .display()
                .to_string()
                .replace('\\', "/");
            let source = read_bounded(&path)?;
            Ok((shown, source))
        })
        .collect()
}

/// This crate's own `src/` and `tests/`, which is the population the guard exists to judge.
fn authored_rust_files() -> Vec<(String, String)> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    scan_rust_files(root, &[root.join("src"), root.join("tests")])
        .unwrap_or_else(|error| panic!("{error}"))
}

/// THE COLLECTOR: scan, judge, and name each offender by path and line.
///
/// Parameterised for one reason, and it is the reason the positive control below exists: a
/// collector that can only ever be pointed at a tree containing no offenders is a collector whose
/// body can be replaced by `Vec::new()` without reddening anything.
fn offending_lines_under(base: &Path, roots: &[PathBuf]) -> Vec<String> {
    scan_rust_files(base, roots)
        .unwrap_or_else(|error| panic!("{error}"))
        .iter()
        .flat_map(|(path, source)| {
            source
                .lines()
                .enumerate()
                .filter(|(_, line)| offends(path, line))
                .map(|(number, line)| format!("{path}:{}: {}", number + 1, line.trim_start()))
                .collect::<Vec<_>>()
        })
        .collect()
}

fn offending_lines() -> Vec<String> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    offending_lines_under(root, &[root.join("src"), root.join("tests")])
}

#[test]
fn authored_strings_carry_no_collapsed_indentation() {
    let offenders = offending_lines();
    assert!(
        offenders.is_empty(),
        "these authored string literals contain collapsed indentation:\n{}",
        offenders.join("\n")
    );
}

/// A derived scan that finds nothing passes for every possible defect, so the derivation is
/// checked before it is trusted. The floor is the REAL count; lowering it is legitimate only
/// alongside a NAMED removal in the same change. The landmarks are the second half -- a count can
/// be met by the wrong files.
#[test]
fn the_scan_covers_src_and_tests() {
    let files = authored_rust_files();
    assert!(
        files.len() >= 37,
        "HARNESS-BROKE: the walk found {} authored Rust files; this crate has 37: {:?}",
        files.len(),
        files.iter().map(|(path, _)| path).collect::<Vec<_>>()
    );
    for required in [
        "src/lib.rs",
        CENSUS_FIXTURE_FILE,
        "tests/source_invariants.rs",
    ] {
        assert!(
            files.iter().any(|(path, _)| path == required),
            "HARNESS-BROKE: the walk must include {required}: {:?}",
            files.iter().map(|(path, _)| path).collect::<Vec<_>>()
        );
    }
}

/// The exemption must still be suppressing something, or it is a licence nobody is spending.
///
/// Same rule as `every_exemption_is_load_bearing` in the workspace sweep, applied one level down:
/// the day the census fixtures stop carrying runs, this cell reds and the exemption goes.
#[test]
fn the_census_fixture_exemption_is_load_bearing() {
    let census = authored_rust_files()
        .into_iter()
        .find(|(path, _)| path == CENSUS_FIXTURE_FILE)
        .expect("the census file is in the walk");
    let unexempted = census
        .1
        .lines()
        .filter(|line| !is_line_comment(line) && has_run_in_literal(line))
        .count();
    assert!(
        unexempted > 0,
        "the census-fixture exemption suppresses nothing in {CENSUS_FIXTURE_FILE}; delete it"
    );
}

/// The point of this whole file: the role exemption must NOT cover authored prose.
///
/// The decoy is the exact shape lane S planted on #1026 -- a ten-space run inside a refusal message
/// in the census file -- beside a real fixture line from the same file. Without this cell, an
/// exemption that accidentally swallowed the file would look identical to one that does not, which
/// is the state this crate was in before #1132.
///
/// **Every run below is BUILT, never typed.** Five of the nine entries in the workspace sweep's
/// EXEMPT list are guard files that had to be excused for quoting the defect they detect; a guard
/// that composes its samples with `" ".repeat(n)` needs no such excuse and stays inside its own
/// scan, which is the property this whole change is about. (The same trick core/execution's
/// `is_comment_filter_fixture` uses, taken one step further: there, the fixture is still typed.)
#[test]
fn the_role_exemption_spares_fixtures_and_still_reads_prose() {
    let run = " ".repeat(10);
    let lead = " ".repeat(8);
    let prose = format!(
        "{lead}panic!(\"OBSERVER_MISSING:{run}Rust parsing failed for production source\");"
    );
    assert!(
        offends(CENSUS_FIXTURE_FILE, &prose),
        "authored prose in {CENSUS_FIXTURE_FILE} must still be read: {prose}"
    );

    let indent = " ".repeat(4);
    let fixture = format!("{lead}\"{indent}let CapturedProcess {{ tree_kill, .. }} = captured;\",");
    assert!(
        has_run_in_literal(&fixture),
        "HARNESS-BROKE: the fixture control must carry a run, or it proves nothing"
    );
    assert!(
        !offends(CENSUS_FIXTURE_FILE, &fixture),
        "a census fixture literal must stay exempt: {fixture}"
    );

    // The exemption is scoped to ONE file: the same fixture text anywhere else is a defect.
    assert!(
        offends("src/process.rs", &fixture),
        "the role exemption must not travel outside {CENSUS_FIXTURE_FILE}"
    );

    // Prose and fixture on ONE line: the line-level exemptions the other twelve guards use would
    // lose this case, and it is why this one blanks per literal. The fixture literal here carries
    // a two-space indent, below the predicate's threshold, so the only run on the line is the one
    // inside the PROSE literal -- a pass here cannot be earned by the fixture half.
    let mixed = format!(
        "{lead}assert_eq!(rendered, \"  let CapturedProcess {{ tree_kill, .. }} = c;\", \
         \"went{run}wrong\");"
    );
    assert!(
        offends(CENSUS_FIXTURE_FILE, &mixed),
        "a prose literal beside a fixture literal must still be read: {mixed}"
    );
}

/// Plant a directory link, and name what the platform would not let us plant.
///
/// Windows: a JUNCTION, because an unprivileged process on this machine cannot create a directory
/// symlink (measured: `mklink /D` answers "You do not have sufficient privilege"). A junction is a
/// reparse point and `FileType::is_symlink` reports it, which is the property under test -- the
/// guard's question is "does this entry redirect the walk elsewhere", not "which reparse tag".
#[cfg(windows)]
fn plant_directory_link(link: &Path, target: &Path) -> bool {
    std::process::Command::new("cmd")
        .arg("/C")
        .arg("mklink")
        .arg("/J")
        .arg(link)
        .arg(target)
        .output()
        .is_ok_and(|output| output.status.success())
}

#[cfg(unix)]
fn plant_directory_link(link: &Path, target: &Path) -> bool {
    std::os::unix::fs::symlink(target, link).is_ok()
}

/// ROOT 1. The starting root is inspected BEFORE it is opened.
///
/// A link reached during the walk was already refused; a link handed in AS a root was not, because
/// `walk` opened the directory before anything asked what the directory was. The lexical
/// `strip_prefix(base)` cannot catch it: the planted root IS the base, so every path under it
/// strips cleanly and the scan reports a foreign tree as this crate's own.
#[test]
fn a_linked_starting_root_is_refused() {
    let bench = tempfile::tempdir().expect("a temporary directory");
    let elsewhere = bench.path().join("elsewhere");
    std::fs::create_dir(&elsewhere).expect("the target directory");
    std::fs::write(elsewhere.join("foreign.rs"), "fn foreign() {}\n").expect("a file to be found");

    let linked_root = bench.path().join("linked_root");
    assert!(
        plant_directory_link(&linked_root, &elsewhere),
        "HARNESS-BROKE: could not plant a directory link, so this cell proves nothing"
    );
    assert!(
        std::fs::symlink_metadata(&linked_root)
            .expect("the planted link is inspectable")
            .file_type()
            .is_symlink(),
        "HARNESS-BROKE: the planted link does not read as a link, so this cell proves nothing"
    );

    let refusal = scan_rust_files(&linked_root, std::slice::from_ref(&linked_root))
        .expect_err("a linked starting root must be refused");
    assert!(
        refusal.contains("refuses a linked root"),
        "the refusal must name the linked ROOT: {refusal}"
    );
}

/// ROOT 2a. The read is bounded, so one enormous file cannot allocate without limit.
#[test]
fn an_oversized_file_is_refused() {
    let bench = tempfile::tempdir().expect("a temporary directory");
    let root = bench.path().join("src");
    std::fs::create_dir(&root).expect("the scanned root");
    let oversized = vec![b'\n'; (MAX_FILE_BYTES + 1) as usize];
    std::fs::write(root.join("huge.rs"), &oversized).expect("an oversized file");

    // The outcome is matched rather than `expect_err`ed: the Ok arm carries the file's whole
    // contents, and a megabyte of newlines in a panic message is a failure nobody can read.
    let refusal = match scan_rust_files(bench.path(), &[root]) {
        Err(refusal) => refusal,
        Ok(files) => panic!(
            "a file past the {MAX_FILE_BYTES}-byte ceiling must be refused; the scan accepted {} file(s)",
            files.len()
        ),
    };
    assert!(
        refusal.contains("refuses an oversized file"),
        "the refusal must name the SIZE ceiling: {refusal}"
    );
}

/// ROOT 2b. Only a REGULAR file is read, so a special file cannot block the gate.
///
/// Unix only, and the reason is a measurement rather than a preference: Windows has no filesystem
/// object a test can plant in a directory that is neither a regular file nor a directory nor a
/// reparse point -- a named pipe there lives in `\.\pipe\`, outside any directory the walk sees.
/// The bound above is the control that DOES run on both platforms. Stated rather than hidden: on
/// Windows this refusal is carried by code review and by the Unix run of this cell, not by a
/// local red.
#[cfg(unix)]
#[test]
fn a_non_regular_file_is_refused() {
    let bench = tempfile::tempdir().expect("a temporary directory");
    let root = bench.path().join("src");
    std::fs::create_dir(&root).expect("the scanned root");
    let fifo = root.join("blocking.rs");
    let name = std::ffi::CString::new(fifo.to_str().expect("a UTF-8 path")).expect("a C string");
    assert_eq!(
        unsafe { libc::mkfifo(name.as_ptr(), 0o644) },
        0,
        "HARNESS-BROKE: could not plant a FIFO, so this cell proves nothing"
    );

    let refusal =
        scan_rust_files(bench.path(), &[root]).expect_err("a non-regular file must be refused");
    assert!(
        refusal.contains("refuses a non-regular file"),
        "the refusal must name the FILE KIND: {refusal}"
    );
}

/// ROOT 3. THE POSITIVE CONTROL: a real offending file, driven through the real collector.
///
/// Every other cell here asks the predicate a question directly, so all of them stay green when
/// `offending_lines_under` returns `Vec::new()` -- the collector could be gutted and the suite
/// would applaud. This cell is the one that reds: it plants a clean file and a bad one, runs the
/// collector over them, and asserts the DIAGNOSTIC names the offending path and the offending
/// line number. Sabotage measured, not asserted: with the body of `offending_lines_under`
/// replaced by `Vec::new()`, this cell fails on the emptiness assertion below and the rest of the
/// file stays green.
///
/// The run is BUILT with `" ".repeat`, for the same reason every other sample in this file is: a
/// typed run would make this guard file its own offender.
#[test]
fn the_collector_names_the_offending_file_and_line() {
    let bench = tempfile::tempdir().expect("a temporary directory");
    let root = bench.path().join("src");
    std::fs::create_dir(&root).expect("the scanned root");

    // BOTH fixtures are BUILT, and the red that produced this shape was this guard
    // reading its OWN source: a typed four-space indent inside these samples is a run in a
    // literal, so the first version of this cell made this file its own offender. `indent`
    // sits above the predicate's three-space threshold in the bad file, and the clean file
    // carries no run at all.
    let run = " ".repeat(10);
    let indent = " ".repeat(4);
    let clean = format!("fn clean() {{\n{indent}let message = \"one space\";\n}}\n");
    let bad = format!("fn bad() {{\n{indent}let message = \"went{run}wrong\";\n}}\n");
    assert!(
        !has_run_in_literal(
            clean
                .lines()
                .nth(1)
                .expect("the clean file has a second line")
        ),
        "HARNESS-BROKE: the clean control must carry no run, or it discriminates nothing"
    );
    assert!(
        has_run_in_literal(bad.lines().nth(1).expect("the bad file has a second line")),
        "HARNESS-BROKE: the bad control must carry a run, or it proves nothing"
    );
    std::fs::write(root.join("clean.rs"), &clean).expect("a clean file");
    std::fs::write(root.join("bad.rs"), &bad).expect("an offending file");

    let offenders = offending_lines_under(bench.path(), &[root]);
    assert_eq!(
        offenders.len(),
        1,
        "exactly the planted offender must be reported: {offenders:?}"
    );
    let reported = &offenders[0];
    assert!(
        reported.starts_with("src/bad.rs:2:"),
        "the diagnostic must name the offending PATH and LINE: {reported}"
    );
    assert!(
        !offenders.iter().any(|entry| entry.contains("clean.rs")),
        "the clean file must not be reported: {offenders:?}"
    );
}
