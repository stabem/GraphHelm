# Generic Deterministic Certification Gate — Implementation Plan (#211)

> **For agentic workers:** REQUIRED SUB-SKILL: use `superpowers:subagent-driven-development` or
> `superpowers:executing-plans` to implement task-by-task. Steps use checkbox (`- [ ]`) syntax.

**Goal:** Let JPD validators earn and hold a thymus certification through the certification harness
that already exists, so typed JPD evidence is judged by the same rule that judges rendered geometry.

**Architecture:** `tools/pathogens` already implements the whole certification rule — a gate is
certified only by rejecting *every* specimen, the suite's digest voids stale certifications when it
grows, and there are anti-triviality controls. What it cannot do is judge anything that is not a
`Deliverable`. This plan makes the harness generic over the evidence it judges and over the axis a
specimen defeats, then adds JPD evidence as a second instantiation. **The certification rule itself is
not touched** — it never referenced the payload's shape, only whether a verdict passed.

**Tech Stack:** Rust 1.97.1, `serde`/`serde_json`, `sha2`/`hex`, workspace crates
`tools/pathogens`, `core/quality`, `core/protocols`, `apps/cli`.

**Spec:** issue #211, plus the interface published at
[#211 comment 5400966615](https://github.com/stabem/GraphHelm/issues/211#issuecomment-5400966615).

## Bases — every claim below was measured, not recalled

| base | ref | how |
|---|---|---|
| repository | `origin/main` @ `f4c9501` | `git show <ref>:<path>`, so the command contains the ref |

Line numbers are lines **in that blob**. After a wrong citation earlier today, every number here was
re-measured against the ref immediately before writing.

## What already exists — this changes what #211 IS

| piece | where | already does |
|---|---|---|
| `CandidateGate` | `tools/pathogens/src/lib.rs:190` | `id()` + `evaluate(&Deliverable) -> Verdict` |
| `certify` | `:320` | `Ok(Certification)` only if the gate rejects **every** specimen; else `Err(CertificationRefusal { fooled_by })` |
| `suite_digest` | `:310` | sha256 over canonical JSON of the specimen list |
| `certification_is_current` | `:346` | stale immunity dies **by comparison, never by cleanup** |
| anti-triviality | `:370`, `:460`, `:491` | `paired_trivial_gate`, `reject_everything_gate`, `is_useless_on_its_axis` |
| `Specimen` | `:154` | `{ id: String, mode: UselessnessMode, deliverable: Deliverable }` |
| `Verdict` | `:166` | `{ passed: bool, findings: Vec<String> }` |
| `GateFinding` / `GateVerdict` / `GateCertified` | `core/protocols/src/event.rs:1017` / `:1034` / `:1050` | the wire diagnostics and the thymus receipt |
| a certification **command** | `apps/cli/src/commands/quality.rs:37-139` | `GeometryGate: CandidateGate`, calls `certify`, renders a `Certification` |
| suite digest over HTTP | `apps/cli/src/commands/serve/routes.rs:994` | already exposed |

**So #211 is not "build a gate". It is "make the existing one generic".** The word doing the work in
the issue title is `generic`.

### The two obstacles, both measured

**1. `evaluate` takes `&Deliverable`** — a geometry/HTML shape (claims, `html`, `reachable_ids`,
journey steps, test cases, diff summary). JPD evidence is typed JSON artifacts. A JPD validator
cannot be a `CandidateGate` today.

**2. `Specimen.mode` is `UselessnessMode`, a CLOSED enum of UI failure axes** — `DeadFeature`,
`UnreachableUi`, `BlankScreen`, `OrphanView`, `GuttedAssertion`, `HappyPathOnly`,
`MinimalDiffNoBehavior`, and siblings. JPD's axes are different in kind: missing observer, flaky
counted as proven, self-validation, forged waiver.

**Adding JPD variants to `UselessnessMode` is the wrong repair** and it is the tempting one. It fuses
two domains into one closed set, and thereafter every exhaustive match over that enum has arms it
cannot mean — the failure is silent because the enum still compiles everywhere. **The axis is
generic, exactly like the evidence.**

### One defect found while measuring, small and worth fixing in passing

`Verdict`'s doc at `:164` says it is *"the same refusal-with-findings shape the `GateVerdict` kind
carries on the wire"*. **It is not.** `Verdict.findings` is `Vec<String>`; `GateVerdict.findings` is
`Vec<GateFinding>` with `severity`, `claim`, `evidence`, `remediation`. A comment is a claim about the
code, and this one overstates. Task 4 makes the sentence true rather than deleting it.

## Global Constraints

- Toolchain `cargo +1.97.1`, `--locked` on every invocation.
- **ED-18:** no code PR merges until `cargo check --workspace --all-targets` runs on the **merge
  result**, under check-tier discipline (isolated `CARGO_TARGET_DIR`, both window ends read from the
  clock, base sha named).
- `clippy` clean for every crate touched, on the merge result.
- **No provider, browser, network, or production dependency in repository tests** (issue invariant).
- **Only the Graph Governor publishes operational mutations** (issue invariant).
- **Agent agreement is advisory** (issue invariant) — enforced by construction in Task 3, not by
  convention.
- `tools/pathogens` stays pure: `serde`, `serde_json`, `sha2`, `hex`. No clock, no entropy, no IO.
- Anything landed in the development-contracts extension package needs its own schema plus manifest
  entries with `sha256` in the same PR (#216 amendment). **Tasks 1–4 add nothing to that package.**

## File Structure

| file | responsibility |
|---|---|
| `tools/pathogens/src/lib.rs` (modify) | the harness, made generic over evidence and axis; geometry aliases keep existing call sites readable |
| `tools/pathogens/src/jpd.rs` (create) | JPD evidence enum and its failure axis — a second instantiation, no geometry knowledge |
| `tools/pathogens/tests/generic_harness.rs` (create) | the harness is genuinely generic and the rule is unchanged |
| `tools/pathogens/tests/jpd_gates.rs` (create) | each JPD validator earns certification, and each is refused when hollowed |
| `core/protocols/src/event.rs` (read only) | `GateFinding` is reused, never re-declared |
| `core/quality/tests/thymus.rs` (modify) | migrated to the aliases; **its assertions do not change** |
| `apps/cli/src/commands/quality.rs` (modify) | migrated to the aliases; behaviour unchanged in this plan |

**Blast radius, measured:** seven call sites across three files outside `pathogens`
(`apps/cli/src/commands/quality.rs` ×6, `serve/routes.rs` ×1, `core/quality/tests/thymus.rs` ×3,
`tools/pathogens/tests/*` internal). Small enough that paying the signature change beats the
alternative, which is a parallel typed-evidence gate reusing `certify`'s *rule* — **duplicating an
ORACLE, which diverges in silence and quietly changes what "certified" means.**

---

### Task 1: Make the harness generic, with the geometry suite unchanged

**Files:**
- Modify: `tools/pathogens/src/lib.rs:154-200` (`Specimen`, `CandidateGate`), `:310-345`
  (`suite_digest`, `certify`)
- Test: `tools/pathogens/tests/generic_harness.rs` (create)

**Interfaces:**
- Produces:
  - `trait CandidateGate { type Evidence; fn id(&self) -> &str; fn evaluate(&self, evidence: &Self::Evidence) -> Verdict; }`
  - `struct Specimen<E, A> { pub id: String, pub axis: A, pub evidence: E }`
  - `type GeometrySpecimen = Specimen<Deliverable, UselessnessMode>`
  - `fn suite_digest<E: Serialize, A: Serialize>(suite: &[Specimen<E, A>]) -> String`
  - `fn certify<G, A>(gate: &G, suite: &[Specimen<G::Evidence, A>]) -> Result<Certification, CertificationRefusal> where G: CandidateGate + ?Sized, G::Evidence: Serialize, A: Serialize`
- Consumes: nothing new.

- [ ] **Step 1: Write the failing test — a gate over evidence that is not a `Deliverable`**

```rust
//! The harness must judge evidence it was not written for, or "generic" is a word in a title.

use pathogens::{CandidateGate, Specimen, Verdict, certify};
use serde::Serialize;

#[derive(Serialize)]
struct Note { text: String }

#[derive(Serialize)]
enum NoteAxis { Empty }

struct RejectsEmptyNotes;

impl CandidateGate for RejectsEmptyNotes {
    type Evidence = Note;
    fn id(&self) -> &str { "gate/rejects-empty-notes" }
    fn evaluate(&self, note: &Note) -> Verdict {
        if note.text.is_empty() {
            Verdict { passed: false, findings: vec!["the note is empty".to_owned()] }
        } else {
            Verdict { passed: true, findings: Vec::new() }
        }
    }
}

#[test]
fn a_gate_can_be_certified_over_evidence_that_is_not_a_deliverable() {
    let suite = vec![Specimen {
        id: "empty-note".to_owned(),
        axis: NoteAxis::Empty,
        evidence: Note { text: String::new() },
    }];

    let certification = certify(&RejectsEmptyNotes, &suite).expect("the gate rejects the specimen");

    assert_eq!(certification.gate_id, "gate/rejects-empty-notes");
    assert_eq!(certification.specimens, 1);
}
```

- [ ] **Step 2: Run it and watch it fail for the right reason**

Run: `CARGO_TARGET_DIR=D:/gh-check/h/211 cargo +1.97.1 test -p pathogens --test generic_harness --locked`
Expected: **compile error** — `Specimen` takes no type parameters, `CandidateGate` has no associated
type. That is the failure this test exists to produce.

- [ ] **Step 3: Make it generic**

```rust
pub struct Specimen<E, A> {
    /// Stable id, part of the canonical digest.
    pub id: String,
    /// The axis this specimen defeats. Generic because a JPD failure axis and a UI
    /// uselessness axis are different KINDS of thing: fusing them into one closed enum
    /// gives every exhaustive match arms it cannot mean, and that failure is silent
    /// because the enum still compiles everywhere.
    pub axis: A,
    /// The evidence a candidate gate is shown.
    pub evidence: E,
}

/// The geometry suite's instantiation. Existing call sites read unchanged.
pub type GeometrySpecimen = Specimen<Deliverable, UselessnessMode>;

pub trait CandidateGate {
    /// What this gate judges.
    type Evidence;
    /// The gate's stable id — what `GateCertified` names.
    fn id(&self) -> &str;
    /// Evaluate one piece of evidence.
    fn evaluate(&self, evidence: &Self::Evidence) -> Verdict;
}
```

and the two functions, whose **bodies do not change** — only their signatures:

```rust
#[must_use]
pub fn suite_digest<E: Serialize, A: Serialize>(suite: &[Specimen<E, A>]) -> String {
    let canonical =
        serde_json::to_string(suite).expect("specimens are plain data and always serialize");
    format!("sha256:{}", hex::encode(Sha256::digest(canonical)))
}

pub fn certify<G, A>(
    gate: &G,
    suite: &[Specimen<G::Evidence, A>],
) -> Result<Certification, CertificationRefusal>
where
    G: CandidateGate + ?Sized,
    G::Evidence: Serialize,
    A: Serialize,
{
    let fooled_by: Vec<String> = suite
        .iter()
        .filter(|specimen| gate.evaluate(&specimen.evidence).passed)
        .map(|specimen| specimen.id.clone())
        .collect();
    if fooled_by.is_empty() {
        Ok(Certification {
            gate_id: gate.id().to_owned(),
            suite_digest: suite_digest(suite),
            specimens: u32::try_from(suite.len()).expect("suites are small"),
        })
    } else {
        Err(CertificationRefusal { gate_id: gate.id().to_owned(), fooled_by })
    }
}
```

Add `#[derive(Serialize)]` to `Specimen` bounded on `E: Serialize, A: Serialize`, and rename the
field `deliverable` to `evidence` at the geometry call sites.

- [ ] **Step 4: Run it and watch it pass, with the geometry suite still green**

Run: `CARGO_TARGET_DIR=D:/gh-check/h/211 cargo +1.97.1 test -p pathogens --locked`
Expected: PASS, and `tests/mold.rs` and `tests/thymus.rs` **unchanged in outcome**.

- [ ] **Step 5: Migrate the three external call sites**

`core/quality/tests/thymus.rs`, `apps/cli/src/commands/quality.rs`,
`apps/cli/src/commands/serve/routes.rs`: `impl CandidateGate for X` gains
`type Evidence = Deliverable;`, and `specimen.deliverable` becomes `specimen.evidence`.
**No assertion in `thymus.rs` changes.** If one needs to, stop: the migration altered behaviour and
that is a different task.

- [ ] **Step 6: Prove the rule survived the generalisation**

```rust
/// The certification RULE must be exactly what it was: any pass anywhere refuses the gate.
#[test]
fn one_specimen_slipping_through_still_refuses_the_whole_certification() {
    struct PassesEverything;
    impl CandidateGate for PassesEverything {
        type Evidence = Note;
        fn id(&self) -> &str { "gate/passes-everything" }
        fn evaluate(&self, _: &Note) -> Verdict { Verdict { passed: true, findings: Vec::new() } }
    }

    let suite = vec![Specimen {
        id: "empty-note".to_owned(),
        axis: NoteAxis::Empty,
        evidence: Note { text: String::new() },
    }];

    let refusal = certify(&PassesEverything, &suite).expect_err("a gate fooled once is not certified");
    assert_eq!(refusal.fooled_by, vec!["empty-note".to_owned()],
        "the refusal must NAME which specimen got through, not merely that one did");
}
```

- [ ] **Step 7: Commit**

```bash
git add tools/pathogens/src/lib.rs tools/pathogens/tests/generic_harness.rs core/quality/tests/thymus.rs apps/cli/src/commands/quality.rs apps/cli/src/commands/serve/routes.rs
git commit -m "refactor(211): make the certification harness generic over evidence and axis"
```

---

### Task 2: JPD evidence and its own failure axis

**Files:**
- Create: `tools/pathogens/src/jpd.rs`
- Modify: `tools/pathogens/src/lib.rs` (add `pub mod jpd;`)
- Test: `tools/pathogens/tests/jpd_gates.rs` (create)

**Interfaces:**
- Consumes: `Specimen`, `CandidateGate`, `Verdict`, `certify` from Task 1.
- Produces:
  - `enum JpdEvidence { JourneyContract(Value), ObservationObligation(Value), RetryLineage(Value), CouncilResult(Value), VerificationResult(Value) }`
  - `enum JpdFailureAxis { MissingObserver, FlakyCountedAsProven, SelfValidation, ForgedWaiver, EvidenceDeleted }`
  - `type JpdSpecimen = Specimen<JpdEvidence, JpdFailureAxis>`

- [ ] **Step 1: Write the failing test — the axis is a CLOSED set with no UI arms in it**

```rust
use pathogens::jpd::{JpdEvidence, JpdFailureAxis, JpdSpecimen};
use serde_json::json;

/// The two axes are different KINDS. This pins that they did not get fused: a JPD specimen
/// cannot be built with a UI uselessness axis, and the compiler is what says so.
#[test]
fn a_jpd_specimen_carries_a_jpd_axis() {
    let specimen: JpdSpecimen = JpdSpecimen {
        id: "missing-observer".to_owned(),
        axis: JpdFailureAxis::MissingObserver,
        evidence: JpdEvidence::VerificationResult(json!({ "status": "passed" })),
    };
    assert_eq!(specimen.id, "missing-observer");
}
```

- [ ] **Step 2: Run it and watch it fail**

Run: `CARGO_TARGET_DIR=D:/gh-check/h/211 cargo +1.97.1 test -p pathogens --test jpd_gates --locked`
Expected: compile error, `unresolved import pathogens::jpd`.

- [ ] **Step 3: Write the module**

```rust
//! JPD typed evidence, as a second instantiation of the harness.
//!
//! Evidence is carried as validated JSON rather than as re-declared Rust structs: the
//! schemas in `extensions/builtin/graphhelm-jpd/schemas/` are the authority, and a second
//! Rust declaration of the same closed shapes is a second producer of one vocabulary —
//! which drifts in silence, because a rename preserves a count and everything still
//! compiles.

use serde::Serialize;
use serde_json::Value;

/// The five artifact kinds #211 names for runtime-side validation.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum JpdEvidence {
    JourneyContract(Value),
    ObservationObligation(Value),
    RetryLineage(Value),
    CouncilResult(Value),
    VerificationResult(Value),
}

/// How a JPD certification can be fooled. A CLOSED set, and deliberately NOT merged into
/// `UselessnessMode`: these are failures of evidence, those are failures of a rendered
/// surface, and one enum covering both gives every exhaustive match arms it cannot mean.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum JpdFailureAxis {
    /// A required observer capability is absent and the result claims success anyway.
    MissingObserver,
    /// A run that only passed on retry, presented as proven.
    FlakyCountedAsProven,
    /// The producer of the work is also its validator.
    SelfValidation,
    /// A waiver without the complete authorised record behind it.
    ForgedWaiver,
    /// Evidence referenced by a result no longer exists.
    EvidenceDeleted,
}

/// A JPD specimen: typed evidence plus the axis it defeats.
pub type JpdSpecimen = crate::Specimen<JpdEvidence, JpdFailureAxis>;
```

- [ ] **Step 4: Run it and watch it pass**

Run: `CARGO_TARGET_DIR=D:/gh-check/h/211 cargo +1.97.1 test -p pathogens --test jpd_gates --locked`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add tools/pathogens/src/jpd.rs tools/pathogens/src/lib.rs tools/pathogens/tests/jpd_gates.rs
git commit -m "feat(211): JPD typed evidence and its own closed failure axis"
```

---

### Task 3: The first validator, and the invariant that agreement is advisory

**Files:**
- Modify: `tools/pathogens/src/jpd.rs`
- Test: `tools/pathogens/tests/jpd_gates.rs`

**Interfaces:**
- Consumes: Task 2's types.
- Produces: `struct VerificationResultGate;` implementing `CandidateGate<Evidence = JpdEvidence>`
  with `id() == "gate/jpd-verification-result"`, and `fn jpd_suite() -> Vec<JpdSpecimen>`.

- [ ] **Step 1: Write the failing test — three specimens the gate must reject**

```rust
use pathogens::jpd::{JpdEvidence, JpdFailureAxis, JpdSpecimen, VerificationResultGate, jpd_suite};
use pathogens::certify;
use serde_json::json;

#[test]
fn the_verification_gate_rejects_every_specimen_in_its_suite() {
    let certification =
        certify(&VerificationResultGate, &jpd_suite()).expect("the gate rejects all specimens");
    assert_eq!(certification.gate_id, "gate/jpd-verification-result");
    assert!(certification.specimens >= 3, "the suite must exercise more than one axis");
}

/// Agreement is ADVISORY. A council that unanimously blesses a result with a missing
/// observer must not move the verdict, and this is the specimen that attacks it directly.
#[test]
fn unanimous_agreement_cannot_rescue_a_missing_observer() {
    let blessed = JpdEvidence::VerificationResult(json!({
        "status": "passed",
        "observers": [],
        "council": { "agreement": "unanimous", "verdict": "approve" }
    }));

    let verdict = pathogens::CandidateGate::evaluate(&VerificationResultGate, &blessed);

    assert!(!verdict.passed,
        "consensus is evidence, never authority: a unanimous council over a missing observer \
         must still refuse");
    assert!(verdict.findings.iter().any(|f| f.contains("observer")),
        "the refusal must name the missing observer, not merely refuse");
}
```

- [ ] **Step 2: Run it and watch it fail**

Run: `CARGO_TARGET_DIR=D:/gh-check/h/211 cargo +1.97.1 test -p pathogens --test jpd_gates --locked`
Expected: compile error — `VerificationResultGate` and `jpd_suite` do not exist.

- [ ] **Step 3: Write the gate and its suite**

```rust
use crate::{CandidateGate, Verdict};

/// Validates a Journey Verification Result without any LLM or provider.
pub struct VerificationResultGate;

impl CandidateGate for VerificationResultGate {
    type Evidence = JpdEvidence;

    fn id(&self) -> &str {
        "gate/jpd-verification-result"
    }

    fn evaluate(&self, evidence: &JpdEvidence) -> Verdict {
        let JpdEvidence::VerificationResult(document) = evidence else {
            return Verdict {
                passed: false,
                findings: vec!["this gate judges verification results only".to_owned()],
            };
        };

        let claims_success = document.get("status").and_then(Value::as_str) == Some("passed");
        if !claims_success {
            // Nothing to protect: the document does not claim success.
            return Verdict { passed: true, findings: Vec::new() };
        }

        let mut findings = Vec::new();

        // The council is read for NOTHING. It is deliberately not consulted here: agreement
        // is advisory, and the only way to keep it advisory is to give it no code path.
        let observers = document
            .get("observers")
            .and_then(Value::as_array)
            .map_or(0, Vec::len);
        if observers == 0 {
            findings.push(
                "a result claiming success carries no observer capability: missing observer"
                    .to_owned(),
            );
        }

        if document.get("attempts").and_then(Value::as_u64).unwrap_or(1) > 1
            && document.get("flakyAccepted").and_then(Value::as_bool) != Some(true)
        {
            findings.push("success reached only on retry is not proven success".to_owned());
        }

        if document.get("producer") == document.get("validator")
            && document.get("producer").is_some()
        {
            findings.push("the producer of the work is also its validator".to_owned());
        }

        Verdict { passed: findings.is_empty(), findings }
    }
}

/// The JPD pathogen suite. Growing it changes `suite_digest`, which voids every stale
/// certification by comparison rather than by cleanup.
#[must_use]
pub fn jpd_suite() -> Vec<JpdSpecimen> {
    vec![
        JpdSpecimen {
            id: "verification/missing-observer".to_owned(),
            axis: JpdFailureAxis::MissingObserver,
            evidence: JpdEvidence::VerificationResult(serde_json::json!({
                "status": "passed",
                "observers": []
            })),
        },
        JpdSpecimen {
            id: "verification/flaky-as-proven".to_owned(),
            axis: JpdFailureAxis::FlakyCountedAsProven,
            evidence: JpdEvidence::VerificationResult(serde_json::json!({
                "status": "passed",
                "observers": [{ "capability": "first_party_deterministic" }],
                "attempts": 3
            })),
        },
        JpdSpecimen {
            id: "verification/self-validated".to_owned(),
            axis: JpdFailureAxis::SelfValidation,
            evidence: JpdEvidence::VerificationResult(serde_json::json!({
                "status": "passed",
                "observers": [{ "capability": "first_party_deterministic" }],
                "producer": "agent-a",
                "validator": "agent-a"
            })),
        },
    ]
}
```

- [ ] **Step 4: Run it and watch it pass**

Run: `CARGO_TARGET_DIR=D:/gh-check/h/211 cargo +1.97.1 test -p pathogens --test jpd_gates --locked`
Expected: PASS, both tests.

- [ ] **Step 5: Mutation check — each specimen must be caught by its OWN check**

Apply each mutation to a **committed** tree, run, revert with `git checkout` against that commit.

| mutation | must redden |
|---|---|
| M1 delete the `observers == 0` block | only the missing-observer path; `certify` refuses naming `verification/missing-observer` |
| M2 delete the `attempts > 1` block | only `verification/flaky-as-proven` |
| M3 delete the producer/validator block | only `verification/self-validated` |
| M4 read `council.agreement` and pass when unanimous | `unanimous_agreement_cannot_rescue_a_missing_observer` |

**A mutation reddening two rows means those checks share an assertion and must be split.** Record the
observed outcomes in the test file, not only in the commit message — that is where the next reader
looks.

- [ ] **Step 6: Commit**

```bash
git add tools/pathogens/src/jpd.rs tools/pathogens/tests/jpd_gates.rs
git commit -m "feat(211): the verification-result gate, with agreement advisory by construction"
```

---

### Task 4: Stable diagnostics — make the comment true

**Files:**
- Modify: `tools/pathogens/src/lib.rs:164` (the `Verdict` doc), and add the bridge
- Test: `tools/pathogens/tests/generic_harness.rs`

**Interfaces:**
- Produces: `impl Verdict { pub fn findings_as_gate_findings(&self, remediation: &str) -> Vec<GateFinding> }`
  — or, if `pathogens` may not depend on `core/protocols`, the bridge lives in
  `apps/cli/src/commands/quality.rs` instead. **Decide by measuring the dependency direction first**;
  `tools/pathogens` currently depends on neither.

- [ ] **Step 1: Measure before deciding**

Run: `git show origin/main:tools/pathogens/Cargo.toml`
If adding `graphhelm-protocols` would make a tool a dependency of core wire types, **do not** — put
the bridge on the consumer side and say so in the doc.

- [ ] **Step 2: Write the failing test**

```rust
/// The doc on `Verdict` claims it is "the same refusal-with-findings shape the GateVerdict
/// kind carries on the wire". It was not: findings were bare strings while the wire carries
/// severity, claim, evidence and remediation. This makes the sentence true.
#[test]
fn a_refusal_converts_to_wire_findings_with_a_remediation() {
    let verdict = Verdict { passed: false, findings: vec!["the note is empty".to_owned()] };

    let wire = verdict.findings_as_gate_findings("write something in the note");

    assert_eq!(wire.len(), 1);
    assert_eq!(wire[0].claim, "the note is empty");
    assert_eq!(wire[0].remediation, "write something in the note");
    assert!(wire[0].evidence.is_empty(),
        "a structural finding grounds in nothing, and the shape must permit that");
}
```

- [ ] **Step 3: Run it and watch it fail** — `no method named findings_as_gate_findings`.

- [ ] **Step 4: Implement, and fix the sentence that was wrong**

Update the `Verdict` doc to state what it now is, and add the conversion. **Do not delete the
sentence** — it recorded an intent that is now satisfied; deleting it loses the record that the two
shapes were once different.

- [ ] **Step 5: Run, then commit**

```bash
git commit -m "feat(211): bridge harness verdicts to wire GateFindings, and make the doc true"
```

---

## Deliberately NOT in this plan

Per the scope check, these are separate subsystems and each earns its own plan once the core lands:

- **The CLI/API/MCP surface.** `apps/cli/src/commands/quality.rs` already certifies geometry; the
  generic command is an extension of it, and it is a public surface with its own review cost.
- **The Graph DSL acceptance journey** proving valid / missing-observer / flaky / waived / refused.
  It composes with #153 and is what #226 executes.
- **Evidence strength enforcement.** #211 names it beside observer trust, and Task 3 covers only
  observer trust. The lattice already exists as a declared artifact
  (`extensions/builtin/graphhelm-jpd/evaluators/evidence-strength-lattice.yaml` with its schema), so
  the gate must READ it rather than re-encode the levels — the `jpd_observer_trust.rs` test today
  hardcodes the five trust levels and eight facts as Rust arrays, which is precisely the second
  producer this plan avoids elsewhere. **That test's arrays are the thing to delete, not to copy.**
- **The remaining four validators** (journey contract, observation obligation, retry lineage, council
  result). Task 3 establishes the shape; each subsequent validator is the same five steps and should
  not be written until the first has survived review.

## What this plan does NOT prove — named, not left to be discovered

- **Evidence is carried as `serde_json::Value`, not as typed Rust structs.** That is deliberate — the
  JPD schemas are the authority and a second Rust declaration would be a second producer of one
  closed vocabulary. **The cost is that a malformed document reaches the gate**; schema validation
  before the gate is `graphhelm_schema`'s job, and this plan does not do it. Whoever wires the CLI
  surface must validate before certifying, or the gate judges shapes nobody checked.
- **`jpd_suite()` starts at three specimens.** Three is not a claim about coverage; it is the
  smallest set that exercises more than one axis. #226's corpus is what grows it, and growing it is
  exactly what voids stale certifications.
- **No test here proves the certification is consumed.** `GateCertified` exists on the wire and
  `certification_is_current` exists in the harness, but nothing in this plan checks that a stale
  receipt actually refuses execution. That guard belongs with the CLI surface and is named here so it
  is not assumed.
