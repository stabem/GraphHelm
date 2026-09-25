//! #211: the executor for `retry-lineage-validation-policy`, which had none.
//!
//! The policy at `extensions/builtin/graphhelm-jpd/evaluators/retry-lineage-validation-policy.yaml`
//! declares nine required checks and `runtimeStatus: declarative_only`. A declarative policy with
//! no executor is a document that looks like a rule.
//!
//! **This module may not read `lineageValidation`, `outcomeClass`, `classification` or
//! `classificationBasis`.** That is not a style preference — it is derived from the schemas. The
//! policy declares `outputField: lineageValidation`, and the three schemas form a strict chain:
//!
//! | schema | top-level properties |
//! |---|---|
//! | `retry-lineage-input` | 7, and **no** `lineageValidation` — what this evaluator CONSUMES |
//! | `retry-chain-input` | those 7 **+ `lineageValidation`** — input plus this evaluator's OUTPUT |
//! | `retry-chain` | those 8 **+ `outcomeClass`, `classification`, `classificationBasis`** |
//!
//! So the readable set is exactly the seven properties of `retry-lineage-input.schema.json`. In the
//! two shipped fixtures `lineageValidation.result` is `invalid` in the negative and `valid` in the
//! positive, so a gate reading that single field separates them PERFECTLY and is worth nothing: it
//! would be reading the grade the document gives itself. Enforced by observation in
//! `the_verdict_ignores_the_documents_self_report`, never by this comment — a comment mentions a
//! rule, a test fails when the rule breaks.

use serde::Serialize;
use serde_json::Value;

use crate::jpd::JpdEvidence;
use crate::{EvidenceGate, FailureAxis, Specimen, Verdict};

/// The nine checks `retry-lineage-validation-policy.yaml` declares, in its own vocabulary.
///
/// A CLOSED set, matched exhaustively: the policy says `all_checks_required`, so a check added
/// upstream must break this build rather than be silently skipped. The wire spellings are the
/// policy's `requiredChecks` entries verbatim — the names were copied from the declaration, not
/// invented, which is the whole lesson of #294.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LineageCheck {
    RootAttemptIdMatches,
    AttemptIdsUnique,
    OrdinalsContiguous,
    RetryEdgesAdjacent,
    RetryEdgesAcyclic,
    TimestampsMonotonic,
    SuccessfulAttemptExistsAndSucceeded,
    FirstFailureMatchesRoot,
    EvidenceDeltasDigestBound,
}

impl LineageCheck {
    /// Every check, in policy order.
    ///
    /// Walked by [`failing_checks`] so that adding a variant without teaching [`holds`] about it
    /// fails to compile, and so a test can assert over the whole population rather than over the
    /// subset someone remembered.
    #[must_use]
    pub const fn every() -> [Self; 9] {
        [
            Self::RootAttemptIdMatches,
            Self::AttemptIdsUnique,
            Self::OrdinalsContiguous,
            Self::RetryEdgesAdjacent,
            Self::RetryEdgesAcyclic,
            Self::TimestampsMonotonic,
            Self::SuccessfulAttemptExistsAndSucceeded,
            Self::FirstFailureMatchesRoot,
            Self::EvidenceDeltasDigestBound,
        ]
    }
}

/// Every attempt in the document: the root first, then the retries as given.
fn attempts(document: &Value) -> Vec<&Value> {
    let mut all: Vec<&Value> = Vec::new();
    if let Some(root) = document.get("rootAttempt") {
        all.push(root);
    }
    if let Some(retries) = document.get("retries").and_then(Value::as_array) {
        all.extend(retries.iter());
    }
    all
}

fn attempt_id(attempt: &Value) -> Option<&str> {
    attempt.get("attemptId").and_then(Value::as_str)
}

/// Which of the nine declared checks this document fails, in policy order.
///
/// Returns a LIST rather than a bool. Under `all_checks_required` a bool would collapse nine
/// distinct structural facts into one, and an assertion over it would be satisfied by a document
/// failing the wrong check — the shape where a fixture passes a guard for a reason nobody intended.
#[must_use]
pub fn failing_checks(document: &Value) -> Vec<LineageCheck> {
    LineageCheck::every()
        .into_iter()
        .filter(|check| !holds(*check, document))
        .collect()
}

/// Whether one declared check holds over the document.
///
/// Every arm reads only the seven input-schema properties. A missing or mistyped field makes its
/// check FAIL rather than pass: this is untrusted evidence, and a check that treats absence as
/// satisfaction is a check that a malformed document walks straight through.
fn holds(check: LineageCheck, document: &Value) -> bool {
    let all = attempts(document);
    let ids: Vec<&str> = all
        .iter()
        .filter_map(|attempt| attempt_id(attempt))
        .collect();
    match check {
        LineageCheck::RootAttemptIdMatches => {
            let declared = document.get("rootAttemptId").and_then(Value::as_str);
            declared.is_some() && declared == document.get("rootAttempt").and_then(attempt_id)
        }
        LineageCheck::AttemptIdsUnique => {
            // `ids.len() == all.len()` also rejects an attempt with no id at all, which would
            // otherwise be dropped by `filter_map` and read as "no duplicates".
            let mut seen: Vec<&str> = ids.clone();
            seen.sort_unstable();
            seen.dedup();
            ids.len() == all.len() && seen.len() == ids.len()
        }
        LineageCheck::OrdinalsContiguous => {
            let mut ordinals: Vec<u64> = all
                .iter()
                .filter_map(|attempt| attempt.get("ordinal").and_then(Value::as_u64))
                .collect();
            ordinals.sort_unstable();
            ordinals.len() == all.len()
                && ordinals
                    .iter()
                    .enumerate()
                    .all(|(index, ordinal)| *ordinal == index as u64 + 1)
        }
        // The check the shipped negative fixture violates: every non-root attempt must name a
        // `retryOf` that exists among the attempt ids. An orphan retry breaks the chain while all
        // eight other checks still pass, which is what makes that fixture a clean instrument.
        LineageCheck::RetryEdgesAdjacent => document
            .get("retries")
            .and_then(Value::as_array)
            .is_some_and(|retries| {
                retries.iter().all(|retry| {
                    retry
                        .get("retryOf")
                        .and_then(Value::as_str)
                        .is_some_and(|parent| ids.contains(&parent))
                })
            }),
        LineageCheck::RetryEdgesAcyclic => all.iter().all(|start| {
            let mut seen: Vec<&str> = Vec::new();
            let mut cursor: &Value = start;
            loop {
                let Some(id) = attempt_id(cursor) else {
                    return true;
                };
                if seen.contains(&id) {
                    return false;
                }
                seen.push(id);
                let Some(parent) = cursor.get("retryOf").and_then(Value::as_str) else {
                    return true;
                };
                let Some(next) = all.iter().find(|a| attempt_id(a) == Some(parent)) else {
                    // A dangling edge is `RetryEdgesAdjacent`'s finding, not this one. Each check
                    // reports its own defect so a specimen cannot satisfy the wrong assertion.
                    return true;
                };
                cursor = next;
            }
        }),
        LineageCheck::TimestampsMonotonic => {
            let mut ordered: Vec<&Value> = all.clone();
            ordered.sort_by_key(|attempt| attempt.get("ordinal").and_then(Value::as_u64));
            let stamps: Vec<(&str, &str)> = ordered
                .iter()
                .filter_map(|attempt| {
                    Some((
                        attempt.get("startedAt").and_then(Value::as_str)?,
                        attempt.get("endedAt").and_then(Value::as_str)?,
                    ))
                })
                .collect();
            // RFC3339 UTC stamps with a fixed shape compare correctly as strings; the fixtures and
            // the input schema both use `...Z`. Compared lexically rather than parsed so this
            // module keeps no date dependency, and any stamp that does not fit the shape sorts
            // wrong and FAILS the check rather than passing it quietly.
            stamps.len() == all.len()
                && stamps.iter().all(|(start, end)| start <= end)
                && stamps.windows(2).all(|pair| pair[0].1 <= pair[1].0)
        }
        LineageCheck::SuccessfulAttemptExistsAndSucceeded => {
            document
                .get("successfulAttemptId")
                .and_then(Value::as_str)
                .and_then(|id| all.iter().find(|attempt| attempt_id(attempt) == Some(id)))
                .and_then(|attempt| attempt.get("result").and_then(Value::as_str))
                == Some("succeeded")
        }
        LineageCheck::FirstFailureMatchesRoot => {
            let first = document
                .pointer("/firstFailure/attemptId")
                .and_then(Value::as_str);
            first.is_some() && first == document.get("rootAttemptId").and_then(Value::as_str)
        }
        LineageCheck::EvidenceDeltasDigestBound => all.iter().all(|attempt| {
            let Some(added) = attempt
                .pointer("/evidenceDelta/added")
                .and_then(Value::as_array)
            else {
                // No delta declared is not a violation: the root attempt has none.
                return true;
            };
            let refs = attempt.get("evidenceRefs").and_then(Value::as_array);
            added.iter().all(|entry| {
                refs.is_some_and(|refs| {
                    refs.iter().any(|reference| {
                        reference.get("evidenceId") == entry.get("evidenceId")
                            && reference.get("contentSha256") == entry.get("contentSha256")
                    })
                })
            })
        }),
    }
}

/// How a retry-lineage certification can be fooled.
///
/// The axis carries WHICH declared check was broken rather than a bare "invalid". Under
/// `all_checks_required` a specimen that trips a different check would still satisfy a coarser
/// assertion, and the suite would certify on a defect it never aimed at.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RetryLineageFailureAxis {
    /// A named structural check fails while the document presents its lineage as complete.
    StructuralCheckFailedUnderClaimedCompleteness(LineageCheck),
}

/// Whether the document presents its lineage as usable.
///
/// Read from `lineageComplete`, which `retry-lineage-input.schema.json` declares and requires —
/// **never** from `lineageValidation`, which the policy declares to be this evaluator's own output.
/// The shipped negative fixture asserts `lineageComplete: true` over a broken chain, so this is
/// exactly where the document's claim and its structure part company.
fn claims_completeness(document: &Value) -> bool {
    document.get("lineageComplete").and_then(Value::as_bool) == Some(true)
}

impl FailureAxis<JpdEvidence> for RetryLineageFailureAxis {
    fn is_defeated_by(&self, evidence: &JpdEvidence) -> bool {
        let document = evidence.document();
        if !claims_completeness(document) {
            return false;
        }
        match self {
            Self::StructuralCheckFailedUnderClaimedCompleteness(check) => {
                failing_checks(document).contains(check)
            }
        }
    }
}

/// The executor `retry-lineage-validation-policy.yaml` declares and did not have.
pub struct RetryLineageGate;

impl EvidenceGate<JpdEvidence> for RetryLineageGate {
    fn id(&self) -> &str {
        // The `evaluatorId` the policy's `success` block names, and the one both shipped fixtures
        // carry in `lineageValidation.evaluator.evaluatorId`.
        "graphhelm-jpd/retry-lineage-validator"
    }

    fn evaluate(&self, evidence: &JpdEvidence) -> Verdict {
        let findings: Vec<String> = failing_checks(evidence.document())
            .into_iter()
            .map(|check| {
                format!(
                    "retry lineage fails the declared check {check:?}: RETRY_LINEAGE_INVALID, \
                     structural_impossibility, not waivable"
                )
            })
            .collect();
        Verdict {
            passed: findings.is_empty(),
            findings,
        }
    }
}

/// Specimens for certifying a retry-lineage gate.
///
/// The one specimen here is the repository's shipped negative fixture ITSELF — the only document in
/// this module that nobody on this lane authored. It is embedded with `include_str!` rather than
/// read at runtime so that a moved path breaks the BUILD: a suite that silently empties itself
/// still certifies, and `certify` cannot tell an empty suite from a gate that caught everything.
///
/// **One specimen is a floor, not a coverage claim.** The repository holds a foreign adversarial
/// document for exactly one of the nine checks; the other eight are exercised in the tests by
/// mutating foreign documents, which keeps the structure someone else's but is weaker, and is
/// named as weaker rather than counted as coverage.
#[must_use]
pub fn retry_lineage_suite() -> Vec<Specimen<JpdEvidence, RetryLineageFailureAxis>> {
    const NEGATIVE: &str = include_str!(
        "../../../extensions/builtin/graphhelm-jpd/fixtures/negative/invalid-retry-lineage.json"
    );
    let orphan: Value = serde_json::from_str(NEGATIVE).expect("shipped negative fixture parses");
    vec![Specimen {
        id: "retry-lineage/orphaned-retry-under-claimed-completeness".to_owned(),
        axis: RetryLineageFailureAxis::StructuralCheckFailedUnderClaimedCompleteness(
            LineageCheck::RetryEdgesAdjacent,
        ),
        evidence: JpdEvidence::RetryLineage(orphan),
    }]
}
