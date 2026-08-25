//! #211: guards for the retry-lineage validator.
//!
//! **The first two tests drive the repository's own shipped fixtures**, and they are first on
//! purpose. #294 shipped a gate whose synthetic specimens all passed while every real document
//! slipped through, including the repository's own negative fixture: a suite written by the same
//! hand as the gate agrees with it about VOCABULARY by construction. Synthetic specimens can
//! disagree with a gate about logic; never about vocabulary. So the foreign documents go first,
//! and everything derived from them comes after.

use pathogens::jpd::JpdEvidence;
use pathogens::retry_lineage::{
    LineageCheck, RetryLineageGate, failing_checks, retry_lineage_suite,
};
use pathogens::{EvidenceGate, certify, is_defeated_on_its_axis};
use std::path::{Path, PathBuf};

/// Load a fixture the JPD extension ships. Panics loudly rather than returning an empty document:
/// a fixture that silently failed to load would make every assertion below vacuous.
fn fixture(kind: &str, name: &str) -> serde_json::Value {
    let path: PathBuf = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../extensions/builtin/graphhelm-jpd/fixtures")
        .join(kind)
        .join(name);
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("fixture {} unreadable: {error}", path.display()));
    serde_json::from_str(&text).expect("fixture parses as JSON")
}

/// The shipped negative fixture must fail exactly one declared check, and it must be the one its
/// own `firstFailure.summary` describes: *"the supposed retry omits retryOf and is therefore
/// orphaned."*
///
/// Asserted as an EQUALITY over the whole list rather than a `contains`: with
/// `all_checks_required`, a checker that failed everything would satisfy `contains` while meaning
/// something completely different, and the fixture's value as an instrument comes precisely from
/// its clean single-arm signature.
#[test]
fn the_repositorys_negative_fixture_fails_exactly_the_adjacency_check() {
    let document = fixture("negative", "invalid-retry-lineage.json");
    assert_eq!(
        failing_checks(&document),
        vec![LineageCheck::RetryEdgesAdjacent],
        "the shipped negative fixture's own summary says the retry omits retryOf; a different set \
         here means the checks do not mean what the policy says they mean"
    );
}

/// And the sound document must fail nothing, so the validator is not "refuse everything".
#[test]
fn the_repositorys_positive_fixture_fails_nothing() {
    let document = fixture("positive", "recovered-retry-chain.json");
    assert_eq!(
        failing_checks(&document),
        Vec::<LineageCheck>::new(),
        "a validator that refuses the good document is not strict, it is broken"
    );
}

/// Copy a real fixture with one pointer replaced.
///
/// The STRUCTURE stays foreign and only the field under test moves. This is the compromise the
/// sealed limitation names: it cannot give a check a genuinely foreign adversary, but it stops my
/// vocabulary from quietly replacing the document's.
fn mutate(
    document: &serde_json::Value,
    pointer: &str,
    value: serde_json::Value,
) -> serde_json::Value {
    let mut copy = document.clone();
    let slot = copy
        .pointer_mut(pointer)
        .unwrap_or_else(|| panic!("pointer {pointer} does not exist in the fixture"));
    *slot = value;
    copy
}

/// Every declared check must be capable of failing.
///
/// **Why this exists.** The positive-fixture test asserts that nothing fails, and that assertion is
/// satisfied just as well by a check that can NEVER fail. An unreddenable branch is the mirror of a
/// guard that was never red: both are green for a reason that has nothing to do with the property.
/// The `uncovered` assertion at the end is the part that survives someone adding a tenth check.
#[test]
fn every_declared_check_can_fail_on_its_own() {
    let good = fixture("positive", "recovered-retry-chain.json");
    let cases: Vec<(LineageCheck, serde_json::Value)> = vec![
        (
            LineageCheck::RootAttemptIdMatches,
            mutate(
                &good,
                "/rootAttemptId",
                serde_json::json!("attempt/does-not-exist"),
            ),
        ),
        (
            LineageCheck::AttemptIdsUnique,
            mutate(
                &good,
                "/retries/0/attemptId",
                serde_json::json!("attempt/issue-210/001"),
            ),
        ),
        (
            LineageCheck::OrdinalsContiguous,
            mutate(&good, "/retries/0/ordinal", serde_json::json!(7)),
        ),
        (
            LineageCheck::RetryEdgesAdjacent,
            mutate(
                &good,
                "/retries/0/retryOf",
                serde_json::json!("attempt/nowhere"),
            ),
        ),
        (
            LineageCheck::TimestampsMonotonic,
            mutate(
                &good,
                "/retries/0/startedAt",
                serde_json::json!("2026-08-22T11:00:00Z"),
            ),
        ),
        (
            LineageCheck::SuccessfulAttemptExistsAndSucceeded,
            mutate(&good, "/retries/0/result", serde_json::json!("failed")),
        ),
        (
            LineageCheck::FirstFailureMatchesRoot,
            mutate(
                &good,
                "/firstFailure/attemptId",
                serde_json::json!("attempt/issue-210/002"),
            ),
        ),
        (
            LineageCheck::EvidenceDeltasDigestBound,
            mutate(
                &good,
                "/retries/0/evidenceDelta/added/0/contentSha256",
                serde_json::json!(
                    "9999999999999999999999999999999999999999999999999999999999999999"
                ),
            ),
        ),
    ];

    for (check, document) in &cases {
        let failing = failing_checks(document);
        assert!(
            failing.contains(check),
            "the mutation aimed at {check:?} did not make it fail; got {failing:?}. That check \
             cannot be reddened, so the positive fixture passing it means nothing"
        );
        // A mutation that fails EVERY check would satisfy the assertion above while proving the
        // checks are not independent at all -- one broken field would be indistinguishable from
        // nine. Nine-of-nine is the shape that says the checks share a single point of failure.
        assert!(
            failing.len() < LineageCheck::every().len(),
            "one mutation aimed at {check:?} failed every declared check ({failing:?}); the checks \
             are not independent"
        );
    }

    // Every check except the acyclic one, which needs a cycle rather than a single field edit and
    // has its own test below. Named rather than silently absent: a list of covered things reads as
    // exhaustive, so the gap has to be stated where the list is.
    let covered: Vec<LineageCheck> = cases.iter().map(|(check, _)| *check).collect();
    let uncovered: Vec<LineageCheck> = LineageCheck::every()
        .into_iter()
        .filter(|check| !covered.contains(check) && *check != LineageCheck::RetryEdgesAcyclic)
        .collect();
    assert!(
        uncovered.is_empty(),
        "these declared checks have no mutation proving they can fail: {uncovered:?}"
    );
}

/// The acyclic check, which a single field edit on the good fixture cannot reach.
#[test]
fn a_cycle_between_two_attempts_fails_the_acyclic_check() {
    let good = fixture("positive", "recovered-retry-chain.json");
    // Point the root at its own retry: root -> 002 -> root.
    let cyclic = mutate(
        &good,
        "/rootAttempt/retryOf",
        serde_json::json!("attempt/issue-210/002"),
    );
    assert!(
        failing_checks(&cyclic).contains(&LineageCheck::RetryEdgesAcyclic),
        "a two-attempt cycle must fail the acyclic check"
    );
}

// ---------------------------------------------------------------------------------------------
// The gate itself.
// ---------------------------------------------------------------------------------------------

/// The gate must refuse the shipped broken document and accept the shipped sound one.
#[test]
fn the_gate_refuses_the_negative_fixture_and_accepts_the_positive() {
    let gate = RetryLineageGate;
    let bad = JpdEvidence::RetryLineage(fixture("negative", "invalid-retry-lineage.json"));
    let good = JpdEvidence::RetryLineage(fixture("positive", "recovered-retry-chain.json"));

    let refused = gate.evaluate(&bad);
    assert!(
        !refused.passed,
        "the shipped negative fixture must be refused"
    );
    assert!(
        refused
            .findings
            .iter()
            .any(|finding| finding.contains("RetryEdgesAdjacent")),
        "the refusal must name the check that failed, not merely refuse; got {:?}",
        refused.findings
    );
    assert!(
        gate.evaluate(&good).passed,
        "the shipped positive fixture must be accepted; a gate that refuses everything is not \
         strict, it is broken"
    );
}

/// A specimen that defeats nothing certifies a gate that caught nothing.
#[test]
fn every_specimen_defeats_the_axis_it_names() {
    let suite = retry_lineage_suite();
    // Guards the LOOP below, not `certify`: a `for` over an empty collection asserts nothing.
    // `certify`'s empty-suite floor does NOT make this redundant -- this test never calls
    // `certify` -- and the old message said "certifies vacuously", which described a different
    // mechanism and nearly got this deleted as redundant when the floor landed.
    assert!(
        !suite.is_empty(),
        "HARNESS-BROKE: the suite is empty, so the loop below checks nothing"
    );
    for specimen in &suite {
        assert!(
            is_defeated_on_its_axis(specimen),
            "specimen {} does not actually defeat {:?}",
            specimen.id,
            specimen.axis
        );
    }
}

/// The gate survives its own suite.
#[test]
fn the_gate_is_certified_by_the_suite() {
    let certification = certify(&RetryLineageGate, &retry_lineage_suite())
        .expect("the gate must reject every specimen");
    // `Certification::specimens` is a `u32` and `len()` is a `usize`; cast at the comparison rather
    // than changing a harness field this slice does not own.
    assert_eq!(
        certification.specimens as usize,
        retry_lineage_suite().len()
    );
    assert_eq!(
        certification.gate_id, "graphhelm-jpd/retry-lineage-validator",
        "the gate id must be the evaluatorId the policy's success block names"
    );
}

/// **The verdict must not move when the document's own grade of itself moves.**
///
/// This is the most valuable test in the file. Every other test proves the gate computes the right
/// answer; this proves it computes the answer FROM THE RIGHT PLACE. #294's gate also produced
/// plausible verdicts -- from invented fields.
///
/// `lineageValidation` is this evaluator's declared OUTPUT (`outputField` in the policy) and
/// `outcomeClass` belongs to the classifier downstream. In the two shipped fixtures those fields
/// separate good from bad PERFECTLY, so a gate reading either passes every other test in this file
/// and is worth nothing. Only this test can tell the difference.
#[test]
fn the_verdict_ignores_the_documents_self_report() {
    let gate = RetryLineageGate;

    // The broken document, relabelled to claim it is fine.
    let bad = fixture("negative", "invalid-retry-lineage.json");
    let bad_claiming_valid = mutate(
        &bad,
        "/lineageValidation/result",
        serde_json::json!("valid"),
    );
    let bad_claiming_valid = mutate(
        &bad_claiming_valid,
        "/outcomeClass",
        serde_json::json!("recovered_success"),
    );

    // The sound document, relabelled to claim it is broken.
    let good = fixture("positive", "recovered-retry-chain.json");
    let good_claiming_invalid = mutate(
        &good,
        "/lineageValidation/result",
        serde_json::json!("invalid"),
    );
    let good_claiming_invalid = mutate(
        &good_claiming_invalid,
        "/outcomeClass",
        serde_json::json!("flaky_pass"),
    );

    assert!(
        !gate
            .evaluate(&JpdEvidence::RetryLineage(bad_claiming_valid))
            .passed,
        "a broken chain that calls itself valid must still be refused: the gate is reading its own \
         declared output instead of the attempt structure"
    );
    assert!(
        gate.evaluate(&JpdEvidence::RetryLineage(good_claiming_invalid))
            .passed,
        "a sound chain that calls itself invalid must still be accepted: refusing on the \
         self-report looks conservative and is the same defect mirrored"
    );
}

// ---------------------------------------------------------------------------------------------
// The policy is READ, not transcribed.
// ---------------------------------------------------------------------------------------------

/// `LineageCheck` must be exactly the policy's `requiredChecks` — compared as SETS, in both
/// directions, against the declared file.
///
/// **Why this replaces the transcription.** The nine variants were copied verbatim out of the
/// policy, and the module comment called that the lesson of #294. It is only half of it. #294's
/// lesson was *do not invent names*; copying them is the failure that comes NEXT — a verbatim copy
/// is a **second declaration of a closed set**, and two declarations drift in silence because
/// nothing compares them. Reading the file is what makes the policy an authority rather than a
/// citation. (Found by L, who measured both directions before saying so.)
///
/// **What it buys, precisely.** `all_checks_required` was already guaranteed in the RUST direction
/// — `holds` matches exhaustively, so a new variant breaks the build. In the POLICY direction it
/// was only ASSERTED: a tenth entry in the YAML broke nothing and was silently unexecuted. This
/// equality breaks.
///
/// The wire spellings come from **serde itself**, never from a `snake_case` helper written here: a
/// third hand-maintained spelling of one vocabulary would be the very defect this test exists to
/// close, one level down.
///
/// **No YAML crate, and that is forced rather than lazy.** A dev-dependency would edit
/// `Cargo.lock`, which lives at the repository ROOT and is therefore OUTSIDE gate machinery — so
/// adding one to `tools/pathogens` would move the judge and the judged together and M06's freeze
/// would refuse this branch. The reader below is deliberately tiny, and the set comparison is what
/// makes it safe: a reader that dropped or invented an entry changes one side of an equality that
/// is asserted in BOTH directions, so it fails LOUDLY rather than passing with less.
fn declared_required_checks(policy: &str) -> Vec<String> {
    let mut inside = false;
    let mut found = Vec::new();
    for line in policy.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("requiredChecks:") {
            inside = true;
            continue;
        }
        if inside {
            if let Some(entry) = trimmed.strip_prefix("- ") {
                found.push(entry.trim().to_owned());
            } else if !trimmed.is_empty() {
                // The sequence ended. Stop rather than scanning on: a later `- ` under some other
                // key would silently enlarge the set with entries that are not required checks.
                break;
            }
        }
    }
    found
}

#[test]
fn the_declared_policy_and_the_implemented_checks_are_the_same_set() {
    const POLICY: &str = include_str!(
        "../../../extensions/builtin/graphhelm-jpd/evaluators/retry-lineage-validation-policy.yaml"
    );
    let declared: std::collections::BTreeSet<String> =
        declared_required_checks(POLICY).into_iter().collect();

    let implemented: std::collections::BTreeSet<String> = LineageCheck::every()
        .into_iter()
        .map(|check| {
            serde_json::to_value(check)
                .expect("a unit variant serializes")
                .as_str()
                .expect("as a string")
                .to_owned()
        })
        .collect();

    // Non-emptiness FIRST: two empty sets are equal, so a reader that silently found nothing would
    // satisfy the comparison below while proving absolutely nothing.
    assert!(
        !declared.is_empty(),
        "HARNESS-BROKE: read zero requiredChecks from the declared policy; the equality below would pass vacuously"
    );
    assert_eq!(
        declared,
        implemented,
        "the declared policy and the implemented checks have diverged.
  declared only: {:?}
           implemented only: {:?}",
        declared.difference(&implemented).collect::<Vec<_>>(),
        implemented.difference(&declared).collect::<Vec<_>>()
    );
}
