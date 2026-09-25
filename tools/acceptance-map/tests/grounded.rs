//! `acceptance_map_is_grounded` — the map cannot rust: every prover fn exists exactly once
//! in the tree, its suite is on the gate's invocation surface, its assert fingerprint
//! appears inside the test's own body (gutting the test breaks the map), every D-040
//! citation still appears in the decision register, and the committed document is
//! byte-identical to what the generator emits.

use acceptance_map::{
    fn_body, gate_cli_suites, generate, load_clauses, repo_root, rust_sources, verify_artifacts,
    verify_demonstration, verify_tracked,
};

#[test]
fn acceptance_map_is_grounded() {
    let root = repo_root();
    let clauses = load_clauses(&root);
    let sources: Vec<(std::path::PathBuf, String)> = rust_sources(&root)
        .into_iter()
        .map(|path| {
            let text = std::fs::read_to_string(&path).unwrap_or_default();
            (path, text)
        })
        .collect();
    let gate = std::fs::read_to_string(root.join("ci/gate.ps1")).expect("gate.ps1 readable");
    // #104: since #98, the gate discovers CLI suites from apps/cli/tests/*.rs rather than
    // naming them as literals - a per-prover `gate.contains("'suite'")` check went
    // permanently vacuous the day that landed. This recomputes the gate's own suite set
    // (discovery, minus the gate's own named exclusions) instead of string-matching text
    // the gate no longer carries.
    let gate_suites = gate_cli_suites(&root);

    assert_eq!(clauses.clause.len(), 7, "the seven §8 clauses, exactly");

    // The manual run's committed evidence still exists and still hashes to what the run
    // recorded — in both directions (nothing named missing, nothing on disk unnamed) —
    // and every artifact file is TRACKED (a named but gitignored artifact vanishes from
    // fresh clones: the 05f journal lesson, paid as a check).
    for clause in &clauses.clause {
        for artifact in &clause.artifact {
            let problems = verify_artifacts(&root, &artifact.directory);
            assert!(
                problems.is_empty(),
                "{}: the run evidence has rusted: {problems:?}",
                clause.id
            );
            let untracked = verify_tracked(&root, &artifact.directory);
            assert!(
                untracked.is_empty(),
                "{}: run evidence must be tracked: {untracked:?}",
                clause.id
            );
        }
        // The third binding: every demonstration replays against the CURRENT build —
        // frozen seed, seed-derived traversal, and the recorded projection digest.
        for demonstration in &clause.demonstration {
            let problems = verify_artifacts(&root, &demonstration.directory);
            assert!(
                problems.is_empty(),
                "{}: the demonstration evidence has rusted: {problems:?}",
                clause.id
            );
            let untracked = verify_tracked(&root, &demonstration.directory);
            assert!(
                untracked.is_empty(),
                "{}: a demonstration named but untracked is a grounding failure: {untracked:?}",
                clause.id
            );
            let problems = verify_demonstration(&root, &demonstration.directory);
            assert!(
                problems.is_empty(),
                "{}: the demonstration no longer replays: {problems:?}",
                clause.id
            );
        }
    }
    assert!(
        clauses
            .clause
            .iter()
            .any(|clause| !clause.demonstration.is_empty()),
        "at least one clause carries the third binding"
    );

    for clause in &clauses.clause {
        assert!(
            clause.gate || !clause.prover.is_empty(),
            "{}: every clause names a prover or is the gate clause",
            clause.id
        );
        for prover in &clause.prover {
            let needle = format!("fn {}(", prover.function);
            let defining: Vec<&std::path::PathBuf> = sources
                .iter()
                .filter(|(_, text)| text.contains(&needle))
                .map(|(path, _)| path)
                .collect();
            assert_eq!(
                defining.len(),
                1,
                "{}: `{}` must exist exactly once in the tree, found in {defining:?}",
                clause.id,
                prover.function
            );
            let (_, source) = sources
                .iter()
                .find(|(_, text)| text.contains(&needle))
                .unwrap();
            let body = fn_body(source, &prover.function)
                .unwrap_or_else(|| panic!("{}: a brace-balanced body", prover.function));
            assert!(
                body.contains(&prover.assert_fingerprint),
                "{}: the fingerprint {:?} must appear inside `{}`'s body — a gutted test \
                 does not count as a prover",
                clause.id,
                prover.assert_fingerprint,
                prover.function
            );
            if prover.suite == "workspace tests" {
                assert!(
                    gate.contains("test --workspace"),
                    "the gate must run the workspace tests stage"
                );
            } else {
                assert!(
                    gate_suites.iter().any(|suite| suite == &prover.suite),
                    "{}: suite {} must be on the gate's discovered CLI suite list \
                     (apps/cli/tests/*.rs minus any suite named in gate.ps1's own \
                     $excludedSuites map) - discovered {gate_suites:?}",
                    clause.id,
                    prover.suite
                );
            }
        }
    }

    // The gate clause: the script still ends in the GREEN verdict and still carries both
    // PostgreSQL passes on its surface.
    assert!(
        clauses.clause.iter().any(|clause| clause.gate),
        "one clause is proven by the gate run itself"
    );
    assert!(gate.contains("GREEN - every stage passed"));
    assert!(gate.contains("PostgreSQL ignored matrix"));
    assert!(gate.contains("PostgreSQL matrix under a non-C collation"));
    // The suite set must stay DISCOVERED, not revert to a hand-maintained literal array
    // (#98's own fix) - otherwise `gate_cli_suites` above would silently stop describing what
    // the gate actually runs.
    //
    // #1053 changed WHERE the discovery happens, and this assertion moved with it. The gate used
    // to walk `apps/cli/tests/*.rs` itself and run one `cargo test --test <suite>` per name; it
    // now runs `cargo nextest run -p graphhelm-cli` once, so the enumeration is cargo's. That is
    // a STRONGER form of #98's property, not a weaker one: there is no loop, no list and no
    // exclusion map left that an edit could narrow -- a new integration target is compiled and
    // run because it EXISTS. What this pins is that the gate keeps running the whole PACKAGE.
    // Narrow it to `--test <something>` and `gate_cli_suites` starts over-describing coverage.
    assert!(
        gate.contains("nextest run -p graphhelm-cli"),
        "the gate must still run the whole graphhelm-cli package in one nextest \
         invocation, so every apps/cli/tests/*.rs target is gated by existing rather than \
         by being listed"
    );

    // The refused scope: every citation resolves in the decision register, verbatim.
    let register = std::fs::read_to_string(root.join("docs/DECISION_REGISTER.md"))
        .expect("the decision register is readable");
    assert!(
        !clauses.refused.is_empty(),
        "the refused-scope table exists"
    );
    for refused in &clauses.refused {
        assert!(
            register.contains(&refused.citation),
            "the citation for {:?} must still appear in the decision register",
            refused.affordance
        );
    }

    // The committed document is exactly what the generator emits — regeneration is the fix,
    // hand-editing is not.
    let committed = std::fs::read_to_string(root.join("docs/acceptance/M05_ACCEPTANCE_MAP.md"))
        .expect("M05_ACCEPTANCE_MAP.md is committed");
    assert_eq!(
        committed.replace("\r\n", "\n"),
        generate(&clauses),
        "the committed map must be byte-identical to the generator's output \
         (cargo run -p acceptance-map to regenerate)"
    );
}
