# #211 slice: runtime-side retry lineage validation — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use `superpowers:subagent-driven-development` or
> `superpowers:executing-plans` to implement this plan task-by-task. Steps use checkbox (`- [ ]`)
> syntax for tracking.

**Goal:** Give `retry-lineage-validation-policy.yaml` — a declared contract with **zero executors
today, measured** — a deterministic runtime executor inside `tools/pathogens`, certified against the
repository's existing negative/positive fixture pair.

**Architecture:** A `RetryLineageGate` implementing the existing generic `EvidenceGate<JpdEvidence>`
from the #211 harness. It derives the nine checks the policy declares **from the attempt structure
only**, and is forbidden by construction from reading its own declared output field. Specimens are
produced by **mutating the repository's real fixtures**, never authored from scratch.

**Tech Stack:** Rust (`cargo +1.97.1`, `--locked`), `serde_json::Value`, the `tools/pathogens`
certification harness (`EvidenceGate`, `FailureAxis`, `Specimen`, `certify`).

**Spec:** GitHub issue #211 (deliverable: *"runtime-side validation for … retry lineage"*), and the
declared contract at `extensions/builtin/graphhelm-jpd/evaluators/retry-lineage-validation-policy.yaml`.

---

## Why this slice, and why not the journey contract validator

The routing suggestion offered two candidates and one ordering rule — *start with what unblocks
others* — with the journey contract validator named as the thing N's corpus (#295) consumes via
gate. **I could not verify that.** Measured on `#295`: its body (103 lines) and **all** its comments
mention `journey.contract`, `contract.validator`, `pathogens`, `EvidenceGate`, `CandidateGate` and
`211` **zero times**. It may be true upstream of the record; it is not *in* the record, so I did not
spend the slice on it. Named so it can be corrected rather than silently overridden.

What decided it instead is a property I can measure, and it comes straight out of #294:

| axis | journey contract | **retry lineage** |
|---|---|---|
| schema declaring the vocabulary | 1 | **4** |
| declared policy contract with named checks | — | **9 checks, `all_checks_required`** |
| **real NEGATIVE fixture (not written by me)** | **none** | **`fixtures/negative/invalid-retry-lineage.json`** |
| **real POSITIVE fixture** | **none** | **`fixtures/positive/recovered-retry-chain.json`** |
| runtime executor today | none | none (`runtimeStatus: declarative_only`) |
| Rust readers of the policy name | n/a | **0** (positive control: `retry-classification-policy` 1, `evidence-strength-lattice` 2) |

**The fixture pair is the whole argument.** The #294 defect — a gate that read invented field names
and passed every real document, including the repository's own negative fixture — was caught by
exactly one thing: **a real adversarial document I did not write.** My synthetic specimens all
passed, because a suite written by the same hand as the gate agrees with it about *vocabulary* by
construction. Synthetic specimens can disagree with a gate about logic; never about vocabulary.

Journey contract has **no fixtures at all**. Building it now would mean authoring both the gate and
its adversaries from my own vocabulary — rebuilding the #294 failure mode deliberately, days after
being burned by it. Retry lineage already has the instrument on disk.

*(One correction I owe my own reasoning: `journey-contract` shows 2 Rust "readers", which I nearly
recorded as "already has an oracle, so a second would duplicate it". It does not — both are **test
files** referencing the schema path, not a runtime validator. Same shape, different role. The count
was real and the conclusion it invited was false.)*

## Global Constraints

- Toolchain `cargo +1.97.1`, `--locked` on every invocation.
- **ED-18:** no merge until `cargo check --workspace --all-targets` runs on the **merge result**
  (main + branch), isolated `CARGO_TARGET_DIR`, base sha named, both window ends read from the clock.
- **M06 binding decision 5:** gate machinery and gated code never travel in one branch. This slice is
  gate-machinery-only (`tools/pathogens/**`); the blueprint lands on its own branch and PR.
- **#216 manifest amendment:** `contributions[]` is append-only and shared. This slice adds **no new
  extension files**, so it appends nothing. If that changes, entries land in the same PR as the files,
  each with its `sha256`.
- Documentation in English (`CLAUDE.md`).
- `.gitattributes` on current main sets `*.rs text eol=lf`. On a Windows worktree, merging main shows
  phantom `M` on untouched files with an **empty** `git diff`. Cure: `git checkout --` those files.
  Never commit a renormalisation of someone else's file into this PR.

---

## The load-bearing design decision: the gate may not read its own output

The policy declares:

```yaml
  inputSchema:  .../retry-lineage-input.schema.json
  outputSchema: .../retry-chain-input.schema.json
  outputField:  lineageValidation
```

The three schemas form a strict chain, and this was **measured**, not assumed:

| schema | top-level properties |
|---|---|
| `retry-lineage-input` | `journeyRunId`, `rootAttemptId`, `rootAttempt`, `retries`, `lineageComplete`, `firstFailure`, `successfulAttemptId` — **7, no `lineageValidation`** |
| `retry-chain-input` | those 7 **+ `lineageValidation`** |
| `retry-chain` | those 8 **+ `outcomeClass`, `classification`, `classificationBasis`** |

So the readable set is not a matter of taste — **the input schema defines it**:

- **READABLE (input):** the seven above.
- **FORBIDDEN (this evaluator's own output):** `lineageValidation` — including `.result` and `.status`.
- **FORBIDDEN (downstream consumers of this output):** `outcomeClass`, `classification`,
  `classificationBasis`.

**Why this is the whole ballgame.** In the two real fixtures, `lineageValidation.result` is
`"invalid"` in the negative and `"valid"` in the positive. A gate that reads that one field
separates the two fixtures **perfectly** and is **worth nothing**: it is reading the document's
grade of itself, and the policy declares that field to be this very evaluator's output. It would
pass both fixture arms, look correct in review, and certify nothing — the #294 defect wearing new
clothes. A guard whose expected value is derivable without doing the work is not a guard.

**Closed by observation, not by promise** (Task 4): the same structural document is driven through
the gate twice, once with its self-report set to `valid`/`recovered_success` and once to
`invalid`/`flaky_pass`. **The verdict must be byte-identical.** Any gate that consults the
self-report fails that arm. A comment saying "do not read `lineageValidation`" would mention the
rule; this fails when the rule breaks.

## What the fixtures actually prove — measured, and sealed

Both fixtures were evaluated against all nine checks mechanically before this plan was written:

```
NEGATIVE invalid-retry-lineage.json : 1 of 9 checks fails -> retry_edges_adjacent
    (the orphan retry omits `retryOf`; the other 8 checks PASS)
POSITIVE recovered-retry-chain.json : 0 of 9 checks fail
```

**A clean single-arm signature**, which is what makes the negative fixture a usable instrument.

**SEALED LIMITATION, stated before any code exists.** The repository contains an adversarial document
for **exactly one** of the nine checks. The other **eight are exercised only by specimens I derive
myself**, and derived specimens share my vocabulary. This slice therefore proves:

- `retry_edges_adjacent` — against a **foreign** adversarial document. Strong.
- the other eight — against **mutations of foreign documents**. Weaker, and deliberately weaker in a
  named way: the *structure* stays authored by someone else and only the field under test is moved,
  so the vocabulary cannot drift with mine. **This is a floor, not a coverage claim.**

**What would lift it:** an adversarial fixture per check, contributed to
`extensions/builtin/graphhelm-jpd/fixtures/negative/` by someone who is not me. Declared on the issue,
not built here.

## File Structure

- **Create `tools/pathogens/src/retry_lineage.rs`** — the nine checks, the failure axis, the gate,
  and the specimen suite. One responsibility: deciding whether an attempt structure is sound.
- **Modify `tools/pathogens/src/jpd.rs`** — nothing but a `pub mod` re-export point if needed;
  `JpdEvidence::RetryLineage(Value)` **already exists** and is currently unconstructed.
- **Create `tools/pathogens/tests/retry_lineage_gates.rs`** — the guards, including the two
  real-fixture arms and the anti-mirror arm.

---

### Task 1: The nine checks, named exactly as the policy names them

**Files:**
- Create: `tools/pathogens/src/retry_lineage.rs`
- Modify: `tools/pathogens/src/lib.rs` (add `pub mod retry_lineage;`)
- Test: `tools/pathogens/tests/retry_lineage_gates.rs`

**Interfaces:**
- Consumes: nothing from earlier tasks.
- Produces: `pub enum LineageCheck` (9 variants), `pub fn failing_checks(document: &Value) ->
  Vec<LineageCheck>`.

**Vocabulary is copied from the policy, never invented.** The nine `requiredChecks`, verbatim:
`root_attempt_id_matches`, `attempt_ids_unique`, `ordinals_contiguous`, `retry_edges_adjacent`,
`retry_edges_acyclic`, `timestamps_monotonic`, `successful_attempt_exists_and_succeeded`,
`first_failure_matches_root`, `evidence_deltas_digest_bound`.

- [ ] **Step 1: Write the failing test — the REAL negative fixture, first, before any other test**

```rust
use std::path::{Path, PathBuf};
use pathogens::retry_lineage::{failing_checks, LineageCheck};

fn fixture(kind: &str, name: &str) -> serde_json::Value {
    let path: PathBuf = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../extensions/builtin/graphhelm-jpd/fixtures")
        .join(kind)
        .join(name);
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("fixture {} unreadable: {e}", path.display()));
    serde_json::from_str(&text).expect("fixture parses")
}

#[test]
fn the_repositorys_negative_fixture_fails_exactly_the_adjacency_check() {
    let document = fixture("negative", "invalid-retry-lineage.json");
    assert_eq!(
        failing_checks(&document),
        vec![LineageCheck::RetryEdgesAdjacent],
        "the shipped negative fixture's own summary says the retry omits retryOf; \
         a different set here means the checks do not mean what the policy says"
    );
}

#[test]
fn the_repositorys_positive_fixture_fails_nothing() {
    let document = fixture("positive", "recovered-retry-chain.json");
    assert_eq!(
        failing_checks(&document),
        Vec::<LineageCheck>::new(),
        "a validator that refuses the good document is not strict, it is broken"
    );
}
```

- [ ] **Step 2: Run to verify it fails for the RIGHT reason**

Run: `cargo +1.97.1 test -p pathogens --locked --test retry_lineage_gates`
Expected: FAIL to **compile** — `unresolved import pathogens::retry_lineage`. That is a red in the
harness, not in the assertion. Record it as such: a compile failure is not yet evidence the guard
can fire. The assertion-level red arrives at the end of Step 3's first run.

- [ ] **Step 3: Write the minimal implementation**

```rust
//! #211: the executor for `retry-lineage-validation-policy`, which had none.
//!
//! **This module may not read `lineageValidation`, `outcomeClass`, `classification` or
//! `classificationBasis`.** The policy declares `outputField: lineageValidation`, so that field is
//! this evaluator's OUTPUT; a validator consulting it grades itself. The readable set is exactly
//! the seven top-level properties of `retry-lineage-input.schema.json`. Enforced by observation in
//! `the_verdict_ignores_the_documents_self_report`, not by this comment.

use serde::Serialize;
use serde_json::Value;

/// The nine checks `retry-lineage-validation-policy.yaml` declares, in its own vocabulary.
///
/// A CLOSED set matched exhaustively at every use: the policy says `all_checks_required`, so a
/// check added upstream must break this build rather than be silently skipped.
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
    /// Every check, in policy order. The suite walks this so a new variant cannot be forgotten.
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

/// Every attempt in the document: the root, then the retries, in ordinal order.
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

/// Which of the nine checks this document fails, in policy order.
///
/// Returns a LIST rather than a bool: `all_checks_required` means a caller needs to know WHICH
/// check failed to say anything useful, and a bool would collapse nine distinct structural facts
/// into one — the failure mode where a fixture satisfies an assertion for the wrong reason.
#[must_use]
pub fn failing_checks(document: &Value) -> Vec<LineageCheck> {
    LineageCheck::every()
        .into_iter()
        .filter(|check| !holds(*check, document))
        .collect()
}

fn holds(check: LineageCheck, document: &Value) -> bool {
    let all = attempts(document);
    let ids: Vec<&str> = all.iter().filter_map(|a| attempt_id(a)).collect();
    match check {
        LineageCheck::RootAttemptIdMatches => {
            document.get("rootAttemptId").and_then(Value::as_str)
                == document.get("rootAttempt").and_then(attempt_id)
        }
        LineageCheck::AttemptIdsUnique => {
            let mut seen: Vec<&str> = ids.clone();
            seen.sort_unstable();
            seen.dedup();
            seen.len() == ids.len() && ids.len() == all.len()
        }
        LineageCheck::OrdinalsContiguous => {
            let mut ordinals: Vec<u64> = all
                .iter()
                .filter_map(|a| a.get("ordinal").and_then(Value::as_u64))
                .collect();
            ordinals.sort_unstable();
            ordinals.len() == all.len()
                && ordinals
                    .iter()
                    .enumerate()
                    .all(|(index, ordinal)| *ordinal as usize == index + 1)
        }
        // The check the shipped negative fixture violates: every non-root attempt must name a
        // `retryOf` that exists among the attempt ids. An orphan retry breaks the chain while
        // every other check still passes, which is why this fixture is a clean instrument.
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
            let mut cursor = *start;
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
                    return true;
                };
                cursor = next;
            }
        }),
        LineageCheck::TimestampsMonotonic => {
            let mut ordered: Vec<&Value> = all.clone();
            ordered.sort_by_key(|a| a.get("ordinal").and_then(Value::as_u64).unwrap_or(0));
            let stamps: Vec<(&str, &str)> = ordered
                .iter()
                .filter_map(|a| {
                    Some((
                        a.get("startedAt").and_then(Value::as_str)?,
                        a.get("endedAt").and_then(Value::as_str)?,
                    ))
                })
                .collect();
            stamps.len() == all.len()
                && stamps.iter().all(|(start, end)| start <= end)
                && stamps.windows(2).all(|pair| pair[0].1 <= pair[1].0)
        }
        LineageCheck::SuccessfulAttemptExistsAndSucceeded => document
            .get("successfulAttemptId")
            .and_then(Value::as_str)
            .and_then(|id| all.iter().find(|a| attempt_id(a) == Some(id)))
            .and_then(|a| a.get("result").and_then(Value::as_str))
            == Some("succeeded"),
        LineageCheck::FirstFailureMatchesRoot => {
            document
                .pointer("/firstFailure/attemptId")
                .and_then(Value::as_str)
                == document.get("rootAttemptId").and_then(Value::as_str)
        }
        LineageCheck::EvidenceDeltasDigestBound => all.iter().all(|attempt| {
            let Some(added) = attempt.pointer("/evidenceDelta/added").and_then(Value::as_array)
            else {
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
```

Add to `tools/pathogens/src/lib.rs`, beside the existing `pub mod jpd;`:

```rust
pub mod retry_lineage;
```

- [ ] **Step 4: Run the tests and confirm they pass**

Run: `cargo +1.97.1 test -p pathogens --locked --test retry_lineage_gates`
Expected: 2 passed.

- [ ] **Step 5: Commit BEFORE any sabotage**

```bash
git add tools/pathogens/src/retry_lineage.rs tools/pathogens/src/lib.rs tools/pathogens/tests/retry_lineage_gates.rs
git commit -m "feat(211): the nine declared retry-lineage checks, against the real fixture pair"
```

---

### Task 2: Prove each check can fail on its own

**Files:**
- Modify: `tools/pathogens/tests/retry_lineage_gates.rs`

**Interfaces:**
- Consumes: `failing_checks`, `LineageCheck` from Task 1.
- Produces: `fn mutate(document: &Value, pointer: &str, value: Value) -> Value` — used by Task 3 and 4.

**Why this task exists.** Task 1 proves one check fires. Eight others are currently asserted only by
the positive fixture *not* firing them, and "did not fire" is satisfied by a check that can never
fire at all. An unreddenable branch is the mirror of a guard that was never red.

- [ ] **Step 1: Write the failing test**

```rust
/// Copy a real fixture with one pointer replaced. The STRUCTURE stays foreign; only the field
/// under test moves. This is the compromise the sealed limitation names: it cannot give a check a
/// genuinely foreign adversary, but it stops my vocabulary from replacing the document's.
fn mutate(document: &serde_json::Value, pointer: &str, value: serde_json::Value) -> serde_json::Value {
    let mut copy = document.clone();
    let slot = copy
        .pointer_mut(pointer)
        .unwrap_or_else(|| panic!("pointer {pointer} does not exist in the fixture"));
    *slot = value;
    copy
}

#[test]
fn every_declared_check_can_fail_on_its_own() {
    let good = fixture("positive", "recovered-retry-chain.json");
    // One mutation per check, each chosen to break ONLY that check.
    let cases: Vec<(LineageCheck, serde_json::Value)> = vec![
        (
            LineageCheck::RootAttemptIdMatches,
            mutate(&good, "/rootAttemptId", serde_json::json!("attempt/does-not-exist")),
        ),
        (
            LineageCheck::AttemptIdsUnique,
            mutate(&good, "/retries/0/attemptId", serde_json::json!("attempt/issue-210/001")),
        ),
        (
            LineageCheck::OrdinalsContiguous,
            mutate(&good, "/retries/0/ordinal", serde_json::json!(7)),
        ),
        (
            LineageCheck::RetryEdgesAdjacent,
            mutate(&good, "/retries/0/retryOf", serde_json::json!("attempt/nowhere")),
        ),
        (
            LineageCheck::TimestampsMonotonic,
            mutate(&good, "/retries/0/startedAt", serde_json::json!("2026-08-22T11:00:00Z")),
        ),
        (
            LineageCheck::SuccessfulAttemptExistsAndSucceeded,
            mutate(&good, "/retries/0/result", serde_json::json!("failed")),
        ),
        (
            LineageCheck::FirstFailureMatchesRoot,
            mutate(&good, "/firstFailure/attemptId", serde_json::json!("attempt/issue-210/002")),
        ),
        (
            LineageCheck::EvidenceDeltasDigestBound,
            mutate(
                &good,
                "/retries/0/evidenceDelta/added/0/contentSha256",
                serde_json::json!("9999999999999999999999999999999999999999999999999999999999999999"),
            ),
        ),
    ];

    for (check, document) in &cases {
        let failing = failing_checks(document);
        assert!(
            failing.contains(check),
            "mutation aimed at {check:?} did not make it fail; got {failing:?}"
        );
    }

    // Every check EXCEPT the acyclic one, which needs a two-attempt cycle rather than one edit and
    // is covered by its own test below. Named rather than silently absent.
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

#[test]
fn a_cycle_between_two_attempts_fails_the_acyclic_check() {
    let good = fixture("positive", "recovered-retry-chain.json");
    // Point the root at its own retry: root -> 002 -> root.
    let cyclic = mutate(&good, "/rootAttempt/retryOf", serde_json::json!("attempt/issue-210/002"));
    assert!(
        failing_checks(&cyclic).contains(&LineageCheck::RetryEdgesAcyclic),
        "a two-attempt cycle must fail the acyclic check"
    );
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo +1.97.1 test -p pathogens --locked --test retry_lineage_gates`
Expected: the two new tests FAIL at their assertions if any check is unreddenable. **Record which.**
A check that cannot be made to fail by any single-field mutation is a finding about the check, not
about the test — fix the check, do not weaken the mutation.

- [ ] **Step 3: Fix whichever checks proved unreddenable**

No code is written speculatively here: run Step 2 first, and repair only what it names.

- [ ] **Step 4: Run to verify all pass**

Run: `cargo +1.97.1 test -p pathogens --locked --test retry_lineage_gates`
Expected: 4 passed.

- [ ] **Step 5: Commit**

```bash
git add tools/pathogens/tests/retry_lineage_gates.rs tools/pathogens/src/retry_lineage.rs
git commit -m "test(211): one mutation per declared check, so no check is unreddenable"
```

---

### Task 3: The gate and its axis

**Files:**
- Modify: `tools/pathogens/src/retry_lineage.rs`
- Modify: `tools/pathogens/tests/retry_lineage_gates.rs`

**Interfaces:**
- Consumes: `failing_checks`, `LineageCheck`; `EvidenceGate`, `FailureAxis`, `Specimen`, `Verdict`
  from `pathogens`; `JpdEvidence::RetryLineage(Value)` from `pathogens::jpd` (**already exists,
  currently unconstructed**).
- Produces: `pub enum RetryLineageFailureAxis`, `pub struct RetryLineageGate`,
  `pub fn retry_lineage_suite() -> Vec<Specimen<JpdEvidence, RetryLineageFailureAxis>>`.

- [ ] **Step 1: Write the failing test**

```rust
use pathogens::jpd::JpdEvidence;
use pathogens::retry_lineage::{retry_lineage_suite, RetryLineageFailureAxis, RetryLineageGate};
use pathogens::{certify, is_defeated_on_its_axis, EvidenceGate};

#[test]
fn the_gate_refuses_the_repositorys_negative_fixture_and_accepts_the_positive() {
    let gate = RetryLineageGate;
    let bad = JpdEvidence::RetryLineage(fixture("negative", "invalid-retry-lineage.json"));
    let good = JpdEvidence::RetryLineage(fixture("positive", "recovered-retry-chain.json"));

    assert!(!gate.evaluate(&bad).passed, "the shipped negative fixture must be refused");
    assert!(gate.evaluate(&good).passed, "the shipped positive fixture must be accepted");
}

#[test]
fn every_specimen_defeats_the_axis_it_names() {
    for specimen in &retry_lineage_suite() {
        assert!(
            is_defeated_on_its_axis(specimen),
            "specimen {} does not actually defeat {:?}; a specimen that defeats nothing \
             certifies a gate that caught nothing",
            specimen.id,
            specimen.axis
        );
    }
}

#[test]
fn the_gate_is_certified_by_the_suite() {
    let certification = certify(&RetryLineageGate, &retry_lineage_suite())
        .expect("the gate must reject every specimen");
    // `Certification::specimens` is a `u32`; `len()` is a `usize`. Cast at the comparison rather
    // than storing the count differently — the harness field is not this slice's to change.
    assert_eq!(certification.specimens as usize, retry_lineage_suite().len());
    assert_eq!(certification.gate_id, "graphhelm-jpd/retry-lineage-validator");
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo +1.97.1 test -p pathogens --locked --test retry_lineage_gates`
Expected: FAIL to compile — `RetryLineageGate` not found.

- [ ] **Step 3: Write the minimal implementation**

Append to `tools/pathogens/src/retry_lineage.rs`:

```rust
use crate::jpd::JpdEvidence;
use crate::{EvidenceGate, FailureAxis, Specimen, Verdict};

/// How a retry-lineage certification can be fooled.
///
/// The axis carries WHICH declared check was broken rather than a bare "invalid": with
/// `all_checks_required`, a specimen that trips the wrong check would still satisfy a coarser
/// assertion, and the suite would certify on a defect it never aimed at.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RetryLineageFailureAxis {
    /// A named structural check fails while the document presents the lineage as complete.
    StructuralCheckFailedUnderClaimedCompleteness(LineageCheck),
}

/// Whether the document presents its lineage as usable.
///
/// Read from `lineageComplete`, which the INPUT schema declares — **never** from
/// `lineageValidation`, which the policy declares to be this evaluator's own output.
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
        "graphhelm-jpd/retry-lineage-validator"
    }

    fn evaluate(&self, evidence: &JpdEvidence) -> Verdict {
        let document = evidence.document();
        let findings: Vec<String> = failing_checks(document)
            .into_iter()
            .map(|check| {
                format!(
                    "retry lineage fails the declared check {check:?}: \
                     RETRY_LINEAGE_INVALID, structural_impossibility, not waivable"
                )
            })
            .collect();
        Verdict {
            passed: findings.is_empty(),
            findings,
        }
    }
}

/// Specimens derived by mutating the repository's real fixtures.
///
/// The first is the shipped negative fixture ITSELF — the one document here that no one on this
/// lane authored. The rest move exactly one field of a foreign document.
#[must_use]
pub fn retry_lineage_suite() -> Vec<Specimen<JpdEvidence, RetryLineageFailureAxis>> {
    // Loaded at compile time so the suite cannot silently empty itself if a path moves.
    const NEGATIVE: &str = include_str!(
        "../../../extensions/builtin/graphhelm-jpd/fixtures/negative/invalid-retry-lineage.json"
    );
    let orphan: Value = serde_json::from_str(NEGATIVE).expect("shipped fixture parses");
    vec![Specimen {
        id: "retry-lineage/orphaned-retry-under-claimed-completeness".to_owned(),
        axis: RetryLineageFailureAxis::StructuralCheckFailedUnderClaimedCompleteness(
            LineageCheck::RetryEdgesAdjacent,
        ),
        evidence: JpdEvidence::RetryLineage(orphan),
    }]
}
```

- [ ] **Step 4: Run to verify it passes**

Run: `cargo +1.97.1 test -p pathogens --locked --test retry_lineage_gates`
Expected: 7 passed.

- [ ] **Step 5: Commit**

```bash
git add tools/pathogens/src/retry_lineage.rs tools/pathogens/tests/retry_lineage_gates.rs
git commit -m "feat(211): the retry lineage gate, certified by the shipped negative fixture"
```

---

### Task 4: The anti-mirror arm — the one that kills the whole class

**Files:**
- Modify: `tools/pathogens/tests/retry_lineage_gates.rs`

**Interfaces:**
- Consumes: `RetryLineageGate`, `mutate`, `fixture`.
- Produces: nothing. This task adds only a guard.

**This is the most valuable test in the slice.** Everything else proves the gate computes the right
answer. This proves it computes the answer *from the right place*. The #294 gate also produced
plausible verdicts — from invented fields.

- [ ] **Step 1: Write the failing test**

```rust
/// The verdict must not move when the document's own grade of itself moves.
///
/// `lineageValidation` is this evaluator's declared OUTPUT (`outputField` in the policy), and
/// `outcomeClass` belongs to the classifier downstream. A gate reading either is grading itself,
/// and in the two shipped fixtures those fields separate good from bad PERFECTLY — so a mirror-gate
/// passes every other test in this file. Only this one can tell the difference.
#[test]
fn the_verdict_ignores_the_documents_self_report() {
    let gate = RetryLineageGate;

    // The broken document, relabelled to claim it is fine.
    let bad = fixture("negative", "invalid-retry-lineage.json");
    let bad_claiming_valid = mutate(&bad, "/lineageValidation/result", serde_json::json!("valid"));
    let bad_claiming_valid =
        mutate(&bad_claiming_valid, "/outcomeClass", serde_json::json!("recovered_success"));

    // The sound document, relabelled to claim it is broken.
    let good = fixture("positive", "recovered-retry-chain.json");
    let good_claiming_invalid =
        mutate(&good, "/lineageValidation/result", serde_json::json!("invalid"));
    let good_claiming_invalid =
        mutate(&good_claiming_invalid, "/outcomeClass", serde_json::json!("flaky_pass"));

    assert!(
        !gate.evaluate(&JpdEvidence::RetryLineage(bad_claiming_valid)).passed,
        "a broken chain that calls itself valid must still be refused: \
         the gate is reading its own output instead of the attempt structure"
    );
    assert!(
        gate.evaluate(&JpdEvidence::RetryLineage(good_claiming_invalid)).passed,
        "a sound chain that calls itself invalid must still be accepted: \
         refusing on the self-report looks conservative and is the same defect mirrored"
    );
}
```

- [ ] **Step 2: Run to verify it passes, then PROVE it can fail**

Run: `cargo +1.97.1 test -p pathogens --locked --test retry_lineage_gates`
Expected: PASS. **A pass here is not yet evidence** — this guard's whole value is catching a gate
that reads the self-report, and the current gate does not. Sabotage it in Step 3.

- [ ] **Step 3: Sabotage — commit FIRST, then edit**

Confirm the tree is committed, then temporarily make `evaluate` consult the self-report:

```rust
// SABOTAGE, to be reverted: the mirror-gate the anti-mirror arm exists to catch.
let findings: Vec<String> = if document.pointer("/lineageValidation/result").and_then(Value::as_str)
    == Some("invalid")
{
    vec!["self-reported invalid".to_owned()]
} else {
    Vec::new()
};
```

Run the suite. **Expected: `the_verdict_ignores_the_documents_self_report` FAILS, and the two
real-fixture arms still PASS.** That asymmetry is the evidence: the mirror-gate satisfies every
fixture-based test and only this arm sees it. Record the exact panic site. Then `git checkout --
tools/pathogens/src/retry_lineage.rs` and confirm green again.

- [ ] **Step 4: Record the mutation result in the PR body**

Both arms, with counts, and the note that a bare `^error` match would misread a genuine red as a
build failure — match `^error[E` / `could not compile` instead.

- [ ] **Step 5: Commit**

```bash
git add tools/pathogens/tests/retry_lineage_gates.rs
git commit -m "test(211): the verdict must not move when the document's self-report moves"
```

---

## Death conditions — how this plan can be shown wrong

| claim | dies when |
|---|---|
| The negative fixture fails exactly `retry_edges_adjacent` | any other check appears in `failing_checks` for it — measured mechanically before writing this plan, re-measured by Task 1 |
| `lineageValidation` is this evaluator's output | the policy's `outputField` changes, or `retry-lineage-input.schema.json` gains the property |
| The nine checks are the whole contract | `requiredChecks` gains a tenth — `LineageCheck::every()` and the exhaustive `match` both break the build, deliberately |
| Eight checks lack a foreign adversary | someone contributes negative fixtures; the seal is then lifted **by name**, not quietly |
| `journey-contract` has no runtime validator | a non-test Rust reader appears |

## What this slice does NOT do — named, not left to be discovered

- **No schema validation.** A malformed document reaches the gate. The JPD schemas are the authority
  and a second Rust declaration of one vocabulary is a second producer that disagrees with itself.
  Belongs with whoever wires a public surface.
- **No CLI / API / MCP surface.** `retry_lineage_suite()` is a library value; nothing runs it outside
  the tests.
- **No classification.** `outcomeClass` and `classification` are the *next* policy's job
  (`retry-classification-policy`, which already has a reader). This slice stops at structure, exactly
  where the policy's stated purpose stops: *"before any outcome classification is attempted."*
- **Three validators still remain** after this one: journey contract, observation obligation, council
  result. Two policies still have no executor: `assurance-tier-policy`, `council-selection-policy`.
- **One specimen is a floor, not a coverage claim.**
