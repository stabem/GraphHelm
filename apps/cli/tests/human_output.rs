//! #1150: the human summary on stderr must cost stdout nothing, and must never carry the key.
//!
//! `AGENTS.md:49` keeps `apps/cli` at JSON presentation only, and 26 files in this directory parse
//! `output.stdout` as JSON across 47 cells. The summary this suite guards goes to a DIFFERENT
//! stream and only when stdout is a terminal, so those cells must all still see what they saw.
//!
//! **The harness cannot be a terminal, and that is not a gap in this suite — it is the guard.**
//! `assert_cmd` spawns the binary with pipes, which is exactly the condition under which the
//! summary must not appear at all. So these cells prove the NEGATIVE half directly: piped stdout
//! is still the single JSON document, byte for byte, and piped stderr is EMPTY. The
//! POSITIVE half (what the text actually says, and that it prints only the fields it names) is
//! held in `apps/cli/src/human.rs`'s own unit cells, which call the renderer in process.
//!
//! The sentinel sweep runs the full `gateway setup` journey with a distinctive key on stdin and
//! reads the RAW BYTES of stdout. A sweep that finds nothing proves nothing on its own — a search
//! that silently matched nothing would pass it — so the same assertion carries a control string
//! that IS present, planted as the route id.
//!
//! **Stderr is not swept, it is required to be EMPTY**, and the difference is the review finding
//! that produced this paragraph (`eloquent-jones` on #1152). A sentinel search over a stream
//! nothing writes to cannot fail for the reason it names; a length can. See `assert_no_stderr`.
//! The risk that search was aimed at — a key reaching the rendered summary — is held where the
//! summary actually exists, by `human.rs`'s `a_field_the_renderer_does_not_name_is_not_printed`,
//! which plants a secret-shaped field and carries its own control in the same population.

use std::path::Path;

use assert_cmd::Command;
use serde_json::Value;

/// The key planted on stdin. It must appear in neither stream, on neither the success nor the
/// refusal path.
const SENTINEL: &str = "sk-SENTINEL-1150-0123456789abcdef";
/// The key the rotation run pastes over the first one.
const SECOND_SENTINEL: &str = "sk-SENTINEL-1150-SECOND-fedcba9876";
/// The control for the same sweep: a string seeded into the same run that MUST be found, so a
/// search that matches nothing cannot report the sentinel as absent.
const CONTROL_ROUTE: &str = "control_1150_judge";

fn command() -> Command {
    Command::new(assert_cmd::cargo::cargo_bin!("graphhelm"))
}

fn combined(output: &std::process::Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn git_project() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join(".git")).unwrap();
    dir
}

fn contains(haystack: &[u8], needle: &str) -> bool {
    haystack
        .windows(needle.len())
        .any(|window| window == needle.as_bytes())
}

/// The contract stdout has always carried, asserted on BYTES: one compact JSON document, one
/// trailing `\n`, and no byte OUTSIDE it.
///
/// **Outside is the whole of what these three assertions check, and the earlier wording claimed
/// more** (`graphhelm-pr-1099-agent-4a311e` on #1152): a summary written *inside* the envelope —
/// into `data`, say — parses whole-slice, carries exactly one newline, and ends with it, so it
/// satisfies all three. What discriminates against that is the five-phrase list in
/// `assert_no_summary`, not this function. Both are needed and neither subsumes the other.
///
/// `from_slice` over the WHOLE slice refuses any prefix or suffix that is not part of the
/// document, so no summary line can hide before or after it; exactly one newline, in last
/// position, refuses a second line and refuses `--pretty` output where none was asked for.
///
/// **What this does NOT do, stated because the difference matters**: it does not diff against a
/// binary built before #1150. Re-serializing the parsed value and comparing would look like that
/// and is not — `serde_json` without `preserve_order` sorts the keys, so the comparison would fail
/// on today's own output and prove nothing. The claim held here is the one a byte diff would also
/// have to show: stdout carries the document, and no byte that is not part of it.
fn assert_stdout_is_the_json_contract(output: &std::process::Output) -> Value {
    let stdout = &output.stdout;
    assert_eq!(
        stdout.last(),
        Some(&b'\n'),
        "stdout must end with one newline: {:?}",
        String::from_utf8_lossy(stdout)
    );
    assert_eq!(
        stdout.iter().filter(|byte| **byte == b'\n').count(),
        1,
        "stdout must be one line: {:?}",
        String::from_utf8_lossy(stdout)
    );
    serde_json::from_slice(stdout).unwrap_or_else(|error| {
        panic!(
            "all of stdout must be one JSON document ({error}): {:?}",
            String::from_utf8_lossy(stdout)
        )
    })
}

/// Nothing the renderer writes may reach a piped run at all.
///
/// **The stderr half is stated as EMPTINESS, not as an absence of phrases, and that is the whole
/// point of this function** (`eloquent-jones` on #1152). Searching stderr for the summary's own
/// phrases looks like the mirror of the stdout search and is INERT: under a piped run
/// `print_human_summary` returns early on `!stdout().is_terminal()`, so nothing is written to
/// stderr at all, and an assertion that a never-written string is absent cannot fail for the
/// reason it names. A search that silently matched nothing would satisfy it identically.
///
/// `stderr.is_empty()` has no such hole. It is strictly stronger — it subsumes every phrase, every
/// sentinel and everything nobody thought to name — and it FAILS the moment the terminal gate is
/// defeated, which is exactly the regression this file exists to catch. It needs no control
/// because there is no search to be fooled: the claim is about a length.
fn assert_no_stderr(output: &std::process::Output) {
    assert!(
        output.stderr.is_empty(),
        "stdout is a pipe here, so the run must write NOTHING to stderr; it wrote {} bytes: {:?}",
        output.stderr.len(),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn assert_no_summary(output: &std::process::Output) {
    assert_no_stderr(output);
    // stdout is a different claim: it carries the JSON document, so emptiness is not available and
    // the summary's own phrases are what distinguish it. `assert_stdout_is_the_json_contract`
    // already refuses any byte outside the document; this names the phrases so a failure says
    // which surface leaked rather than only that the parse broke.
    let seen = String::from_utf8_lossy(&output.stdout);
    for phrase in [
        "Next, in this shell:",
        "is ready.",
        "is initialized.",
        "did not run.",
        "it placed no model call",
    ] {
        assert!(
            !seen.contains(phrase),
            "no summary may be written to stdout: {seen}"
        );
    }
}

fn init(project: &Path) -> std::process::Output {
    command()
        .args(["init", "--project"])
        .arg(project)
        .args(["--harness", "claude-code", "--harness", "codex"])
        .output()
        .unwrap()
}

fn setup(project: &Path, extra: &[&str], stdin: &str) -> std::process::Output {
    command()
        .args(["gateway", "setup", "--provider", "typesafe", "--project"])
        .arg(project)
        .args(extra)
        .write_stdin(stdin)
        .output()
        .unwrap()
}

// ---------------------------------------------------------------------------
// Cell 1: the three commands that gained a renderer still publish exactly the JSON contract on
// stdout when stdout is a pipe, and write no summary anywhere.
// ---------------------------------------------------------------------------

#[test]
fn piped_stdout_is_exactly_the_json_contract_and_carries_no_summary() {
    let project = git_project();

    let initialized = init(project.path());
    assert!(initialized.status.success(), "{}", combined(&initialized));
    let value = assert_stdout_is_the_json_contract(&initialized);
    assert_eq!(value["command"], "init");
    assert_eq!(value["ok"], true);
    assert_no_summary(&initialized);

    let wired = setup(project.path(), &[], &format!("{SENTINEL}\n"));
    assert!(wired.status.success(), "{}", combined(&wired));
    let value = assert_stdout_is_the_json_contract(&wired);
    assert_eq!(value["command"], "gateway.setup");
    assert_eq!(value["data"]["probe"]["health"], "available", "{value}");
    assert_no_summary(&wired);

    let key = std::fs::read_to_string(project.path().join(".graphhelm").join("serve.key")).unwrap();
    let probed = command()
        .args(["gateway", "probe", "--manifest"])
        .arg(project.path().join(".graphhelm").join("manifest.json"))
        .args(["--route", "judge", "--broker"])
        .arg(project.path().join(".graphhelm").join("broker"))
        .arg("--keyring")
        .arg(project.path().join(".graphhelm").join("keyring"))
        .args(["--key-id", "studio"])
        .env("GRAPHHELM_GATEWAY_KEY", key.trim())
        .output()
        .unwrap();
    assert!(probed.status.success(), "{}", combined(&probed));
    let value = assert_stdout_is_the_json_contract(&probed);
    assert_eq!(value["command"], "gateway.probe");
    assert_eq!(value["data"]["health"], "available", "{value}");
    assert_no_summary(&probed);
}

// ---------------------------------------------------------------------------
// Cell 2: a refusal keeps the same property. The renderer's failure shape is the one a person is
// most likely to meet, and it must not push a word onto stdout either.
// ---------------------------------------------------------------------------

#[test]
fn a_refusal_also_leaves_stdout_exactly_the_json_contract() {
    let project = git_project();
    assert!(init(project.path()).status.success());

    let refused = setup(project.path(), &[], "\n");
    assert!(!refused.status.success());
    let value = assert_stdout_is_the_json_contract(&refused);
    assert_eq!(value["ok"], false);
    assert_eq!(value["diagnostics"][0]["path"], "/stdin", "{value}");
    assert_no_summary(&refused);
}

// ---------------------------------------------------------------------------
// Cell 3: the key reaches neither stream. Raw bytes, both streams, with a control that IS found.
// ---------------------------------------------------------------------------

#[test]
fn the_key_reaches_neither_stdout_nor_stderr_and_the_sweep_can_find_what_is_there() {
    let project = git_project();
    assert!(init(project.path()).status.success());

    let output = setup(
        project.path(),
        &["--route-id", CONTROL_ROUTE],
        &format!("{SENTINEL}\n"),
    );
    assert!(output.status.success(), "{}", combined(&output));

    // The control first: this run really did seed a findable string into these bytes, so the two
    // absences below are a statement about the key and not about the search.
    assert!(
        contains(&output.stdout, CONTROL_ROUTE),
        "the control must be found, or this sweep proves nothing: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    assert!(
        !contains(&output.stdout, SENTINEL),
        "the key must not reach stdout"
    );
    // STDERR IS CLAIMED AS EMPTY, not as "the sentinel is absent from it" (`eloquent-jones` on
    // #1152). Under `assert_cmd` the child's stdout is a pipe, so no summary is written to stderr
    // at all — a sentinel absence there could not fail for the reason it names, and would look
    // identical to a search that matched nothing. Emptiness needs no control and says more.
    assert_no_stderr(&output);

    // The rotation path, where a SECOND key is read over the first: both must stay out of both
    // streams. A second run without `--replace` is refused BEFORE the key is read, so it would
    // prove nothing about a key — this one reads one.
    let rotated = setup(
        project.path(),
        &["--route-id", CONTROL_ROUTE, "--replace"],
        &format!("{SECOND_SENTINEL}\n"),
    );
    assert!(rotated.status.success(), "{}", combined(&rotated));
    assert!(
        contains(&rotated.stdout, CONTROL_ROUTE),
        "control: {}",
        String::from_utf8_lossy(&rotated.stdout)
    );
    assert_no_stderr(&rotated);
    for planted in [SENTINEL, SECOND_SENTINEL] {
        assert!(
            !contains(&rotated.stdout, planted),
            "{planted} reached stdout"
        );
        assert!(
            !contains(&rotated.stderr, planted),
            "{planted} reached stderr: {}",
            String::from_utf8_lossy(&rotated.stderr)
        );
    }
}

// ---------------------------------------------------------------------------
// #1172: the terminal face, proven from the side the harness can occupy.
// ---------------------------------------------------------------------------

/// Whether the stream carries an ANSI escape introducer.
///
/// THE DETECTOR HAS ITS CONTROL ELSEWHERE, and it is named here so this negative is not read as
/// stronger than it is: `palette::tests::a_plain_palette_writes_no_escape_and_a_coloured_one_does`
/// asserts that a coloured palette DOES produce `\x1b`, so a byte search that could never match is
/// not what these cells are passing on.
fn carries_an_escape(bytes: &[u8]) -> bool {
    bytes.contains(&0x1b)
}

/// A piped run writes no escape byte, and `CLICOLOR_FORCE` cannot change that.
///
/// The variable forces colour where a palette is chosen; without a terminal no palette is chosen
/// at all, because the face is the JSON contract and the summary is not written. A run that
/// honoured the variable here would be a run that put decoration inside a document every other
/// test in this directory parses.
#[test]
fn a_piped_run_writes_no_escape_byte_even_with_clicolor_force() {
    let project = git_project();
    let initialized = command()
        .args(["init", "--project"])
        .arg(project.path())
        .env("CLICOLOR_FORCE", "1")
        .output()
        .unwrap();
    assert!(initialized.status.success(), "{}", combined(&initialized));
    assert!(
        !carries_an_escape(&initialized.stdout),
        "stdout carried an escape: {:?}",
        String::from_utf8_lossy(&initialized.stdout)
    );
    assert!(
        !carries_an_escape(&initialized.stderr),
        "stderr carried an escape: {:?}",
        String::from_utf8_lossy(&initialized.stderr)
    );
    assert_stdout_is_the_json_contract(&initialized);
}

/// `--json` is inert without a terminal: the same command produces the same stdout bytes with and
/// without it. The flag exists to force the machine face AT a terminal, and a flag that also
/// changed the piped answer would be a second contract.
#[test]
fn the_json_flag_is_inert_without_a_terminal() {
    let project = git_project();
    let missing = project.path().join("no-such-manifest.json");
    let probe = |extra: &[&str]| {
        command()
            .args(["gateway", "probe", "--manifest"])
            .arg(&missing)
            .args(["--route", "judge"])
            .args(extra)
            .output()
            .unwrap()
    };

    let plain = probe(&[]);
    let forced = probe(&["--json"]);
    assert_eq!(
        plain.status.code(),
        forced.status.code(),
        "the flag changed the exit code"
    );
    assert_eq!(
        String::from_utf8_lossy(&plain.stdout),
        String::from_utf8_lossy(&forced.stdout),
        "the flag changed piped stdout"
    );
    assert!(
        !plain.status.success(),
        "a missing manifest must refuse, or this cell compares two successes: {}",
        combined(&plain)
    );
    assert_no_stderr(&plain);
    assert_no_stderr(&forced);
}

/// Every root command keeps the structured argument-error contract when a global presentation
/// flag appears before or after it. These four placements cover both faces and both command
/// families that previously fell through to clap's raw stderr error.
#[test]
fn malformed_global_flag_invocations_keep_one_json_envelope_for_all_command_families() {
    for arguments in [
        vec!["--json", "gateway", "setup", "--bad"],
        vec!["gateway", "--json", "setup", "--bad"],
        vec!["--pretty", "init", "--bad"],
        vec!["init", "--pretty", "--bad"],
    ] {
        let output = command().args(arguments).output().unwrap();
        assert_eq!(output.status.code(), Some(2));
        assert_no_stderr(&output);
        let value: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(value["ok"], false);
        assert!(matches!(
            value["command"].as_str(),
            Some("gateway" | "init")
        ));
        assert_eq!(value["diagnostics"][0]["code"], "GHCLI001_ARGUMENT_INVALID");
    }
}
