//! `acceptance_map_is_grounded` — the map cannot rust: every prover fn exists exactly once
//! in the tree, its suite is on the gate's invocation surface, its assert fingerprint
//! appears inside the test's own body (gutting the test breaks the map), every D-040
//! citation still appears in the decision register, and the committed document is
//! byte-identical to what the generator emits.

use acceptance_map::{
    fn_body, generate, load_clauses, repo_root, rust_sources, verify_artifacts,
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
                    gate.contains(&format!("'{}'", prover.suite)),
                    "{}: suite {} must be on the gate's CLI suite list",
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
