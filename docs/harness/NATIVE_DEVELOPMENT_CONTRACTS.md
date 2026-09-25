# Native Development Contracts — journey certification and the sabotage corpus

## Status

Delivered for issue #226 (task-010). This document records what was **measured**, and is deliberate
about the difference between what has been proven and what has only been written.

The S1b journey-contract obligation now executes through the generic candidate gate. The broader
public quality command/API/MCP contract from #211 remains pending and is named in section 7.

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
| `extensions/builtin/graphhelm-development-contracts/fixtures/sabotage/s{1b,2,4,5a}-*/` | the original red-window entries; S1b and S2 are now marked by their executed gates |
| Appendix B of this document | the five entries whose protections predated the corpus; S1b's later transition remains beside its original red-window record |
| `apps/cli/tests/development_journey.rs` | holds the graph and its fixtures against each other |
| `apps/cli/tests/development_sabotage.rs` | holds the corpus against the schemas it attacks |

### What the corpus digest binds, and what it does not (#827)

Eleven contributions in the package's `extension.json` name paths under `fixtures/sabotage/`, so
`GHEX005` binds those fixtures' **raw bytes**. That is worth stating precisely, because it is
narrower than "the corpus is frozen":

**The digest binds the bytes. It does not bind the meaning.** A fixture's donor is cited as a
repository path, and `resolved_path` in `development_sabotage.rs` admits a citation only when that
file exists on disk. So a fixture's population depends on a tree the fixture does not own: the
bytes cannot change without the digest noticing, and the subject they describe can move without
the digest having anything to say.

Measured while closing #827, and reported in full because the answer is "all of them":

| | |
|---|---|
| distinct `.rs` paths cited across the doc and the corpus | 7 |
| resolving against the live tree | 7 |
| cited by a fixture rather than only by this document | 3 (`core/governor/src/memory.rs`, `core/graph/src/persistence.rs`, `core/runtime/src/retrieval.rs`) |

**A rename does not thin the corpus silently, and this was measured rather than assumed.** Moving
`core/governor/src/memory.rs` aside — repairing only the module declaration, so the build survives
and the red cannot come from the compiler — turns three cells red. The one that names the cause
reconciles the coordinates the extractor consumed against the coordinates the text spells, because
`count_coordinates_in_line` reads syntax while the extractor reads the disk. Two instruments, one
of which does not consult the filesystem, is what makes the loss visible.

That divergence is an interaction rather than a decision, so
`a_citation_whose_path_cannot_resolve_diverges_the_two_counts` pins it: put the existence filter
back into the counter and the corpus becomes quietly hollow-able again, with every other test
still green.

**A fixture-local donor tree was considered and declined.** It would make existence a property of
the fixture, which is the honest shape for something called frozen. It was proposed to stop a
*silent* emptying, and the emptying is not silent — what remains is conceptual coupling with a loud
failure. Reopen it if the coupling starts costing something the count reconciliation cannot show.

The journey runs `declare_scope → bind_snapshots → retrieve → capture_gate → { evidence_with_capture
→ admit_memory | evidence_without_capture } → certify`.

The two capture branches exist because the acceptance criteria require **distinct** boundaries, and
they refuse for different reasons: the disabled branch prohibits `memory.capture` outright, the
enabled branch must pass the admission screens. `onUnknown` is `fail` rather than a route, because
an undetermined capture state is not the same thing as capture being off.

### Current memory-admission surface

The current public slice exposes the same fixed-input operation through `graphhelm development memory-propose`, `POST /v1/development/memory`, and the MCP tool `memory_propose`. It reports an admission verdict but accepts no caller content or scope and returns no record identifier. The operation always evaluates the built-in safe proposal under an enabled local scope; it proves surface parity, not a general memory-ingestion workflow.

The Governor also implements durable `memory_admission_refused` events for explicitly opted-in projects. Each event carries only a closed refusal code, a closed location, and the rejected byte count. It never carries rejected content or its digest. Disabled capture and incoherent opt-in return before repository access. A handoff into a scope without capture opt-in is refused as `handoff_target_not_opted_in`, distinct from `opt_in_absent` on the source project's own capture. This event path is a library contract; the fixed-input public operation does not currently trigger it or persist a `MemoryCandidate` or `MemoryRecord`.

Node completion contracts name `coverage_carried` and `observer_distinct_from_actor`. Those are two
of the sabotages written as **requirements** rather than as attacks: the corpus attacks, the graph
declares, and both must name the same property or the sabotage has nothing to bite.

## 3. Two columns, and why the labels matter

**Red window (2):** the protection has not been written, so the sabotage can be genuinely red today.
`S4` unsafe compression · `S5a` secret capture, open half.

**Marked (7):** five protections shipped before their entries were written; S1b is the sixth and
S2 the seventh. Each entry names its trigger and refusal site. **Two categories of evidence sit here and they are not interchangeable:** S1b and S2's schema tripwire carry an OBSERVED transition -- each existed while its subject was still red and went green when the protection landed. S2's runtime cell does not: it was written after #616 merged, so it never saw a pre-fix red, and its red is proved BY SABOTAGE only. Both are evidence; only the first is history. Each entry names its trigger and
refusal site.
`S1b` · `S2` · `S3` · `S5b` · `S6` · `S7` · `S8`.

Nine entries from eight named sabotages — `S5` splits into an open and a closed half.

Labelling, never omission. Dropping the marked entries would leave those boundaries uncovered and
imply they were never considered.

`S1a` (`requires.observers` in the extension manifest) is recorded in the sealed expectations file
and **deliberately not counted**: no producer emits that requirement, so a guard for it would pass
vacuously forever, and counting it as coverage would be the exact error this corpus exists to avoid.

## 4. What was measured

Every claim below carries the command's own base (`origin/main`) and was taken by walking parsed
documents rather than by pattern-matching text.

**The remaining red-window entries against shipped artifacts share one shape**: the record needed to
detect the defect is absent, optional, or unlinked.

| entry | artifact | the record is... | measurement |
|---|---|---|---|
| S2 | development envelope | **absent** | `#/$defs/coverageState` has **0** `$ref`s, re-measured at the commit this line landed in: 60 schema files, control `#/$defs/opaqueId` = **241**. (The earlier figures here were 57 files and control 206; a count like this decays every time someone adds a schema, so it is re-taken rather than carried forward. **A `$ref` count is not a consumer count** -- the definition has one live Rust consumer, `apps/cli/tests/development_contract_schemas.rs`, which pins it to `CoverageState::every()`, and #630 pinned the receipt's copy to the same type rather than deleting either.) The envelope root does not set `additionalProperties: false`, so a `coverage` field is carried and never checked against the closed vocabulary that exists to constrain it. |
| S4 | context capsule | **optional** | `sections` is required with six keys, none carrying `minItems`; `excluded` is not required and is unconstrained. A capsule may empty every section, omit `excluded`, and validate. |

**S1b was a third-site finding, not a missing check.** The discipline *"independence is compared by
identity, never by count"* is applied twice with the reasoning written down —
`core/governor/src/memory.rs:930` (`validate_candidate`: a validator roster may not be the producer) and
`apps/cli/tests/jpd_plugin.rs:1560` (`identityDistinctValidation`, present in 1 of 57 schemas) — and
was absent on the promise-to-observer pair. `JourneyContractGate` now binds the validated contract,
its validated observation obligations, and its verification result by `contractId` and trusted
`contractDigest`. For each promise it requires one or more obligations with the same `promiseId`,
validates every obligation, matches `requiredObserverCapability` to each
`resolution.capabilityBinding.capabilityId`, and confirms that every bound `observerId` is present
in the verification result's global observer roster. Each exact observer is compared with the
`actorId` on the promise's referenced step. Any equality refuses the promise with a stable finding.
Two actors may therefore observe one another without the global roster being falsely treated as a
per-promise assignment.

**A guard that passes, and what it does not prove.**
`apps/cli/tests/development_contract_schemas.rs:418` compares the `coverageState` enum on both
sides and asserts there are eight states. It is careful and correct. It says nothing about whether
anything references the definition or carries the value. *"The coverage vocabulary is guarded"* is
true; *"coverage is enforced"* is what it will be taken to mean.

**Oracle drift, recorded as drift.** `core/governor/src/memory.rs:299` screens with
`content.contains("ghp_")`; `core/graph/src/persistence.rs:736` requires a prefix **and** a tail of
16 or 20. A bare `ghp_` is refused by the first and not the second. The direction is fail-safe —
memory refuses more — so this is logged as divergence between two oracles, **not** as a
vulnerability. Its real cost is false positives on prose that mentions the prefix.

**A declared gap is not a hidden one.** The memory screen ships a section headed *"WHAT THIS SCREEN
DOES NOT REJECT, declared rather than left to be inferred from the cases that are covered."* The
marker corpus records those as `DECLARED_GAP` rather than attacking them, which turns it into a
regression net on the declaration itself. S1b entered the corpus as silence and is now an executed
obligation.

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

**S1b proof executed.** The first behavioral mutation held `requiredObserverCapability` constant at
`browser.semantic-journey` and changed the real observer identity. Against head `37cdffb`, the actor
identity incorrectly passed; the named test failed on its verdict assertion. A later review found
that the verification result's `bindings.observers[]` is only a global roster, not a per-promise
assignment. The next RED cell proved the consequence: A observing B and B observing A was falsely
refused, while missing obligation links incorrectly passed. GREEN now executes in this test against
the judge frozen on `main` at `7b55cf0`. The gate joins each promise through one or more observation
obligations' `resolution.capabilityBinding.{capabilityId,observerId}` values and uses the result
roster only to confirm each observer's participation. Every matching obligation is validated;
multiple independent obligations are accepted, while any actor-as-observer binding refuses the
promise. Mismatched contract ids, digests, capabilities, missing roster identities, and missing
obligations are refused before any identity claim is made. A renamed actor/observer pair is still
caught, and schema-bounded duplicate records produce one finding per promise rather than amplifying
diagnostics. S2, S4 and S5a retain their prior status; this slice makes no claim about them.

**A gap between instruments, still bounded rather than hidden.** The executed sabotage test now
validates the S1b artifacts through `OfflineSchemaSet` and passes the validated evidence to the
typed journey gate. `cargo test -p graphhelm-cli --test development_sabotage --locked` is the
executed measurement of that path. Other journey tests use `load_graph`, which additionally types
graph documents and enforces its own depth, size, and resource limits. No differential corpus proves
that these instruments accept and refuse exactly the same full input space. The executed S1b path
is evidence for that path, not a universal equivalence claim.

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

**Agent consensus is advisory in both directions.** The same typed verification evidence is tested
with a schema-valid council recommendation and a schema-valid blocked council result. Both produce
the same verdict and findings as the legal direct-tier baseline. This is tested for both
verification-result and journey-contract gates. Journey evidence may carry a council binding inside
its verification result, but neither gate has a council-dependent code path.

## 7. What must change when a protection lands

Each red-window guard asserts what is true **today**: the sabotage is accepted. When one fails, the
protection has landed. Then, and only then:

1. mark the entry in this document, with its owning issue and close state;
2. invert its guard in `development_sabotage.rs`, so it now asserts the refusal;
3. record the transition pair — `red @ <sha>` and `green @ <sha>` — in the sealed expectations file.

The pair is the certification. A lone green is not: it cannot distinguish a protection that works
from a cell that was never able to fail.

**Partially delivered by #211.** S1b now executes through a generic typed-evidence gate. S2 was blocked on #219 and is now MARKED: the protection landed in #616 as the retrieval-plan compiler, not as the typed-evidence gate this paragraph anticipated. S4 remains blocked on #222. The public quality command/API/MCP contract that accepts typed JPD
evidence and returns structured diagnostics also remains open under #211.

## 8. Provenance

Expectations were sealed before each fixture existed, append-only, in the #226 sabotage
expectation record (never committed to `main`). Corrections there are added below the original with
a pointer rather than edited in place, so an entry that changed can be read against what it replaced
— including the several corrections that measurement forced along the way.

## Appendix A — the original red-window entries and their transitions, in full

Each entry below shipped as a README beside its fixtures. It lives here instead because an
extension package declares its own inventory and there is no contribution kind for prose: a file
the manifest cannot declare is a file the package guard refuses. The reasoning still travels with
the corpus, one document further out.

## S1b — the observer is the actor (marked by #211)

Fixtures: `extensions/builtin/graphhelm-development-contracts/fixtures/sabotage/s1b-observer-is-the-actor/`

Sabotage corpus entry for #226 (task-010). Protection landed in the S1b slice of #211 after the
corpus had already recorded the red window.

### What it attacks

Declared independence that is not independent: the observer bound in the verification result has
the same identity as the promise step's `actorId`. The actor vouches for itself, while the required
capability can keep a different, valid name.

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

The actor identity lives in the **contract** (`step.actorId`). The result's
`bindings.observers[].observerId` values form a global roster and do not say which observer served
which promise. The exact assignment lives in the **observation obligation**: root `contractId`,
`contractDigest`, and `promiseId`, plus
`resolution.capabilityBinding.{capabilityId,observerId}`. The result roster confirms that the named
observer participated; it cannot create the per-promise relationship by itself.

**What is missing is not the ability to compare — it is any recorded obligation to.** No schema, no
test, and no declaration ties the two identities together.

That distinction matters: this is a **hidden** gap, not a declared one. Compare
`core/governor/src/memory.rs`, which ships a section headed *"WHAT THIS SCREEN DOES NOT REJECT,
declared rather than left to be inferred"*. A refusal named at the site that would execute is a
different thing from silence, and this is silence.

### Files

| file | role |
|---|---|
| `contract-observer-is-the-actor.json` | the original schema-valid contract subject |
| `contract-observer-independent.json` | the original schema-valid contract control |

The executed mutation keeps `requiredObserverCapability` constant and binds each contract to a
validated observation obligation and verification result. The obligation's
`capabilityBinding.observerId` changes between actor and independent cases, while the result roster
confirms that identity. A second control has both actors in the global roster while the obligations
cross-bind A to B and B to A; it proves the roster is not being misread as two per-promise
self-observation assignments. The controls are load-bearing: without them, an assertion could be
satisfied by refusing every evidence bundle.

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

### Assertion (registered before the fixture, now executed)

Verification refuses the promise, naming the bound observer and stating that its identity is not
distinct from the actor performing the step. The refusal must be for **non-independence
specifically**, not for absence — an implementation that only checks presence would pass this
fixture, since the field is present and well-formed.

### What is NOT claimed

- S2, S4, and S5a are not repaired by S1b's executed red-to-green transition.
- Variants 1 and 2 are **not** claimed as gaps; they are discarded as plausibly already mechanical.
- The historical measurement above established the pre-protection state at its recorded base. It
  is not a claim that the obligation remains unrecorded after `JourneyContractGate` landed.

Sealed expectation record: the #226 sabotage expectation record (S1, ADDENDUM-1/4/8).

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
the #226 sabotage expectation record (S2, ADDENDUM-2, ADDENDUM-3).

**Transition to MARKED — protection landed in #616.** The measurement above is kept as written: it
is still a true record of what was red and when. The protection is the retrieval-plan compiler
(`core/runtime/src/retrieval.rs`), not a schema change, so the schema-grain observation above STILL
HOLDS after the fix -- the wire really does lose the vocabulary at schema grain, by the design
decision recorded in #630 (the envelope's `spec` stays opaque on purpose, and a `RetrievalPlan`
per-kind schema is deliberately not written here). The runtime now refuses the attack before
certification would see it.

The tripwire that was supposed to announce this could not: its subject was the schema validator, so
it would have stayed green forever while its own doc-comment promised the signal (#630). It is
rewritten in S1b's shape -- the schema-accepts assertion stays, documenting why, and the assertion
that carries the weight moved to where the fix is.

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

Sealed expectation record: the #226 sabotage expectation record (S4, ADDENDUM-5/6).

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
| memory admission | `core/governor/src/memory.rs:299` | `content.contains("ghp_")` — one prefix, no tail requirement |
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

Sealed expectation record: the #226 sabotage expectation record (S5a, ADDENDUM-7).

## Appendix B — the MARKED entries

## MARKED entries — regression nets, not certifications

Corpus companion for #226 (task-010). These five sabotages have protections that **already
shipped**. A red for them is impossible by CHRONOLOGY, not by carelessness, and each entry says so.

**Gate 1 is satisfied by LABELLING, never by omission.** Dropping these would leave the boundary
uncovered and make the corpus dishonest in the other direction: a corpus of only-red-window entries
implies the closed boundaries were never considered.

Every site below is **measured at `origin/main`**, with the ref inside the command.

---

### S2 — false structural absence · owner #616, MARKED

**Protection.** The retrieval-plan compiler refuses a plan whose claimed coverage is outside the
closed vocabulary, and refuses a COMPLETE claim the producer's own record contradicts:

```
core/runtime/src/retrieval.rs:490   fn plan_coverage_is_a_closed_token
core/runtime/src/retrieval.rs:371   return Err(RetrievalReceiptError::CoveragePromotion)
core/runtime/tests/retrieval.rs:3116
       a_plan_coverage_token_outside_the_closed_vocabulary_is_refused
core/runtime/tests/retrieval.rs:3196
       the_committed_s2_corpus_is_refused_by_the_retrieval_plan_compiler
```

**Trigger it would catch.** Make `plan_coverage_is_a_closed_token` return `true` unconditionally, or
drop the promotion check — wrong but legal, and both were run.

**What is different about this entry.** The last cell above is the only one in the corpus that
carries the committed fixtures' ATTACK VALUE into the protection -- the claimed coverage token
and the producer's, read from the files, while the request and response around them are
synthesised. Measured while writing it: nothing in the
runtime suite read `fixtures/sabotage/s2-false-structural-absence/` at all — the guards proved the
property with inputs built beside them, so the fixture this document calls "the attack" was not the
thing proving the cure. A corpus that never meets its guard can rot into nonsense while every test
stays green, which is why that cell also asserts the fixtures still say what it claims they say.

**Why the schema half is not part of the protection.** There is no `RetrievalPlan` per-kind schema,
and the envelope's `spec` is opaque by a recorded decision. Removing the envelope's unreferenced
`$defs/coverageState` was proposed and REJECTED on measurement: it has zero `$ref`s but one live
Rust consumer pinning it to `CoverageState::every()`, while the receipt's copy — the one the wire
uses — had no such pin. Deleting the pinned copy and keeping the unpinned one inverts the safety.
Both copies are now pinned to the same type (#630), so neither can move alone.

---

### S3 — stale snapshots · owner #217 (task-001), CLOSED

**Protection.** `SnapshotBinding` carries TWO identities, and freshness is the RELATION between
them:

```
core/protocols/src/development.rs      is_fresh() -> repo_snapshot == index_generation
apps/cli/tests/development_contract_schemas.rs:613
       freshness_is_the_relation_between_the_two_snapshot_identities
apps/cli/tests/development_contract_schemas.rs:620-622
       stale.snapshots.index_generation = OpaqueId::parse("snapshot-h")
       assert!(!stale.snapshots.is_fresh(), "an index built from another snapshot is stale ...")
```

**Trigger it would catch.** Invert the identity comparison in `is_fresh()` — different reading as
fresh.

**Would fall at.** `apps/cli/tests/development_contract_schemas.rs:622`.

**Why it cannot be red-first.** The protection AND its guard shipped with task-001. Recorded in the
blueprint as a correction: the plan names "stale snapshots" only under task-010, so the term's
LOCATION suggested the protection was mine. It was not. **The term's location in the plan is not the
protection's location.**

---

### S5b — secret capture, the closed half · owner #220 (task-004), CLOSED

**Protection.**

```
core/governor/src/memory.rs:254   if content_is_secret_shaped(content) { ... }
core/governor/src/memory.rs:259   code: MemoryRefusalCode::SecretDetected
core/governor/src/memory.rs:299   fn content_is_secret_shaped -> content.contains("ghp_")
core/governor/tests/memory.rs:58    fn refusal_names_the_code_and_location_but_never_the_secret_value
core/governor/tests/memory.rs:1026  fn capture_refuses_secret_bearing_content_before_touching_any_boundary
core/governor/tests/memory.rs:1063  fn the_secret_detector_covers_the_shape_it_declares_and_no_other
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
core/governor/src/memory.rs:925-949
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
result binds a real `observerId`, and the gate compares that identity with the promise step's actor.
The required capability is not mistaken for an identity.

---

### S7 — scope bleed · owners #220 (task-004) + #222 (task-006), CLOSED

**Protection.**

```
core/governor/src/memory.rs:233-235   if scope != admitting_into { ... ScopeMismatch ... }
apps/cli/tests/development_contract_schemas.rs:557
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

**7 MARKED entries** (S1b, S2, S3, S5b, S6, S7, S8) against **2 red-window entries** (S4, S5a).
Total **9** entries from **8** named sabotages — S5 splits into S5a (open) and S5b (closed).

S1a (`requires.observers` never emitted in the extension manifest) is retained separately in the
sealed record as a narrow `legal-vs-produced` note. It is **not** counted here: its population is
empty, so a guard for it would pass vacuously forever, and counting it as coverage would be the
exact error this file exists to avoid.

Sealed expectation record: the #226 sabotage expectation record.
