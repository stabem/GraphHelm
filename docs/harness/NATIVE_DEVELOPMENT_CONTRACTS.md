# Native Development Contracts — journey certification and the sabotage corpus

## Status

Delivered for issue #226 (task-010). This document records what was **measured**, and is deliberate
about the difference between what has been proven and what has only been written.

Wiring to the generic candidate gate (#211) is **pending** and named as such in section 7.

## 1. Purpose

Task-010 certifies the owner journey end to end and ships a sabotage corpus that attacks it. The
corpus is not a list of bad inputs: each entry records the property it attacks, the assertion that
must refuse it, and the site that assertion would fall at.

The corpus exists because a passing suite says nothing about what it could have caught. An entry
written *after* its protection shipped proves the protection holds today and nothing about whether
the cell could ever have been red.

## 2. What ships

| artifact | role |
|---|---|
| `extensions/builtin/graphhelm-development-contracts/graphs/development-contracts-self-validation.yaml` | the journey: the development contracts exercised against themselves |
| `extensions/builtin/graphhelm-development-contracts/graphs/development-contracts-self-validation.fixtures.json` | six cases — two honest, four refusals — binding the journey to the corpus |
| `extensions/builtin/graphhelm-development-contracts/fixtures/sabotage/s{1b,2,4,5a}-*/` | the red-window entries, each with its own README |
| `extensions/builtin/graphhelm-development-contracts/fixtures/sabotage/MARKED.md` | the five entries whose protections already shipped |
| `apps/cli/tests/development_journey.rs` | holds the graph and its fixtures against each other |
| `apps/cli/tests/development_sabotage.rs` | holds the corpus against the schemas it attacks |

The journey runs `declare_scope → bind_snapshots → retrieve → capture_gate → { evidence_with_capture
→ admit_memory | evidence_without_capture } → certify`.

The two capture branches exist because the acceptance criteria require **distinct** boundaries, and
they refuse for different reasons: the disabled branch prohibits `memory.capture` outright, the
enabled branch must pass the admission screens. `onUnknown` is `fail` rather than a route, because
an undetermined capture state is not the same thing as capture being off.

Node completion contracts name `coverage_carried` and `observer_distinct_from_actor`. Those are two
of the sabotages written as **requirements** rather than as attacks: the corpus attacks, the graph
declares, and both must name the same property or the sabotage has nothing to bite.

## 3. Two columns, and why the labels matter

**Red window (4):** the protection has not been written, so the sabotage can be genuinely red today.
`S1b` observer-is-the-actor · `S2` false structural absence · `S4` unsafe compression ·
`S5a` secret capture, open half.

**Marked (5):** the protection shipped before the entry was written. A red is impossible by
chronology rather than by carelessness, and each entry says so, names its trigger, and names the
site it would fall at. `S3` · `S5b` · `S6` · `S7` · `S8`.

Nine entries from eight named sabotages — `S5` splits into an open and a closed half.

Labelling, never omission. Dropping the marked entries would leave those boundaries uncovered and
imply they were never considered.

`S1a` (`requires.observers` in the extension manifest) is recorded in the sealed expectations file
and **deliberately not counted**: no producer emits that requirement, so a guard for it would pass
vacuously forever, and counting it as coverage would be the exact error this corpus exists to avoid.

## 4. What was measured

Every claim below carries the command's own base (`origin/main`) and was taken by walking parsed
documents rather than by pattern-matching text.

**The three red-window entries against shipped artifacts share one shape**: the record needed to
detect the defect is absent, optional, or unlinked.

| entry | artifact | the record is... | measurement |
|---|---|---|---|
| S2 | development envelope | **absent** | `#/$defs/coverageState` has **0** `$ref`s under BOTH populations: all 57 schema files (1211 refs, control `#/$defs/opaqueId` = 206) and the 42 live ones excluding `schemas/releases/` pinned copies (890 refs, control = 108). The envelope root does not set `additionalProperties: false`, so a `coverage` field is carried and never checked against the closed vocabulary that exists to constrain it. |
| S4 | context capsule | **optional** | `sections` is required with six keys, none carrying `minItems`; `excluded` is not required and is unconstrained. A capsule may empty every section, omit `excluded`, and validate. |
| S1b | journey contract + result | **unlinked** | Actor identity lives in the contract, observer identity in the verification result, and the result binds the contract by `contractDigest` — so the join is available. Nothing records an obligation to perform it. |

**S1b is a third-site finding, not a missing check.** The discipline *"independence is compared by
identity, never by count"* is applied twice with the reasoning written down —
`core/governor/src/memory.rs:512` (a validator roster may not be the producer) and
`apps/cli/tests/jpd_plugin.rs:1550` (`identityDistinctValidation`, present in 1 of 57 schemas) — and
is absent on the promise-to-observer pair. The repository has beaten this defect twice and stopped
one relation short. **The fix needs no invention: `identityDistinctValidation` is already shipped and
tested.**

**A guard that passes, and what it does not prove.**
`apps/cli/tests/development_contract_schemas.rs:223` compares the `coverageState` enum on both
sides and asserts there are eight states. It is careful and correct. It says nothing about whether
anything references the definition or carries the value. *"The coverage vocabulary is guarded"* is
true; *"coverage is enforced"* is what it will be taken to mean.

**Oracle drift, recorded as drift.** `core/governor/src/memory.rs:285` screens with
`content.contains("ghp_")`; `core/graph/src/persistence.rs:736` requires a prefix **and** a tail of
16 or 20. A bare `ghp_` is refused by the first and not the second. The direction is fail-safe —
memory refuses more — so this is logged as divergence between two oracles, **not** as a
vulnerability. Its real cost is false positives on prose that mentions the prefix.

**A declared gap is not a hidden one.** The memory screen ships a section headed *"WHAT THIS SCREEN
DOES NOT REJECT, declared rather than left to be inferred from the cases that are covered."* The
marker corpus records those as `DECLARED_GAP` rather than attacking them, which turns it into a
regression net on the declaration itself. S1b, by contrast, is silence — and silence is what a
sabotage corpus is for.

## 5. How the guards avoid passing vacuously

- **A verdict requires a discriminating instrument.** `OfflineSchemaSet::validate` returns a
  diagnostic when the schema id is not registered, so `!diagnostics.is_empty()` cannot separate *"the
  document was refused"* from *"the validator never looked at it."* `development_sabotage.rs`
  classifies every attempt as `Accepted | Refused | HarnessBroke`, and a dedicated guard passes a
  fake id to prove the classification works. Schema ids are read from each schema's `$id` rather
  than transcribed, which removes the failure mode instead of guarding it.
- **Controls are baselined against the subject.** Negative controls derived by mutating a positive
  case inherit the positive case's own errors. A control counts only when it introduces a **new**
  error, at the path its mutation targets.
- **Honest cases sit beside the refusals.** Each capture branch has a case that certifies, so no
  refusal assertion can be satisfied by an implementation that refuses everything. S4 ships the
  honest twin and the intact capsule; S1b ships the independent-observer contract.
- **Refusal reasons are distinct.** No two refusal cases share a named reason, so an implementation
  refusing for the wrong cause cannot satisfy two entries at once.
- **The secret case forbids echoing.** Refusing while logging the value fails the half that matters
  most — the same argument the memory screen makes at its own refusal site, because the refusal is
  itself a persisted event.

## 6. What is proven, and what is not

**Proven.** Every sabotage fixture is schema-valid against the artifact it attacks, established with
negative controls that each fail by a different mechanism. Every fixture case names a node the graph
defines and a file that exists. The journey graph satisfies the shipped graph schema.

**Not proven.** **No `cargo` has run.** The two test files are written and their assertions were
simulated against the real artifacts, but neither has been compiled. Per the standing rule, a red
counts only once the run proves the subject BUILT and the named test RAN — `exit 101` does not
separate *"tests failed"* from *"did not build"*, and the bias runs one way, because a sabotage that
fails to compile looks like confirmation.

The four red-window entries are therefore **RED PENDING BUILD-PROOF**, which is not a weaker result
than a red. It is not a result.

**A gap between instruments, named in advance.** Fixtures were validated with a JSON-Schema
validator; the tests use `load_graph` and `OfflineSchemaSet`, which additionally type the document
and enforce their own depth, size and resource limits. **Nothing run so far covers the space between
those instruments.** The first real `cargo test` is the first measurement of that space, and should
be read as such rather than as a regression.

**One entry is thinner than the others.** S5a has no schema-grain guard, because captured journey
evidence has no schema in this tree — validating a shape chosen here against a schema also chosen
here would be a tautology. Its refusal assertions exist only at journey grain, which does not exist
yet.

**Replay preservation is not exercised.** The acceptance criteria require that *"replay preserves
attempts, refusals, waivers, memory transitions, and final accurate status."* `graph replay` returns
`ok` with `simulationStatus: completed`, and that is the whole of what was measured. Simulating an
unactivated graph produces `node_state_changed` and `simulation_completed` events and nothing else —
no attempt, refusal, waiver or memory transition is generated, so none can be shown to survive a
replay. **The criterion is untested, not met.**

**No browser or semantic-action coverage.** The criteria ask for semantic user actions and visible
loading/error/recovery/success states *"where the target journey exposes them"*, with proof by role
and label rather than coordinates. **This journey exposes none of them**: every node is a contract,
retrieval or evidence step with no rendered surface. The qualifier arguably makes it inapplicable
rather than failed — but the reader should be told which, and not left to infer it from silence.

**Agent consensus is asserted nowhere.** The criteria require that *"the generic gate certifies typed
evidence; agent consensus remains advisory."* The first half is exercised; the second half has **no
cell in this corpus**. That the property holds today is visible in the gate's own source — the JPD
module states the council is not read *"not carefully — at all"*, because a gate that consulted
consensus merely to REFUSE would still be letting agreement move the verdict — but reading a
comment is not testing a property. **This is the whole authority boundary resting on one unguarded
sentence**, and the sabotage it needs must fall on the VERDICT moving, never on the council field
being present: a specimen asserting only that consensus is recorded would pass against an
implementation that records it and lets it decide.

## 7. What must change when a protection lands

Each red-window guard asserts what is true **today**: the sabotage is accepted. When one fails, the
protection has landed. Then, and only then:

1. move the entry from the red window to `MARKED.md`, with its owning issue and close state;
2. invert its guard in `development_sabotage.rs`, so it now asserts the refusal;
3. record the transition pair — `red @ <sha>` and `green @ <sha>` — in the sealed expectations file.

The pair is the certification. A lone green is not: it cannot distinguish a protection that works
from a cell that was never able to fail.

**Pending on #211.** The corpus must ultimately execute *through* the generic candidate gate. That
wiring is not written, and the validation commands in the issue body cannot pass until the gate
lands. Reporting that as pending is part of this deliverable, not a gap in it.

## 8. Provenance

Expectations were sealed before each fixture existed, append-only, in
`.factory/n-agent-226-sabotage-expectations.md`. Corrections there are added below the original with
a pointer rather than edited in place, so an entry that changed can be read against what it replaced
— including the several corrections that measurement forced along the way.

## Appendix A — the red-window entries, in full

Each entry below shipped as a README beside its fixtures. It lives here instead because an
extension package declares its own inventory and there is no contribution kind for prose: a file
the manifest cannot declare is a file the package guard refuses. The reasoning still travels with
the corpus, one document further out.

## S1b — the observer is the actor

Fixtures: `extensions/builtin/graphhelm-development-contracts/fixtures/sabotage/s1b-observer-is-the-actor/`

Sabotage corpus entry for #226 (task-010). **Protection is mine to write.** Red window.

### What it attacks

Declared independence that is not independent: a promise whose `requiredObserverCapability` resolves
to the same identity as the step's `actorId`. The actor vouches for itself, and every field the
design uses to express independence is filled in correctly.

### Why this variant and not the other two

The sabotage has three plausible shapes. Two were discarded **by measurement, not by taste**:

1. a promise naming a capability absent from the catalog → referential integrity;
2. a capability present but whose `facts` / `evidenceKinds` do not cover the promise → also
   mechanical;
3. **a capability whose identity is the actor** → legal at every level, and false only in the one
   property the design exists to protect.

Variants 1 and 2 fail loudly against machinery that plausibly already exists. Variant 3 is the one
that passes everything.

### What the repository already enforces — checked before claiming a gap

Independence **is** enforced here, and well, on a different pair:

```
apps/cli/tests/jpd_plugin.rs:1550  jpd_defect_independence_is_distinct_input_with_candidate_authority
  - identityDistinctValidation must be present
  - reporter and reviewer actorIds        must be distinct
  - reporter and reviewer runIds          must be distinct
  - reporter and reviewer runAuthorityRefs must be distinct
```

**`identityDistinctValidation` appears in exactly 1 of 57 schemas** (`journey-defect-claim`). So the
repository has a working, proven pattern for exactly this class of defect — applied once, to the
reporter-vs-reviewer pair, and not to the actor-vs-observer pair.

### The gap, stated precisely

Measured over `journey-verification-result.schema.json`:

```
observerId                  : 2   (at /$defs/bindings/properties/observers/items)
actorId                     : 0
custody                     : 0
identityDistinct            : 0
requiredObserverCapability  : 0
```

The actor identity lives in the **contract** (`step.actorId`); the observer identity lives in the
**result** (`bindings.observers[].observerId`). The result binds to the contract through
`contractId` and `contractDigest`, so **the join is available**.

**What is missing is not the ability to compare — it is any recorded obligation to.** No schema, no
test, and no declaration ties the two identities together.

That distinction matters: this is a **hidden** gap, not a declared one. Compare
`core/governor/src/memory.rs`, which ships a section headed *"WHAT THIS SCREEN DOES NOT REJECT,
declared rather than left to be inferred"*. A refusal named at the site that would execute is a
different thing from silence, and this is silence.

### Files

| file | role |
|---|---|
| `contract-observer-is-the-actor.json` | the attack — `requiredObserverCapability` == `steps[0].actorId` |
| `contract-observer-independent.json` | positive control — a distinct observer identity |

The control is load-bearing: without it, an assertion could be satisfied by refusing every contract,
and the property is *"the observer is someone else"*, not *"observers are suspicious"*.

### Measured state today (`origin/main`, zero cargo)

```
SUBJECTS            contract-observer-is-the-actor     ACCEPTED
                    contract-observer-independent      ACCEPTED
NEGATIVE CONTROLS   version != 1                       REJECTED (const)
                    promise missing the observer field REJECTED (required)
                    expectedStates empty               REJECTED (minItems)
                    requiredFact outside enum          REJECTED (enum)
                    contractId breaking the id pattern REJECTED (pattern)
```

**Validator proven alive and discriminating: 5 of 5 controls rejected, each by a different
mechanism** — including one proving `requiredObserverCapability` is genuinely required, which is
what makes the attack a *satisfied* requirement rather than a missing one.

### Expected assertion (registered before the fixture)

Verification refuses the promise, naming the capability and stating that its identity is not
distinct from the actor performing the step. The refusal must be for **non-independence
specifically**, not for absence — an implementation that only checks presence would pass this
fixture, since the field is present and well-formed.

### What is NOT claimed

- **No red is recorded.** No cargo has run; there is no slot. Status: **RED PENDING BUILD-PROOF**.
- Variants 1 and 2 are **not** claimed as gaps; they are discarded as plausibly already mechanical.
- The measurement above says the obligation is unrecorded. It does **not** claim no runtime code
  anywhere performs such a comparison — only that nothing in the schemas, the one existing
  independence mechanism, or the verification result records it.

Sealed expectation record: `.factory/n-agent-226-sabotage-expectations.md` (S1, ADDENDUM-1/4/8).

---

## S2 — false structural absence

Fixtures: `extensions/builtin/graphhelm-development-contracts/fixtures/sabotage/s2-false-structural-absence/`

Sabotage corpus entry for #226 (task-010). **Untrusted data by construction**: no executable
content, no network, no real credentials.

### What it attacks

*"I looked and there is nothing"* standing in for *"my instrument did not see."* A conclusive zero
asserted over a non-conclusive one.

`core/protocols/src/development.rs` defines `CoverageState` as a CLOSED eight-state vocabulary —
`complete, partial, excluded, skipped, extraction_gap, stale, unknown, unresolved` — because
*"each state has a DIFFERENT correct response to a zero result."* `core/runtime/src/retrieval.rs`
carries it as `IndexResponse.coverage`, deliberately as a return value so that "forgot to check
coverage" is unrepresentable.

**The wire loses it.** Measured at `origin/main` across 56 schema files / 1204 `$ref`s:
`#/$defs/coverageState` has **0 references** (control: `#/$defs/opaqueId` has 206). The vocabulary
is defined in `development-envelope.schema.json` and referenced by nothing. The envelope's **root
object does not set `additionalProperties: false`**, so a `coverage` field is carried and never
checked against the enum that exists to constrain it.

### Files

| file | role |
|---|---|
| `producer-record.json` | ground truth — what the producer actually recorded: `extraction_gap`, nothing searched |
| `evidence-claims-complete.json` | the attack — `coverage: "complete"` over `hits: []` |
| `evidence-unknown-token.json` | discriminator twin — `coverage: "totally_fine"`, outside the closed vocabulary |

The two evidence files exist **separately on purpose**. A single assertion covering both would pass
against an implementation that validates the vocabulary and never compares the claim to the
producer's record. They must be refused for **different reasons**:

- `evidence-unknown-token.json` → refused because the token is not in the closed vocabulary.
- `evidence-claims-complete.json` → refused because the claim contradicts the recorded state.
  This one is the load-bearing half: every token in it is legal.

### Measured state today (`origin/main`, zero cargo)

Validated with `Draft202012Validator` against the real schema read at `origin/main`:

```
SUBJECTS            evidence-claims-complete.json  ACCEPTED
                    evidence-unknown-token.json    ACCEPTED
NEGATIVE CONTROLS   bad kind                       REJECTED (enum)
                    bad digest                     REJECTED (pattern)
                    missing producer               REJECTED (required)
                    scope + extra key              REJECTED (additionalProperties:false)
```

**The validator is alive** — 4 of 4 controls rejected, each by a different mechanism, one of them
proving `additionalProperties: false` IS enforced where it is set. So the subjects' acceptance is a
property of the schema, not of a dead validator.

**Observed red:** a token outside a closed eight-state vocabulary is accepted by the same schema
file that defines that vocabulary.

### What is NOT yet claimed

The journey-grain assertion (*certification is refused, naming the recorded state*) has **no site
yet** — the journey does not exist; it is this task's to write. The red above is at schema grain.
Per the seal, a red landing on fixture load or schema parse would be **vacuous** and reshapes the
fixture rather than being relabelled; that is why the subjects were proven ACCEPTED first.

### Derivation note, marked rather than assumed

`digest` is computed here as `sha256` over the canonical JSON of `spec` (sorted keys, no
whitespace). The repository's canonicalization rule for this field was **not measured**; the value
is well-formed and self-consistent, and any consumer that derives it differently should treat this
as a fixture-local convention, not as a claim about the wire rule.

Full expectation record, sealed before these files existed:
`.factory/n-agent-226-sabotage-expectations.md` (S2, ADDENDUM-2, ADDENDUM-3).

---

## S4 - unsafe compression

Fixtures: `extensions/builtin/graphhelm-development-contracts/fixtures/sabotage/s4-unsafe-compression/`

Sabotage corpus entry for #226 (task-010). **Untrusted data by construction**: no executable
content, no network, no real credentials.

**Owner of the protection: #221 (task-005), OPEN.** This entry is in the red window - the protection
is not written yet, and the pair `red @ <sha>` then `green @ <sha after #221>` is the certification.

### What it attacks

A Context Capsule whose compression drops content that downstream evidence then cites as if intact.

### The seam, measured at `origin/main`

`schemas/context-capsule.schema.json`:

| field | required? | shape |
|---|---|---|
| `sections` | **yes** | object; 6 required keys, each `array of string` - **no `minItems`** |
| `excluded` | **no** | `array of string`, free prose, unconstrained |
| `provenance` | yes | `array of string`, free prose |
| `dependencyHash` | yes | bare `string` - **no pattern** |
| root | - | `additionalProperties: false` (closed) |

**So a capsule may empty every section, omit `excluded` entirely, and validate.** Nothing forces
`excluded` to be present when content is dropped, and nothing ties its contents to what was actually
removed.

Contrast with the same repository own better answer, in `core/runtime/src/retrieval.rs`:
`IndexResponse.coverage` is a return value so that "forgot to check coverage" is unrepresentable
rather than merely discouraged. **Here, forgetting is representable** - the record of what was lost
is optional.

### Files

| file | role |
|---|---|
| `capsule-drops-silently.json` | the attack - all six sections `[]`, no `excluded` key |
| `capsule-declares-exclusion.json` | honest twin - same drop, declared |
| `capsule-intact.json` | positive control - real content present |
| `evidence-cites-dropped-content.json` | downstream evidence citing a line the capsule no longer carries |

The honest twin exists so the assertion cannot be satisfied by refusing *all* compression. **The
property is "what was dropped is declared", not "nothing was dropped"** - an implementation that
refused both would pass a careless assertion and break legitimate budgeting.

### Measured state today (`origin/main`, zero cargo)

```
SUBJECTS            capsule-drops-silently        ACCEPTED
                    capsule-declares-exclusion    ACCEPTED
                    capsule-intact                ACCEPTED
NEGATIVE CONTROLS   missing dependencyHash        REJECTED (required)
                    sections missing a key        REJECTED (required, nested)
                    extra root key                REJECTED (additionalProperties:false)
                    allocated = 0                 REJECTED (minimum)
                    version as semver string      REJECTED (type)
```

**Validator proven alive and discriminating: 5 of 5 controls rejected, each by a different
mechanism.** The last control is a real mistake made while writing this fixture - `version` is an
`integer`, not a semver string - and it is kept as a control precisely because it already caught one
malformed subject. Had it not been checked, the red would have landed on schema validation instead
of the assertion: a vacuous red, which this corpus discards rather than relabels.

### Expected assertion (registered before the fixture; unchanged)

The journey refuses to certify evidence whose cited content cannot be reproduced from the capsule as
shipped, and the refusal names the section that lost it.

### What is NOT claimed

- **No red is recorded here.** Per ADDENDUM-5, a cargo-grain red counts only once the run proves it
  BUILT and the named test RAN. There is no slot, so no cargo has run. Status: **RED PENDING
  BUILD-PROOF**.
- The schema-grain acceptance above is a property of the substrate, not a journey verdict.
- **Withdrawal condition, pre-committed:** if #221 own validator red already asserts the
  journey-visible property rather than only its own input, this entry is withdrawn per the grain
  boundary - #221 owns "does the validator refuse this input", #226 owns "does the journey refuse
  to certify end-to-end".

### Flagged, not chased

`dependencyHash` is an unpatterned `string` here, while the development envelope constrains its
`wireHash` to `^sha256:[0-9a-f]{64}$`. Whether that asymmetry is deliberate is **not measured and
not mine**; recorded so it is not lost, and not counted as a finding of this task.

Sealed expectation record: `.factory/n-agent-226-sabotage-expectations.md` (S4, ADDENDUM-5/6).

---

## S5a — secret capture, the OPEN half

Fixtures: `extensions/builtin/graphhelm-development-contracts/fixtures/sabotage/s5a-secret-capture/`

Sabotage corpus entry for #226 (task-010). **Owners of the protection: #221 (task-005) and #223
(task-007), both OPEN.** Red window.

The CLOSED half is S5b (#220, task-004), recorded as MARKED in the sealed expectations file.

### Safety, and a tension with my own seal, named rather than glossed

My sealed entry committed to *"synthetic markers only, never a real-shaped credential"*. **These
markers ARE shape-real** — they must be, because the detectors match on shape and nothing else.
Every tail here is the literal word `EXAMPLE` repeated. They carry the shape, they are not
credentials, and they authenticate nothing. The seal's intent — never paste a live token — is kept;
its wording was stricter than the test it was written for, and this note is the amendment rather
than a silent reinterpretation.

### The two detectors, both read at `origin/main`

| | site | trigger |
|---|---|---|
| memory admission | `core/governor/src/memory.rs:285` | `content.contains("ghp_")` — one prefix, no tail requirement |
| durable content | `core/graph/src/persistence.rs:736` | 25 prefixes each with a tail minimum (16 or 20), plus JWT, compact PEM, authorization, environment-URI and reference-name forms |

`memory.rs` documents its own boundary, and that doc comment was **verified against
`persistence.rs` rather than trusted**: the claim of *"roughly two dozen prefixes plus JWT, PEM and
secret-URI forms"*, and of being private to that crate, both hold — 25 prefix entries, every
function `fn`, none `pub`.

The same doc names the reason the list was not copied:

> a duplicated mechanism diverges loudly, a duplicated oracle diverges in silence and quietly
> changes what "refused" means.

**This corpus is built to hold that line.**

### The measured divergence

The two implementations already disagree, on an axis the memory doc does not mention. It declares
which FAMILIES it covers; it says nothing about trigger precision.

```
"ghp_" bare (no tail)   memory.rs      -> REFUSES
                        persistence.rs -> does NOT   (needs len >= prefix + 16)
```

Direction matters: `memory.rs` refuses MORE, so the divergence is fail-safe, not a hole. Its cost is
false positives — prose that merely mentions the prefix is refused as a secret. **Recorded as a
finding about oracle drift, not as a vulnerability, and not escalated as one.**

### The marker corpus, in three classes

`markers.json` labels each marker with what each detector does today:

| class | memory | durable | role in the corpus |
|---|---|---|---|
| `AGREED` | refuses | refuses | both paths must refuse; divergence here means a second oracle appeared |
| `DIVERGENT` | refuses | does not | the measured drift above, pinned so it cannot move unnoticed |
| `DECLARED_GAP` | does not | refuses | memory is narrow ON PURPOSE and says so |

**The `DECLARED_GAP` class is why this corpus is not merely a list of secrets.** Asserting refusal
there would be a false accusation against a boundary whose author declared it, with the reason
written at the screening site. A refusal named at the site that would execute is not the same defect
as a silent hole. So those markers are recorded as **declared not covered** — which turns the corpus
into a regression net on the DECLARATION: widen or narrow the memory detector without amending the
doc, and the corpus notices.

### Expected assertions (registered before the fixtures; separate on purpose)

1. The journey **refuses** to certify evidence carrying an `AGREED` marker.
2. The refusal **does not echo the marker**.

Fused into one assertion, an implementation that refuses while logging the secret would pass the
half that matters most. `memory.rs` makes this exact argument at its own refusal site: the refusal
is itself a persisted event, so quoting the value would carry it into the journal through the one
path the design argued was safe.

3. **Added from the measurement above:** a marker refused by one path must be refused by the other,
   or the disagreement is reported as drift. This is what makes a second detector — the likely
   shortcut when #221/#223 need screening — visible instead of silent.

### What is NOT claimed

- **No red is recorded.** No cargo has run; there is no slot. Status: **RED PENDING BUILD-PROOF**.
- The divergence above is measured by READING both implementations, not by executing them.
- Whether #221/#223 will screen at all is unknown; that is what the red window is for.

Sealed expectation record: `.factory/n-agent-226-sabotage-expectations.md` (S5a, ADDENDUM-7).

## Appendix B — the MARKED entries

## MARKED entries — regression nets, not certifications

Corpus companion for #226 (task-010). These five sabotages have protections that **already
shipped**. A red for them is impossible by CHRONOLOGY, not by carelessness, and each entry says so.

**Gate 1 is satisfied by LABELLING, never by omission.** Dropping these would leave the boundary
uncovered and make the corpus dishonest in the other direction: a corpus of only-red-window entries
implies the closed boundaries were never considered.

Every site below is **measured at `origin/main`**, with the ref inside the command.

---

### S3 — stale snapshots · owner #217 (task-001), CLOSED

**Protection.** `SnapshotBinding` carries TWO identities, and freshness is the RELATION between
them:

```
core/protocols/src/development.rs      is_fresh() -> repo_snapshot == index_generation
apps/cli/tests/development_contract_schemas.rs:358
       freshness_is_the_relation_between_the_two_snapshot_identities
apps/cli/tests/development_contract_schemas.rs:364-368
       stale.snapshots.index_generation = OpaqueId::parse("snapshot-h")
       assert!(!stale.snapshots.is_fresh(), "an index built from another snapshot is stale ...")
```

**Trigger it would catch.** Invert the identity comparison in `is_fresh()` — different reading as
fresh.

**Would fall at.** `development_contract_schemas.rs:367`.

**Why it cannot be red-first.** The protection AND its guard shipped with task-001. Recorded in the
blueprint as a correction: the plan names "stale snapshots" only under task-010, so the term's
LOCATION suggested the protection was mine. It was not. **The term's location in the plan is not the
protection's location.**

---

### S5b — secret capture, the closed half · owner #220 (task-004), CLOSED

**Protection.**

```
core/governor/src/memory.rs:240   if content_is_secret_shaped(content) { ... }
core/governor/src/memory.rs:245   code: MemoryRefusalCode::SecretDetected
core/governor/src/memory.rs:285   fn content_is_secret_shaped -> content.contains("ghp_")
core/governor/tests/memory.rs:43, 472, 501
```

**Trigger it would catch.** Remove the screen call at :240, or widen `content_is_secret_shaped` to
return `false`.

**A property worth keeping, stated at the refusal site itself:** the record names the code and the
field, never the value — *"the refusal is itself a persisted event, so quoting the secret here would
carry it into the journal through the one path this design argued was safe."*

**The OPEN half is S5a**, which is in the red window and carries the marker corpus.

---

### S6 — self-validation · owner #220 (task-004), CLOSED

**Protection.**

```
core/governor/src/memory.rs:512-528
    let producer = candidate.produced_by.as_deref();
    let independent = validators.iter().any(|v| Some(*v) != producer);
    if !independent { ... MemoryRefusalCode::SelfValidated ... }
```

**Trigger it would catch.** Replace the identity comparison with a count — `validators.len() >= 1`.

**Why this entry matters beyond its own boundary.** Its comment states the defect exactly:

> Validators are IDENTIFIED, not counted. "At least one validator signed off" is true when the only
> signature is the producer's own, and that is the shape self-validation takes in practice: nobody
> writes validate(self), they write a roster that happens to contain themselves.

**That is the same defect as S1b, solved here.** See `S1b — the observer is the actor`: the journey
promise names a `requiredObserverCapability` and nothing compares it to the step's actor. The
discipline is applied at two sites with the reason written down, and not at the third.

---

### S7 — scope bleed · owners #220 (task-004) + #222 (task-006), CLOSED

**Protection.**

```
core/governor/src/memory.rs:216-226   if scope != admitting_into { ... ScopeMismatch ... }
apps/cli/tests/development_contract_schemas.rs:302
       a_scope_mismatch_is_refused_under_its_own_code
```

**Trigger it would catch.** Compare `scope.project_id` instead of the whole struct.

**The reason is written at the site and is worth preserving:** the WHOLE scope is compared, not a
field of it, because comparing the project alone lets content cross workspaces whenever two tenants
name a project the same way — *"identical project names across tenants is the normal case, not the
exotic one"* — and because comparing the struct means **a field added to `DevelopmentScope` is
covered the day it lands, instead of the day someone remembers to add it here.**

---

### S8 — evidence deletion · owner #222 (task-006), CLOSED

**Protection.** `core/events/src/retention.rs` — deletion runs through an authenticated, digest-bound
retention pipeline rather than a direct remove:

```
retention_authority_authentication_bytes()   :153
retention_request_digest()                   :257
RetentionPlan::is_eligible()                 :439
prepared_authentication_bytes()              :517
```

**Trigger it would catch.** Make `is_eligible()` return `true` unconditionally, or drop the request
digest from the authenticated bytes.

**Attribution, marked rather than assumed.** The **site** above is measured directly. The **owner**
(#222 / task-006) comes from the blueprint's plan-to-issue mapping and is **not** verified at commit
level. Recorded as derived, not measured — the two halves of this entry do not carry the same
evidential weight and should not be read as if they did.

---

### Denominator

**5 MARKED entries** (S3, S5b, S6, S7, S8) against **4 red-window entries** (S1b, S2, S4, S5a).
Total **9** entries from **8** named sabotages — S5 splits into S5a (open) and S5b (closed).

S1a (`requires.observers` never emitted in the extension manifest) is retained separately in the
sealed record as a narrow `legal-vs-produced` note. It is **not** counted here: its population is
empty, so a guard for it would pass vacuously forever, and counting it as coverage would be the
exact error this file exists to avoid.

Sealed expectation record: `.factory/n-agent-226-sabotage-expectations.md`.
