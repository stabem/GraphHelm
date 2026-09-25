//! M06 Task 7, binding decision 5: gate definitions freeze before implementation — a
//! synthetic PR diff touching both a gate definition and gated code is REFUSED, and
//! either side alone is clean.

use graphhelm_quality::{GateManifestChange, freeze_violation, freeze_violation_with_lockfile};

const BASE_LOCK: &str = r#"# generated
version = 4

[[package]]
name = "graphhelm-policy"
version = "0.1.0"

[[package]]
name = "pathogens"
version = "0.1.0"
dependencies = [
 "filetime",
 "serde",
]

[[package]]
name = "serde_yaml_ng"
version = "0.10.0"
checksum = "stable"
"#;

const KEEL_LOCK: &str = r#"# generated
version = 4

[[package]]
name = "graphhelm-policy"
version = "0.1.0"

[[package]]
name = "pathogens"
version = "0.1.0"
dependencies = [
 "filetime",
 "graphhelm-policy",
 "serde",
 "serde_yaml_ng",
]

[[package]]
name = "serde_yaml_ng"
version = "0.10.0"
checksum = "stable"
"#;

const BASE_MANIFEST: &str = r#"[package]
name = "pathogens"
version = "0.1.0"

[dependencies]
serde = { workspace = true }
"#;

const KEEL_MANIFEST: &str = r#"[package]
name = "pathogens"
version = "0.1.0"

[dependencies]
graphhelm-policy = { path = "../../core/policy" }
serde = { workspace = true }
serde_yaml_ng = { workspace = true }
"#;

fn keel_paths() -> [&'static str; 2] {
    ["tools/pathogens/Cargo.toml", "Cargo.lock"]
}

fn keel_manifest() -> GateManifestChange<'static> {
    GateManifestChange {
        path: "tools/pathogens/Cargo.toml",
        base: BASE_MANIFEST,
        current: KEEL_MANIFEST,
    }
}

#[test]
fn a_diff_touching_gate_and_gated_together_is_refused() {
    let mixed = [
        "core/quality/src/lib.rs",
        "apps/cli/src/commands/serve/monitor.rs",
    ];
    let violation = freeze_violation(&mixed).expect("the mixed diff must be refused");
    assert!(violation.0.starts_with("core/quality/"));
    assert!(violation.1.starts_with("apps/cli/"));

    let gates_only = ["tools/pathogens/src/lib.rs", "core/quality/tests/thymus.rs"];
    assert!(
        freeze_violation(&gates_only).is_none(),
        "evolving the gates alone is legitimate"
    );

    let code_only = ["apps/cli/src/commands/serve/monitor.rs", "CHANGELOG.md"];
    assert!(
        freeze_violation(&code_only).is_none(),
        "evolving the code alone is legitimate"
    );
}

#[test]
fn a_keel_dependency_only_lock_delta_is_neutral() {
    let paths = keel_paths();
    let borrowed = paths.map(|path| path);
    assert_eq!(
        freeze_violation_with_lockfile(&borrowed, BASE_LOCK, KEEL_LOCK, &[keel_manifest()]),
        None,
        "the existing graphhelm-policy and serde_yaml_ng lock edges are explained by the changed gate manifest"
    );
}

#[test]
fn a_reversed_lock_dependency_delta_remains_a_freeze_violation() {
    let paths = keel_paths();
    assert!(
        freeze_violation_with_lockfile(&paths, KEEL_LOCK, BASE_LOCK, &[keel_manifest()]).is_some(),
        "a lockfile delta in the reverse direction cannot be explained by the manifest additions"
    );
}

#[test]
fn a_lockfile_version_change_remains_a_freeze_violation() {
    let changed = KEEL_LOCK.replace("version = \"0.1.0\"", "version = \"0.1.1\"");
    let paths = keel_paths();
    assert!(
        freeze_violation_with_lockfile(&paths, BASE_LOCK, &changed, &[keel_manifest()]).is_some()
    );
}

#[test]
fn a_lockfile_checksum_change_remains_a_freeze_violation() {
    let changed = KEEL_LOCK.replace("checksum = \"stable\"", "checksum = \"tampered\"");
    let paths = keel_paths();
    assert!(
        freeze_violation_with_lockfile(&paths, BASE_LOCK, &changed, &[keel_manifest()]).is_some()
    );
}

#[test]
fn a_new_lock_package_remains_a_freeze_violation() {
    let changed = format!("{KEEL_LOCK}\n[[package]]\nname = \"unrelated\"\nversion = \"1\"\n");
    let paths = keel_paths();
    assert!(
        freeze_violation_with_lockfile(&paths, BASE_LOCK, &changed, &[keel_manifest()]).is_some()
    );
}

#[test]
fn a_lock_edge_not_named_by_the_gate_manifest_remains_a_freeze_violation() {
    let changed = KEEL_LOCK.replace(" \"serde_yaml_ng\",", " \"serde_json\",");
    let paths = keel_paths();
    assert!(
        freeze_violation_with_lockfile(&paths, BASE_LOCK, &changed, &[keel_manifest()]).is_some()
    );
}

#[test]
fn an_existing_non_gate_dependency_version_change_remains_a_freeze_violation() {
    let base = BASE_LOCK.replace(
        "name = \"graphhelm-policy\"\nversion = \"0.1.0\"",
        "name = \"graphhelm-policy\"\nversion = \"0.1.0\"\ndependencies = [\"sha2 0.10.9\"]",
    );
    let changed = KEEL_LOCK.replace(
        "name = \"graphhelm-policy\"\nversion = \"0.1.0\"",
        "name = \"graphhelm-policy\"\nversion = \"0.1.0\"\ndependencies = [\"sha2 0.11.0\"]",
    );
    assert!(
        freeze_violation_with_lockfile(&keel_paths(), &base, &changed, &[keel_manifest()])
            .is_some(),
        "a non-gate package cannot switch between two existing dependency versions"
    );
}

#[test]
fn malformed_dependency_lists_remain_freeze_violations() {
    for malformed in [
        KEEL_LOCK.replace(" \"filetime\",", " \"filetime\",, "),
        KEEL_LOCK.replace(
            "dependencies = [",
            "dependencies = [\n  \"serde\",\n]\ndependencies = [",
        ),
    ] {
        assert!(
            freeze_violation_with_lockfile(
                &keel_paths(),
                BASE_LOCK,
                &malformed,
                &[keel_manifest()]
            )
            .is_some(),
            "malformed lockfile grammar cannot qualify the gate exception"
        );
    }
}

#[test]
fn malformed_or_concurrent_lockfile_changes_remain_a_freeze_violation() {
    let paths = keel_paths();
    assert!(
        freeze_violation_with_lockfile(
            &paths,
            BASE_LOCK,
            "[[package]]\nname =",
            &[keel_manifest()]
        )
        .is_some()
    );

    let concurrent = [
        "tools/pathogens/Cargo.toml",
        "Cargo.lock",
        "apps/cli/src/main.rs",
    ];
    assert!(
        freeze_violation_with_lockfile(&concurrent, BASE_LOCK, KEEL_LOCK, &[keel_manifest()])
            .is_some()
    );
}

#[test]
fn the_checked_in_lockfile_and_gate_manifest_are_parseable() {
    const LOCK: &str = include_str!("../../../Cargo.lock");
    const MANIFEST: &str = include_str!("../../../tools/pathogens/Cargo.toml");
    let paths = keel_paths();
    assert_eq!(
        freeze_violation_with_lockfile(
            &paths,
            LOCK,
            LOCK,
            &[GateManifestChange {
                path: "tools/pathogens/Cargo.toml",
                base: MANIFEST,
                current: MANIFEST,
            }],
        ),
        None,
        "the semantic checker must understand the repository's real lockfile grammar"
    );
}

/// Whether the freeze covers this prefix, asked of the rule rather than of a copied list.
///
/// Pairing a path under `prefix` with a landmark that is certainly NOT gate machinery makes
/// `freeze_violation` answer exactly one question: is `prefix` frozen? A second copy of the
/// list here would be a second oracle, and duplicated oracles diverge in silence -- the
/// defect this repository spent #323 removing.
fn frozen(prefix: &str) -> bool {
    let under_the_prefix = format!("{prefix}any/file.rs");
    freeze_violation(&[under_the_prefix.as_str(), NOT_GATE_MACHINERY]).is_some()
}

/// A path no reasonable reading of the freeze covers.
const NOT_GATE_MACHINERY: &str = "README.md";

/// Every frozen prefix, named one assertion at a time, on purpose.
///
/// **The list is a `const` declared INSIDE `freeze_violation`, so no other reader is possible** --
/// by scope, not by search. The compiler refuses `graphhelm_quality::GATE_MACHINERY` with `E0425`;
/// a grep can only report what it happened to look at, and this needs no grep. That is also why
/// both tests here ASK the function rather than compare against a copied list: asking is the only
/// access there is. A pull request that only removes a prefix touches `core/quality/` alone, which
/// is a clean gate-only diff.
///
/// **The scope argument covers Rust, and only Rust.** A shell script or a CI file that hardcoded
/// these same prefixes by value would not be a READER of this constant, and `E0425` says nothing
/// about it -- it would simply hold a stale copy that drifts. Named here rather than left to the
/// pull request that added this paragraph, because a limit recorded only in a PR body is gone the
/// moment the PR merges.
///
/// **And the set is not hypothetically mobile -- it has already moved once, unwatched.**
/// Design note #211 records it as `[&str; 3]`; it is `[&str; 4]`
/// today. A seal in that same document points AT this constant (line 407) and noticed the growth
/// in neither direction. The set moves, things are aimed at it, and nobody was looking. (Found by
/// N while reviewing #402.)
///
/// **What a removal is NOT is silent, and the "before" measurement said so rather than confirming
/// what was expected.** Every one of the four is caught today. But the coverage is INCIDENTAL, and
/// that is what this test replaces:
///
/// | removing | caught today by | because |
/// |---|---|---|
/// | `core/quality/`, `tools/pathogens/` | the test above | it uses them as example paths |
/// | `tools/source-invariants/` | `shared_source_invariant_predicate.rs` | by design (#361) |
/// | `docs/gates/` | that same test's CONTROL | it was picked as a convenient known-gate landmark |
///
/// The last row is the fragile one: `docs/gates/` is protected only because someone needed a gate
/// path for an unrelated control. Change that landmark and the protection leaves with it, and
/// nothing anywhere records that it was load-bearing for a second purpose.
///
/// **And the message the incidental catch produces points at the wrong thing.** Removing
/// `docs/gates/` today reads `CONTROL FAILED: freeze_violation did not recognise a known gate
/// path` -- which sends the reader to debug the oracle, not to look at their own deletion. A guard
/// that fires for the right reason with the wrong name costs a debugging session before it helps.
///
/// This test cannot PREVENT that, and no in-repository check can: whoever may edit the rule may
/// edit the rule's test. What it changes is what the act LOOKS like. Removing a prefix now means
/// deleting a named assertion with its reason written beside it, rather than a comma in a list.
/// **Lowering a threshold is a plausible edit; deleting a named assertion is a visible one** --
/// the same discipline the scan floors in this repository already use, applied to the frozen set.
///
/// One assertion per prefix rather than a loop over an array, deliberately. A loop would put the
/// prefixes back into a list, and a list element is exactly what deletes without comment.
///
/// This is not hypothetical. Design note #211 records a sealed claim
/// whose death condition is *"`GATE_MACHINERY` stops listing `tools/pathogens/`"* -- another
/// lane's prediction depends on this set not shrinking, and before this test nothing would have
/// told them it had. (#342)
#[test]
fn every_frozen_prefix_is_named_here_so_removing_one_deletes_an_assertion() {
    // CONTROL FIRST. `frozen` returning true for everything would satisfy every assertion below
    // while observing nothing.
    assert!(
        !frozen("apps/cli/"),
        "CONTROL FAILED: ordinary code reads as frozen, so the verdicts below mean nothing"
    );

    assert!(
        frozen("core/quality/"),
        "core/quality/ left the freeze. It holds the rule itself, its enforcement, and the \
         thymus suite -- unfrozen, a branch may rewrite the judge in the same breath as the code \
         the judge is judging, which is the whole of M06 binding decision 5"
    );

    assert!(
        frozen("tools/pathogens/"),
        "tools/pathogens/ left the freeze. It is the pathogen suite the gate certifies against, \
         and a branch that edits a pathogen alongside the code it detects has moved the target \
         and the shot together"
    );

    assert!(
        frozen("docs/gates/"),
        "docs/gates/ left the freeze. It holds the freeze charter itself (docs/gates/freeze.md), \
         so a branch that rewrites what the freeze means alongside the code the freeze judges has \
         amended the law in the same breath as the act. (This message once claimed the directory \
         held gate stamps; measured in #282, no stamp was ever written there -- certification is \
         a GateCertified event in the stream, and the directory was empty until the charter \
         moved in.)"
    );

    assert!(
        frozen("tools/source-invariants/"),
        "tools/source-invariants/ left the freeze. It holds the shared detection predicate, and \
         a predicate is not an input to a gate -- it IS the gate. Outside the freeze it can be \
         edited in the same pull request as the code it judges (#323)"
    );
}

/// Tracked files under one repository prefix, asked of git rather than the filesystem.
///
/// `git ls-files` on purpose: a stray UNTRACKED file in an otherwise-empty prefix would satisfy a
/// filesystem count while the repository still ships nothing there -- and shipping nothing is the
/// condition under test. `freeze_enforced.rs` in this same suite already shells to git, so the
/// dependency is not new.
///
/// A git that cannot answer is a PANIC, not a zero: an unanswerable population reported as empty
/// would fail the assertions below with an accusation about the wrong thing.
fn tracked_file_count(prefix: &str) -> usize {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(std::path::Path::parent)
        .expect("core/quality sits two levels below the repository root");
    let output = std::process::Command::new("git")
        .args(["ls-files", "--", prefix])
        .current_dir(root)
        .output()
        .unwrap_or_else(|e| panic!("HARNESS-BROKE: git ls-files did not run: {e}"));
    assert!(
        output.status.success(),
        "HARNESS-BROKE: git ls-files failed for {prefix}, so the population below is undefined \
         rather than empty: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter(|line| !line.trim().is_empty())
        .count()
}

/// Every frozen prefix matches at least one TRACKED file.
///
/// #282's class, pinned rather than merely fixed. `docs/gates/` sat empty in `GATE_MACHINERY`
/// from the day the constant was written: one quarter of the frozen surface could never refuse
/// anything, the guard above could not tell that entry from a typo, and the assertion protecting
/// it justified the emptiness with contents that never existed. #554 made the prefix non-empty by
/// moving the freeze charter in -- and nothing pinned it there. Rename `docs/gates/freeze.md`
/// away and every test in this file stays green while the prefix quietly returns to the state
/// #282 describes. (Cell shape measured by D reviewing #554: red at `6136ff2`, green at
/// `bd5544f`, on the exact defect.)
///
/// One named assertion per prefix, matching the test above and for its reason: a loop would put
/// the prefixes back into a list, and a list element is exactly what deletes without comment.
///
/// **DECLARED LIMIT, and it is a choice, not an omission (D's condition on closing #282).** The
/// four prefixes below are a HAND COPY of `GATE_MACHINERY`, by necessity: the constant is
/// unreadable outside `freeze_violation` by design -- `E0425`, no other reader is possible (see
/// the scope argument on the test above). So an entry added to the constant must be added here by
/// hand, and the two ways that goes wrong were MEASURED separately while closing #282, because
/// they end differently:
///
/// * **replacing** a listed prefix with a misspelling (`docs/gates/` -> `docs/gatez/`) is CAUGHT
///   -- the named assertion above fails, since the real prefix stops being frozen;
/// * **adding** a misspelled fifth entry (`docs/gatez/` alongside the four) is SILENT -- it
///   freezes a prefix that matches nothing, and every test in this file stays green, this cell
///   included, because nothing outside `freeze_violation` can read the constant to learn the
///   ghost entry exists.
///
/// The only reader of the truth is `freeze_violation`'s own runtime. What this cell DOES pin is
/// the mirror half: a prefix both sides spell correctly whose contents quietly leave the
/// repository.
#[test]
fn every_frozen_prefix_matches_at_least_one_tracked_file() {
    assert!(
        tracked_file_count("core/quality/") >= 1,
        "core/quality/ matches no tracked file. The freeze's own rule, enforcement, and thymus \
         harness are supposed to live here -- an empty prefix guards nothing and cannot be told \
         from a typo (#282)"
    );
    assert!(
        tracked_file_count("tools/pathogens/") >= 1,
        "tools/pathogens/ matches no tracked file. The pathogen suite the gate certifies against \
         is supposed to live here -- an empty prefix guards nothing and cannot be told from a \
         typo (#282)"
    );
    assert!(
        tracked_file_count("docs/gates/") >= 1,
        "docs/gates/ matches no tracked file. The freeze charter moved here in #554 precisely so \
         this prefix would stop being the empty third of the frozen surface (#282). Which is it: \
         did gate documentation move without its prefix, or is docs/gates/ no longer meant to be \
         frozen at all? The answer decides the edit, and it is not this message's to make -- it \
         belongs in the same commit as the named assertion above, with the reason written"
    );
    assert!(
        tracked_file_count("tools/source-invariants/") >= 1,
        "tools/source-invariants/ matches no tracked file. The shared detection predicate is \
         supposed to live here -- an empty prefix guards nothing and cannot be told from a typo \
         (#282)"
    );
}

/// The gate's OWN run manifest is not gated code (#898).
///
/// #674(a) made every authoritative gate commit its manifest under `.factory/gate-runs/` onto the
/// branch it judged. Read as "code side" by `freeze_violation`, that receipt paired with any
/// gate-machinery path -- so every branch touching `core/quality/` or `tools/pathogens/` was RED at
/// `this_branch_does_not_move_the_judge_and_the_judged_together` from its SECOND run on, and a
/// gate-machinery change could never carry the GREEN manifest `merge-proof` (retired
/// 2026-09-24) required. The rule condemned its own receipt. Measured on #859's third gate
/// (manifest `c6887f05`). Receipts are no longer committed, but `ci/gate.ps1` still writes its
/// manifests under that prefix, so the exemption stays.
///
/// The exemption is the store's prefix and nothing wider, and the three controls below are what
/// keep it from becoming a bypass: the real pairing still refuses, code-only stays clean, and a
/// non-manifest file under `.factory/` still counts as code.
#[test]
fn the_gates_own_run_manifest_is_not_gated_code() {
    const MANIFEST: &str = ".factory/gate-runs/22a92349944f-20260905T064634.239Z-b99c28d2.json";
    const GATE: &str = "tools/pathogens/src/jpd.rs";

    assert!(
        freeze_violation(&[GATE, MANIFEST]).is_none(),
        "a gate-machinery branch paired with the manifest its own gate committed was refused: the \
         rule is condemning its own receipt, and no gate-machinery change can ever go GREEN"
    );
    assert!(
        freeze_violation(&[MANIFEST, GATE]).is_none(),
        "same pair, manifest first: the exemption must not depend on path order"
    );

    // CONTROL 1: the rule itself is untouched -- judge and judged still refuse.
    assert!(
        freeze_violation(&[GATE, "apps/cli/src/commands/serve/monitor.rs"]).is_some(),
        "CONTROL FAILED: a gate path beside production code no longer refuses, so the exemption \
         widened into a bypass"
    );
    // CONTROL 2: code-only with a manifest is clean, as code-only always was.
    assert!(
        freeze_violation(&[MANIFEST, "apps/cli/src/commands/serve/monitor.rs"]).is_none(),
        "CONTROL FAILED: a manifest beside production code reads as a violation, so the manifest \
         is being treated as gate machinery instead of as neither"
    );
    // CONTROL 3: the exemption is the STORE, not the whole `.factory/` tree.
    assert!(
        freeze_violation(&[GATE, ".factory/orchestrator-board.md"]).is_some(),
        "CONTROL FAILED: a non-manifest file under .factory/ stopped counting as code, so the \
         exemption is wider than the store it names"
    );
}
