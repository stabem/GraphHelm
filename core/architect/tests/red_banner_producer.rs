//! #1153: the banner has ONE producer, and that assumption ages against the tree rather than
//! against anyone's memory of it.
//!
//! `judgment::red::excerpt` lets `[gate] RED - failed stages:` decide `failed_stages` and keeps
//! `[gate] FAILED: <stage>` only as the fallback for a run that dies before any verdict line.
//! **The whole justification is an asymmetry in who writes the two strings**: the banner is
//! emitted once per run by `ci/gate.ps1`; `[gate] FAILED:` is printed by many things, the gate's
//! own self-test fixtures included, which is the contamination #1149 measured.
//!
//! Nothing in the module asserts that asymmetry (`unruffled-babbage-df857c-1d` on #1153, who
//! checked it by hand and pointed out that it is the load-bearing assumption). A comment would
//! record it; this fails when it stops being true.
//!
//! **This is an INVENTORY guard, not a producer detector, and the difference is deliberate.**
//! Deciding lexically which line "emits" rather than "asserts" is exactly the kind of
//! spelling-sensitive rule that a new `Write-Host` form walks past in silence. So the cell names
//! every file in `ci/` that mentions the string at all, with its role, and fails on ANY change to
//! that set. A new file forces a person to classify it as producer or assertion; there is no
//! spelling that evades a test which trips on the mention itself.

use std::path::{Path, PathBuf};

/// The literal the excerpt reads. Kept in sync with `judgment::red`'s own constant by
/// `the_guard_and_the_module_read_the_same_string`.
const RED_BANNER: &str = "[gate] RED - failed stages:";

/// What the SCAN looks for, and it is deliberately WIDER than the literal above.
///
/// `ci/gate-stage-reddens.tests.ps1` matches `'RED - failed stages: .*ci powershell suites'` —
/// the tail without the `[gate] ` prefix. Scanning for the full literal misses it, which is how
/// the first version of this guard reported two files where `grep` reported three: the
/// instrument's unit was not the question's unit, and the narrower scan would have let a new
/// near-form producer in unseen. An inventory guard wants the widest string that still means
/// this banner.
const BANNER_TAIL: &str = "RED - failed stages:";

/// Every `ci/` file that mentions the banner, and why each is or is not a producer. Measured
/// 2026-09-18 at `516e2a34`.
const KNOWN_MENTIONS: [(&str, &str); 3] = [
    (
        "ci/gate.ps1",
        "THE PRODUCER: `$lines.Add(\"[gate] RED - failed stages: ...\")` builds the verdict block",
    ),
    (
        "ci/gate-verdict.tests.ps1",
        "asserts on the wording and on its absence; prints nothing into a gate log",
    ),
    (
        "ci/gate-stage-reddens.tests.ps1",
        "matches the TAIL only, no `[gate] ` prefix, in a captured slice; prints nothing into a gate log",
    ),
];

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("core/architect sits two levels below the workspace root")
        .to_path_buf()
}

/// Every file directly under `ci/` that contains the banner literal, as repo-relative paths.
fn mentions(root: &Path) -> Vec<String> {
    let ci = root.join("ci");
    let mut found = Vec::new();
    let entries =
        std::fs::read_dir(&ci).unwrap_or_else(|error| panic!("ci/ must be readable: {error}"));
    for entry in entries {
        let entry = entry.expect("a ci/ entry must be inspectable");
        if !entry.file_type().expect("file type").is_file() {
            continue;
        }
        let path = entry.path();
        // Read as bytes: a PowerShell file written by a redirect can be UTF-16, and a lossy
        // decode of one answers every search with nothing -- the clean-looking zero this
        // repository has now been bitten by three times in a day.
        let bytes = std::fs::read(&path).expect("a ci/ file must be readable");
        let text = String::from_utf8_lossy(&bytes);
        let utf16 = bytes.starts_with(&[0xFF, 0xFE]) || bytes.starts_with(&[0xFE, 0xFF]);
        assert!(
            !utf16,
            "{path:?} is UTF-16; this scan would answer zero for it without reading a byte of \
             its content"
        );
        if text.contains(BANNER_TAIL) {
            found.push(format!("ci/{}", entry.file_name().to_string_lossy()));
        }
    }
    found.sort();
    found
}

/// The set of files that mention the banner is exactly the recorded set.
///
/// A new member fails this cell and forces the question the excerpt depends on: does it PRINT the
/// banner into a gate log, or only read one? If it prints, `excerpt` no longer has a single
/// authority for `failed_stages` and the fallback rule needs revisiting. If it reads, add it here
/// with its role.
#[test]
fn the_red_banner_has_exactly_one_producer_and_two_readers() {
    let root = workspace_root();
    let found = mentions(&root);
    let mut expected: Vec<String> = KNOWN_MENTIONS
        .iter()
        .map(|(path, _)| (*path).to_owned())
        .collect();
    expected.sort();
    assert_eq!(
        found, expected,
        "the set of ci/ files naming the banner changed; classify the difference as PRODUCER or \
         READER and update KNOWN_MENTIONS with its role. Roles today: {KNOWN_MENTIONS:?}"
    );
}

/// The scan really can find the string, so the equality above is a statement about the tree and
/// not about a reader that opened nothing.
///
/// Without this, a `mentions` that returned an empty vector would fail the cell above with a
/// message about a missing file rather than about a broken scan — and a `KNOWN_MENTIONS` someone
/// emptied "to make it pass" would then be green forever.
#[test]
fn the_scan_reads_ci_and_finds_the_producer() {
    let root = workspace_root();
    let gate = root.join("ci/gate.ps1");
    let text = std::fs::read_to_string(&gate).expect("ci/gate.ps1 must be readable");
    assert!(
        text.contains(RED_BANNER),
        "the producer must carry the FULL literal, prefix included, or every claim here is vacuous"
    );
    assert!(
        !mentions(&root).is_empty(),
        "the scan must find at least the producer"
    );
}

/// The literal this guard pins is the literal the module reads. Two copies of a string is how a
/// guard drifts off its subject while staying green.
#[test]
fn the_guard_and_the_module_read_the_same_string() {
    let root = workspace_root();
    let module = root.join("core/architect/src/judgment/red.rs");
    let text = std::fs::read_to_string(&module).expect("red.rs must be readable");
    assert!(
        text.contains(&format!("const RED_BANNER: &str = \"{RED_BANNER}\";")),
        "red.rs no longer declares this exact banner constant, so this guard is pinning a string \
         nothing reads"
    );
    // And the wide scan must still be part of the literal the module reads. Two constants is how
    // a guard drifts off its subject while staying green; this ties them.
    assert!(
        RED_BANNER.contains(BANNER_TAIL),
        "the scan string is no longer part of the banner, so it is scanning for something else"
    );
}
