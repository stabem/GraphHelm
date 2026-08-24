# Blueprint — #220 (task-004): opt-in durable memory and governed publication

> **FOLD, 2026-08-24.** The version the cross-review was written against is pinned at
> **`c5d77b9`** (blob `fbec16f`) and is unchanged in history — this document is that one **plus** the
> seven findings from the #220 review, plus one coordinator arbitration. **What was reviewed still
> exists; this is not a rewrite of it.** Sections §2.1 and §2.3 are deliberately untouched: the
> reviewer asked that the two strongest mechanisms not pay for the repairs, and none of the repairs
> required it.
>
> **Provenance of each addition is marked in place** — `[F1]`…`[F7]` for review findings,
> `[ISSUE]` where the authority is the issue text itself rather than any later ruling.
> **Nothing below is claimed as my own derivation when it is not.**

**Status: BLUEPRINT ONLY. No code, no fixtures, no policy files.** The implementation opens on the
coordinator's word. Everything below is a design commitment written so the implementation can be
*checked against it* rather than compared to a memory of a conversation.

**Sources read:** issue `#220`; the plan at
`docs/superpowers/plans/2026-08-22-native-token-efficient-development-contracts.md` on
`issue-216-token-efficient-development-contracts` (`f0640cd`); `origin/main` at `ef9afcb`, including
`apps/cli/tests/jpd_capsule_authority.rs` as the trap model named by the coordinator.

---

## 1. Contracts consumed from #217, named — and what happens when they are absent

This task **consumes** and does not redefine:

| from #217 | consumed as |
|---|---|
| the common envelope + unknown-major rejection | every memory artefact validates **before** typed deserialization |
| `ArtifactBinding` (scope, schema id/version, producer, digest, required snapshots) | the only way a memory record names its evidence |
| stable refusal codes | every refusal below emits one of these, never a bespoke string |
| canonical digests + deterministic serialization | answers *is the MEANING the same?* — **`[F4]` never byte-identity: a digest over canonical JSON is blind to key order, so a published record binds on BYTES and keeps the digest for meaning** |

**Design commitment: absence of a #217 contract is a REFUSAL, never an improvisation.** If a required
schema id, refusal code, or binding field is not registered, admission fails closed with the
registry-missing refusal — it does not fall back to a local shape. **A task that improvises a contract
when its dependency is missing silently forks the wire**, and the fork is invisible until two
producers disagree.

**This is a dependency in the strong sense:** the blueprint below is not implementable before #217
lands, and the first guard written must be the one that fails when it has not.

---

## 2. The cycle, with every refusal named AT THE SITE THAT WOULD EXECUTE

The requirement the coordinator set is precise and it is the spine of this design: **a refusal is
recorded at the point that would otherwise perform the act, not in a document about the act.** A
refusal named elsewhere is a signpost — it buys placement, not enforcement.

```
                 ┌─ capture gate ─┐   ┌─ admission ─┐   ┌─ transition ─┐   ┌─ publication ─┐
   candidate ───▶│  opt-in check  │──▶│   scanning  │──▶│ state matrix │──▶│ Governor-only │
                 └────────────────┘   └─────────────┘   └──────────────┘   └───────────────┘
```

### 2.1 Capture gate — the refusal that must leave no trace

**Site:** the earliest call in the capture path, **before** any candidate is constructed and before
any provider or store handle is acquired.

**Refusals named here:** capture-disabled; project-not-opted-in; scope-not-owned.

**The property is unusual and drives the guard design: when disabled, the correct behaviour is that
NOTHING happens** — no candidate, no provider touch, no log line, no event, no persistent write.

**`[F6]` And the capture side has its own crash boundary, which the publication ordering in G7 does
not cover: the opt-in record must be durable STRICTLY BEFORE the first content byte.** Otherwise the
surviving intermediate is content on disk with no record of the consent that permitted it — **the
intermediate that reads as "someone captured this without asking"**, which is the wrong one to leave
behind.

**Design commitment: the disabled path acquires no handle.** The opt-in check sits above handle
acquisition, not inside the code that already holds one. A check that runs after the handle exists
has already touched the boundary it is meant to protect.

### 2.2 Admission — pre-candidate scanning

**Site:** between the capture gate and candidate construction. **Nothing scanned here has been
persisted, logged, or handed to a provider yet** — that ordering is the point, and it is what makes
the refusals meaningful.

**Refusals named here, one code each:** raw prompt/chat content; secret or credential material; broad
tool output beyond the allowlisted fields; unknown fields at a major version; provider-loop
(content that originated from a provider being re-admitted as source); cross-scope content.

**`[F1]` THE REFUSAL IS ITSELF A PERSISTED EVENT, and this is the hole the review found.** The
paragraph above describes the instant BEFORE the refusal. **The refusal that follows is an event with
a code, and it can carry the very content the scan refused** — a secret rejected for being a secret,
copied into a refusal record "for diagnostic quality", now lives in an append-only journal that this
issue's own rollback clause forbids deleting or rewriting. **Every step legal; the content reaches
persistence through the REFUSAL path, which was the one path argued safe because it precedes
persistence.** The guard becomes the vehicle.

**Design commitment, following the pattern the repo already uses for `GHEX*` codes — paths and
pointers, never content:**

> **A refusal record names the CODE and the LOCATION, never the value that caused it.**

**Design commitment: allowlist, never denylist.** A denylist refuses what it recognises; the failure
mode of this task is content nobody enumerated. **The allowlist's failure mode is a refusal someone
must widen deliberately — visible, arguable, and logged.**

### 2.3 Transition — the closed state matrix

**Site:** the single function that performs a state change. There is one, and every path routes
through it.

**Refusals named here:** invalid-transition; stale-dependency; authority-insufficient;
validator-not-independent.

**Design commitment, and it is the most important one in this blueprint:** the matrix of allowed
`(state, transition)` pairs is **DERIVED from the state and transition enums**, never written as a
list. The complement — *every other tuple refuses* — is a **closed-world claim**, and a closed-world
claim tested against a hand-written list of the tuples someone thought of **shrinks in silence the
day a variant is added**. That is #98's allowlist and #207's derived-guard requirement, at a third
altitude. **If the matrix cannot be derived, this half does not ship as a hand list; it ships
declared as uncovered.**

### 2.4 Publication — Governor-only, atomic, re-sealing

**Site:** the Governor's publish entry. **No other caller can reach the mutation path**, and that is
enforced by visibility, not by convention.

**Refusals named here:** not-the-Governor; evidence-not-resealed; provisional-metadata-mutation;
publication-not-atomic (crash boundary).

**Design commitment: publication binds NEWLY SEALED evidence and never edits the provisional record.**
The provisional metadata is annotated by a successor record, not updated in place. **Annotating is not
updating**: a record edited in place stops being evidence of what was true when it was written, and
the audit trail this task exists to produce is exactly that.

### 2.5 `[ISSUE]` Withdrawal, erasure, handoff — decided by the issue, not by a ruling

**`[F2]` The review measured this section's absence** — `withdraw`, `erasure`, `expired`, `handoff`
were **zero** in the reviewed version, with a positive control proving the instrument saw the rest.
**§3's G6 covers SUPERSESSION, and supersession is not withdrawal.**

**The authority here is the ISSUE, and that correction came from the coordinator against their own
arbitration:** they ruled on it, then pointed out that **#220 had already decided it** —
*"expired/stale/withdrawn memory is excluded by default **but remains auditable**"* — and that **a
decision already in the record outranks a fresh ruling that merely agrees with it**, because a ruling
ages with the person who made it while the issue's text travels with the issue. **Cited from the
issue; the arbitration is recorded as concurring, not as the source.**

> **The opt-in clock is the CAPTURE clock, and withdrawal is an event that closes USE, never the
> record.**

1. **Legality of PERSISTING is judged at the instant of capture.** Content opted-in at T1 stays
   persisted; the append-only journal is not rewritten. *The record tells the truth of the instant it
   was written.*
2. **Withdrawal at T2 is an EVENT in the same journal**, and from it the Governor **closes all
   downstream USE** of the withdrawn content — reads for capsules, presentation, handoff.
3. **Audit access survives**, which is why the criteria demand persistence at all.

**`auditable` ≠ `usable`, and that distinction is what dissolves the tug-of-war:** opt-in governs what
comes IN, withdrawal governs what goes OUT from then on, **and the journal never lies about either.**
It is the same move as a spent claim leaving its node parked — **the new state does not erase the old
fact, it changes what that fact licenses.**

**And ERASURE is the word where the tension was real — the mechanism already exists and this design
consumes it rather than inventing one.** Measured on `origin/main` `073d8fa`:
`core/protocols/src/event.rs:729,741` carry an **`erasure_pending`** state, and `README.md:74`
describes *"auditable cryptographic erasure … Replay never requires plaintext."* **Erasure is key
destruction, not journal deletion:** ciphertext stays, the trail stays complete, plaintext becomes
unrecoverable. **Both invariants hold at once.**

**`[H2]` The reviewer verified that correction instead of accepting it, and the verification made the
mechanism FINER than I had described it.** Measured on the same base, `retention.rs:17` imports
**three distinct kinds, not one** — `EvidenceErasureRequested`, `EvidenceErasureCompleted` and
`EvidenceCiphertextDeleted`, each with its own timestamp accessor (`:1074`–`:1076`). **So deleting the
ciphertext is a step of its own, downstream of erasure and not a synonym for it**, and the path is
blockable: `retention.rs:72` yields **`RetentionBlockReason::LegalHold`**.

**Therefore the withdrawal path must NAME which of the three it emits, because they are not
interchangeable** — and this design emits **none of them by default**. Withdrawing a memory record
closes USE; it does not request erasure of the evidence beneath it. **Erasure is a separate, requested
transition that a legal hold may refuse**, and a design that quietly coupled the two would turn a
user's "stop using this" into an irreversible destruction the hold existed to prevent.

**One convergence worth naming, because it is the repo teaching the same rule the review taught me:**
`EvidenceCiphertextDeleted` keeps `ciphertext_sha256` (`core/protocols/src/event.rs:787`). **The
record of a deletion carries the IDENTITY of the deleted bytes and never the bytes** — which is
exactly `[F1]`'s *name the code and the location, never the value*, already implemented on `main`
before this blueprint restated it as a rule.

**`[F3]` HANDOFF is a legal transition into the exact leak, and it was absent too.** A handoff into a
scope that never opted in ends with persisted content in a non-opted-in scope. **Refusal named at the
transition site: `handoff-target-not-opted-in`.** The issue keeps handoff in scope and puts
cross-project sharing out, **and the boundary between the two is where this leaks** — where the design
cannot yet settle a case, §4 declares it uncovered rather than leaving it silent.

---

## 3. The guards, red-first — what the implementation will seal

Each guard below **must be observed red at its own assertion before being made green**, and the red
must be named by its panic site *against a stated commit*. A red landing anywhere upstream — a
fixture that fails to build, a schema that refuses the batch — proves the fixture broke and says
nothing about whether the guard sees.

### G1 — disabled capture touches nothing (PAIRED, because bare absence is not a guard)

Asserting "nothing was written" passes identically when the feature is off, when the read is
mis-targeted, and when the whole path is broken. **This guard is written as a PAIR over the same
read:** disabled → the boundary is untouched; enabled, same fixture, same read → the boundary shows
the candidate. **A dead or mis-pointed read cannot produce the transition.**

**`[F7]` And a pair needs a THIRD outcome, or it degenerates into the thing it exists to prevent.**
If the ENABLED arm silently fails to produce a candidate, the pair reduces to the bare-absence
assertion — **and it reduces GREEN.** An exit code cannot separate *"ran and failed"* from *"never
started"*, and neither can two states: the guard reports **PASS / FAIL / HARNESS-BROKE**, with a
matrix mutation whose required result is HARNESS-BROKE rather than either of the other two.

The negative half asserts on **every** boundary the acceptance criterion names — candidate, provider,
log, event, persistent — because a guard that checks one of five and reports "nothing happened" is
the flattening this task is built to prevent.

### G2 — each admission refusal fires at its own code, on its own fixture

One fixture per refusal class, each valid under #217's envelope first (**positive control**), then
mutated in exactly one dimension. **The assertion is on the refusal CODE, not on "an error
occurred"** — six refusals that all assert "some error" are indistinguishable from one refusal that
fires six times.

### G3 — the self-publication trap, modelled on `capsule_v0_1_rejects_self_promotion_and_publication`

The model from #215 is the right shape and this blueprint adopts it directly: **one valid fixture,
validated first; then N single-dimension mutations, each with its own named refusal message.**

For this task the mutations are: a **skill** attempting to publish; a **provider-sourced record**
claiming producer authority; and a **provisional** record self-promoting to published without the
Governor. Each is refused with its own code, and each assertion carries the sentence that says which
claim was refused — *"a data-only skill cannot publish a memory mutation"*, not *"invalid"*.

### G4 — refusal leaves the predecessor byte-identical

**"Refuses without changing the predecessor" is two assertions, and a guard that makes only the first
passes while the predecessor is corrupted.** This guard reads the predecessor record **after** the
refusal and compares it to the bytes read before. Asserting at the finest grain: bytes, not "still
present".

### G5 — independent validation is not self-validation

A memory validated only by its own producer is refused. **Validation that shares a source with the
claim is not validation**, however diligent the code path looks — the guard mutates the validator
identity to equal the producer identity and asserts the named refusal.

### G6 — contradiction and supersession stay visible

A superseded record remains readable and remains linked to what superseded it. **The guard asserts
the sweeper exists**: excluded-by-default is a *view*, not a deletion, and the audit read returns
both. A record that is excluded and unreachable is indistinguishable from one that was erased.

### G7 — the crash boundary

Publication interrupted between the reseal and the record write leaves a state that **reads as
truth**: the predecessor still provisional, which is true and is what a later reader should conclude.
**Ordering is the mitigation, chosen rather than defaulted** — two writes that cannot be atomic are
ordered so the surviving intermediate is the honest one.

### G8 — the dependency guard (#217 absent)

Written first, and red before anything else exists: with a required contract unregistered, admission
refuses with the registry-missing code. **This is the guard that stops the improvisation named in
§1.**

---

### G9 `[F1]` — the sentinel secret does not reach the journal by way of its own refusal

Admission is fed a **known sentinel** secret and refuses it. The guard then **greps the entire journal
for the sentinel**. **The sentinel is a positive control by construction:** it is known to exist in
the input, so a journal grep that finds nothing separates *"the refusal record carried no content"*
from *"the grep is broken"* — which a search for an unknown secret never could.

### G10 `[F3]` — handoff into a scope that never opted in is refused at the transition site

Named refusal `handoff-target-not-opted-in`, asserted on the code and on the predecessor being
byte-identical afterwards (the G4 form).

### G11 `[ISSUE]` — withdrawal closes USE and leaves the RECORD

Two assertions over the same read, because either alone passes for the wrong reason: after the
withdrawal event, **every downstream use path refuses**, and **the audit read still returns the
record**. A guard asserting only the first passes on an implementation that deleted the row.

---

## 3b. Preconditions the implementation inherits — not findings, but blockers

**`[F5]` The manifest is append-only and the validator rejects undeclared files.** Measured against
`core/schema/src/extension.rs` on `origin/main` `073d8fa`: the validator walks `policies/` and
`fixtures/`, refuses any undeclared file with **`GHEX012_INVENTORY`**, and verifies `sha256` against
the bytes (**`GHEX005_DIGEST`**); only `extension.json` and `README.md` are exempt.

**So the three groups in this task's strict scope — `memory-admission.yaml`,
`memory-transition.yaml`, `fixtures/memory/**` — do not land without manifest entries in the SAME
PR.**

**CLOSED BY RE-MEASUREMENT, 2026-08-24 (this line was written to fall, and it fell).** The first
reading found **zero** occurrences of `extension.json`, `manifest`, `sha256` or `GHEX` in `#220`'s
body, against a positive control of `policies/memory` ×2. **The cause was not that the amendment did
not exist — it existed as a COMMENT.** The re-reading, after the coordinator moved it, finds each of
the four terms **×1 in the body**, same control, under a dated `## Scope amendment` section that adds
`extension.json` to the file scope append-only, requires this task's own `contributions[]` entries
with their `sha256` in the same PR, puts editing another task's entry out of scope, and points at
`#216` for the rule.

**The distinction the two readings measured is worth more than the blocker was:** *a scope amendment
in a comment is a RECORD; in the body it is the SCOPE.* An implementer reads the body. **Same
artefact, wrong section — the delivered-versus-recorded split, occurring inside a single issue.**

## 4. What this blueprint does NOT settle — declared, not omitted

- **`[H2]` Whether withdrawing a PROJECT's opt-in reaches records already captured under it.** The
  issue's phrase is *"withdrawn **memory**"*, which answers the withdrawal of a RECORD; the
  project-level case is a different question and the mechanism above makes **both** answers
  implementable. **Declared as a routing question, not designed here** — and it is smaller than the
  finding it survived, marked as such rather than restated at reduced volume.

- **MCP-backed store access is out**, pending #213. Nothing here designs it, and the blueprint does
  not assume its shape.
- **Whether the transition matrix is derivable from the current enums is NOT MEASURED.** §2.3 states
  what must happen if it is not: ship the covered half and declare the rest, never a hand list. The
  measurement belongs to the implementation's first hour.
- **No threat-model section is produced here.** #220 requires one *before* implementation; this
  blueprint is upstream of that and does not substitute for it.
- **No claim about test counts, timings, or gate outcomes.** Nothing has been run. No cargo, no gate,
  no fixtures.
- **Out of scope per the issue and untouched here:** general memory-provider implementation, vector
  database, raw session capture, automatic publication, Dreams consolidation, cross-project sharing.

## 5. Wave rules acknowledged

**ED-18 applies:** nothing merges without checking the result of the merge, not only the branch.
**Skills stay data-only** — the self-publication trap in G3 exists precisely to make that structural
rather than stated. **Strict file scope** from #220 is the boundary for the implementation phase; this
blueprint touches no file in it.
