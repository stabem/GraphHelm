# #159 blueprint — the customs pipeline (two-stage completion with clearance)

Author: B, 2026-08-20. Design-only, zero cargo. Feeds the #159 implementation lanes.

STATE STAMP (graph-first per directive): code facts below come from the codebase-memory
graph of `F:/github/GraphHelm`, indexed 2026-08-20T10:04Z at main `13a65e4` (M10 close);
`check_index_coverage` on every cited file: no recorded gaps, freshness `metadata_changed`
(mtime churn only). Line cites are against that root. Required reads honored: issue #159
(the sealed anchor), `d-agent-m11-synthesis.md` (sha 03b4e2e5a1c9046d), #153's four
addenda. CITE-or-MARK throughout.

## 0. The one-sentence design

**Customs is a new event family folded by the ONE replay projection everything already
reads: a claim is evidence-bearing testimony, clearance is the countersignature that
alone changes readiness, the dead-letter node is topology that accumulates instead of
running, and the sweep is a journaled verb whose clock is an argument — so CLI and HTTP
cannot disagree because neither owns any derivation the other lacks.**

## 1. What exists, graph-cited (the rails this snaps onto)

| fact | receipt |
|---|---|
| Readiness is ONE derivation with TWO call sites — both drivers | `ready_set` (core/execution/src/ready.rs:93-128); SPEND SITES (D re-derived at 9f3ee1e, git grep, tests excluded): sync CLI driver (apps/cli/src/commands/execution/driver.rs:128) and async runtime driver (core/runtime/src/driver.rs:520), both via `dispatch_candidates`. [CORRECTED per review F3: my first receipt said "five consumers" — that was the graph's INBOUND REACHABILITY count (start/resume/serve reach the derivation through a driver, they never call it); the spend-site enumeration is the honest one (#80's lesson in its original shape), and the parity thesis comes out STRONGER: one derivation, two call sites, both drivers, signature unchanged, so neither edits.] |
| Gating decision per edge is pure | `edge_gates` (ready.rs:79-87): condition==false never gates; `Failure` edges gate unless predecessor Failed; else `!satisfies_dependents(predecessor)` |
| Node states are a closed enum, `WaitingInput` exists | `NodeState` (core/protocols/src/simulation.rs:6-27), 16 variants |
| The fold turns recorded outcomes into node state | `NodeOutcomeRecorded` (core/protocols/src/event.rs:372-388); test `the_recorded_next_state_becomes_the_node_state` (core/events/tests/execution_projection.rs:267-273) |
| Evidence-on-outcome precedent exists | `record_outcome_with_evidence` (core/runtime/src/driver.rs:55-169) |
| Event-variant count is load-bearing in TWO conformance suites | `all_twenty_five_event_variants_are_complete_closed_and_replay_safe` (core/schema-evolution/tests/conformance.rs:888-1113) and `all_twenty_five_safe_event_variants_strictly_round_trip_against_schema` (core/protocols/tests/persistence_wire.rs:104-239) — adding variants renames/extends BOTH (a named cost, not a surprise) |
| Sequence-as-identity precedent | M09's `armed_at_sequence` (envelope sequence discriminates re-arms where name/rdv/cursor cannot — projection.rs, #74) |
| Append-layer has NO fold validation; command layer refuses | #55/#74 archaeology: `append_atomic` checks shape/idempotency/CAS only; wake recorder's two blades (liveness filter + pinned-sequence CAS) are the proven pattern for decide-then-append |

The load-bearing consequence of row 1: **"the fold of readiness lives where CLI and HTTP
read the SAME derivation" is already true and stays true by extending `ready_set`'s
inputs, never by adding a second reader.** The known CLI/HTTP divergence is parallelism
MECHANISM (D synthesis §3, L's declare-the-driver rule) — not readiness derivation; this
design touches only the derivation and inherits parity at both call sites — both drivers.

## 2. The event family — exact schemas

**ELEVEN** new `EventKind` variants — CORRECTED (A's finding, third-party #3): this
document said "nine" and specified eleven, a count that does not resolve against its own
evidence set. There is no criterion under which nine is right; it was simply wrong, and
the correction was DERIVED by enumerating the specifications below rather than recounted
from memory (the method the M10 close used to settle its own merge count). Same family as
`assert_eq!(len, 25)` against a literal, and as "Eighteen merges vs 18 SHAs" — a number
carried beside a list it no longer matches. The list, so the number is checkable against
it: `completion_claimed`, `completion_cleared`, `completion_rejected`,
`completion_refused`, `clearance_identity_registered`, `clearance_identity_revoked`,
`dlq_routed`, `dlq_redrive`, `dlq_returned`, `sweep_performed`, `overdue_exception`.
(Distinct from the refusal REASON-CODE registry, which is nine strings after the
`SignatureUnverifiable` amendment — two different nines were in play, which is how the
wrong one survived reading.)

All of them: wire names snake_case, all payload structs
`#[serde(rename_all = "camelCase")]`; every OPTIONAL field
`#[serde(default, skip_serializing_if = ...)]` — the M07/M09 hash-chain law: absent stays
absent on the wire, a null re-hashes every committed event).

### 2a. The wait's identity (the thing a claim answers)

A node enters `WaitingInput` today via a recorded outcome; the ENVELOPE SEQUENCE of that
recording event is the wait's identity — unique, strictly monotone per stream, and
re-derivable from any journal forever (the `armed_at_sequence` precedent, decided there
for exactly this problem: names and node-ids repeat across re-parks, sequences cannot).
No schema change creates the identity; the fold carries it:
`open_waits: BTreeMap<node_id, WaitIdentity { at_sequence: u64 }>` on the projection —
DERIVED, never stored, valid on replayed projections only (same rehydration warning as
armed_at_sequence, written at the field).

### 2b. `completion_claimed`

```rust
CompletionClaimed {
    execution_id: OpaqueId,
    node: OpaqueId,
    /// Envelope sequence of the EXACT open waiting_input this answers (issue: the
    /// rendezvous). A claim naming a superseded/answered wait is refused (2d).
    completes_wait_seq: u64,
    evidence: Vec<ClaimEvidence>,      // {kind: String, content_hash: WireHash, size: u64}
    attestation: ClaimAttestation,     // {asserter: OpaqueId, mode: OperatorAttested | MachineVerified}
    // NO deadline field on the claim. See THE STAGE-ENTRY DEADLINE RULE below (2b') —
    // review F1/F2: deadlines born only with claims left the un-claimed wait unsweepable
    // forever (the exact 0/4 state), and a claim-anchored instant made redrive's
    // "clock restarts" a contradiction. One rule replaced both.
}
```

### 2b'. THE STAGE-ENTRY DEADLINE RULE (review F1+F2, one arithmetic for every stage)

**Every customs deadline = the occurred_at of the event that ENTERED the stage, plus the
stage's duration declared on the NODE SPEC.**

**FIELD PLACEMENT — CORRECTED (A's finding, third-party #4; orchestrator decided the
shape).** My first text put these budgets in a `completion:` block on the node as though
that block were new. IT IS NOT: `completion` already exists in `schemas/node.schema.json`
with an unrelated meaning — the COMPLETION CONTRACT (`requires` / `forbids` predicates
over the node's output) — used by three example graphs (manual-override-deploy.yaml,
research-to-publish.yaml, software-feature.yaml) and read by the governor. Following my
letter would have overloaded one field with two unrelated senses and typed strictly over
live graphs: flattening, in a field the owner already uses. A stopped before typing and
ran the legal-vs-produced check instead of assuming a new block — the credit is his.

**DECIDED SHAPE — nest, do not add a sibling:**

```yaml
completion:                          # EXISTING block, unchanged meaning
  requires: [ ... ]                  # existing completion-contract predicates
  customs:                           # NEW sub-block, all of #159's declarations
    requires_evidence: [kind, ...]
    budgets:
      wait_within_seconds: u64       # REQUIRED when customs is declared — F1's fix: the
                                     # un-claimed wait is sweepable from the moment it
                                     # parks, before any claim exists
      clearance_within_seconds: u64  # REQUIRED — the sealed quarantine risk
      dlq_within_seconds: u64?       # OPTIONAL — named decision in §4
```

Grounds (orchestrator's): customs IS about completing, so siblings would break the
kinship and mint two confusable fields; nesting is additive and breaks no existing
graph; and it preserves the option to UNIFY later, which siblings would turn into a
schema migration.

**CONDITION 2 VERIFIED BEFORE TYPING — AND ITS PREMISE WAS BACKWARDS (B, graph-read at
the index of 2026-08-20, `core/governor/src/externalize.rs`).** The condition as handed
to me was: "if externalize serializes the whole block, the budgets travel along = new
behavior to declare or scope." Measured, the risk runs the OTHER way, and the two
readers of this block do not even agree with each other:

- `collect_completion_content` (externalize.rs:807-840) is **key-selective and
  permissive**: it walks only `requires` / `forbids` and registers only items carrying
  `expression`. A nested `customs` block is INVISIBLE to it — no budget leaks into
  externalized content. The "budgets travel" risk does not exist.
- `build_completion_control` (externalize.rs:1454-1554) is **key-exhaustive and
  STRICT**: for a node's completion block it matches `contractRef | requires | forbids`
  and its catch-all is `_ => return Err(GovernorError::InvalidAuthoring)`. **A graph
  declaring `completion.customs` is REFUSED AT PUBLICATION until this match learns the
  key.**

So the real consequence is the opposite of the feared one, and better: **no silent
leakage is possible, and forgetting the governor arm is LOUD — publication refuses
rather than half-accepting.** Binding on lane 1: extending `build_completion_control`
with a `customs` arm is part of the field's landing, and the refusal is its own red
(author a graph with customs against an unextended governor ⇒ `InvalidAuthoring`,
observed before the arm exists). Note for whoever writes it: the two consumers'
tolerances DIFFER (one ignores unknown keys, one refuses them) — that asymmetry is
pre-existing and worth a line where the arm lands, so the next reader does not infer a
uniform policy from either half — **filed as #170** (marker issue: what was measured,
what was NOT verified — chiefly whether the split is deliberate — and the question left
unadjudicated), so it stops being folklore inside this document.

**Strict typing applies INSIDE `customs` only** — the surrounding contract keeps its
current permissive shape; three live graphs must keep publishing unchanged, which is
also this amendment's own regression test.

**OPEN QUESTION, recorded rather than silenced:** `requires` (predicates over the node's
output) and `requires_evidence` (what a claim must present) are close relatives — a
future milestone may unify them into one vocabulary. Nesting keeps that door open; it is
NOT decided here, and the question belongs to whoever owns the completion-contract
grammar next (adjacent to #153's finish-predicate work).

**THE AUTHORING→PERSISTED CROSSING IS PART OF LANE 1, NOT AN ASSUMPTION (A's finding,
third-party #5; orchestrator decided the shape).** My §2b' had the fold computing
`entering occurred_at + budget declared on the node spec` — and the budget does not
reach the fold. Verified: `PersistedNode` (core/protocols/src/projection.rs:299-318),
which is what the fold sees through `projection.current_graph`, carries exactly
`node_type, optionality, controls, content_slot_ids, timeout_seconds` — **no
`properties`**, and `completion.customs` lives in properties. Publication normalizes
authoring→persisted and the budgets do not survive sealing. The arithmetic in the table
below was correct and had no inputs.

**Carried, following the `timeout_seconds` precedent — which is literal, one milestone
old, and the same class of defect.** That field's own doc (M08) says it: the bound *"was
already declared in the graph and already demanded by our own linter... and persistence
dropped it"*, and — the sentence worth reusing — *"this is not a new policy; it is a
declaration that did not survive the store."* Customs budgets are the same shape, one
milestone later, another field.

Two properties inherited from that precedent rather than re-invented:
- **Optional + `skip_serializing_if`**, so every graph version already published stays
  BYTE-IDENTICAL: replay re-serializes and re-hashes, and an always-emitted null would
  break their chains (the M07/M09 hash-chain law, third instance).
- **Absence stays ABSENCE — never zero, never a default.** A pre-customs version
  replayed after this field lands has no budget, and the fold must read that as "no
  customs deadline", NOT as a zero duration that would make every stage instantly
  overdue. That is also exactly the G2 opt-in boundary (2b'') seen from the fold's side:
  undeclared means unswept and SAID SO, never silently deadline-zero.

**Two alternatives rejected, with the reason each dies against a cited source:**
- **(b) Put the instant on the event instead of deriving it.** Contradicts M09's
  `WakeLease::matures_in_seconds`, where the fold computes the horizon from the event's
  own `occurred_at` plus a declared duration precisely so replay reads ONE clock. An
  instant on the wire would make two clocks answer "when did this happen".
- **(c) Keep the deadline only inside the sweep.** Then the deadline field the status
  surface renders (lane 4, E's half) is EMPTY for nodes that DID declare a budget — a
  surface lying by construction about the one thing the operator declared. Rejected on
  the same grounds as everything else in this design: no surface may report calm it did
  not measure.

Credit to A, and for the right thing: on hitting the gap he kept `deadline: None` with
the reason written in the field rather than inventing a path or faking a value.

Stage entries and their entering events (the fold computes instant = entering
occurred_at + declared duration; ONE clock source, the envelope, as before):

| stage | entered by | deadline duration |
|---|---|---|
| Waiting (un-claimed) | the wait-recording event (today's parking outcome) — its envelope sequence is ALSO the wait's identity (2a) | wait_within_seconds |
| Waiting re-opened | `DlqReturned` | wait_within_seconds (fresh) |
| Claimed | `CompletionClaimed` | clearance_within_seconds |
| Claimed re-entered | `DlqRedrive` — the fold REBASES from the redrive's own envelope; no new claim event is minted, the claim's testimony (evidence, attestation) stands, only the stage clock restarts. This is review F2's "marker the fold rebases from" schema, chosen over "redrive mints a claim" because re-driving does not change WHAT was claimed, only WHEN clearance is again owed | clearance_within_seconds |
| Dead-lettered | `DlqRouted` | dlq_within_seconds (if declared) |

Consequences, now true AS WRITTEN rather than of two layers out of three: a parked WAIT,
a parked CLEARANCE, and a re-driven claim are overdue by the SAME arithmetic; DlqReturned
routes work back into a state the sweep sees with a fresh deadline on arrival; and
"no exempt state" is a property of the rule, not a promise about its uses. Claims,
clearances and redrives inherit one deadline family (D's fix direction, adopted whole;
the form — stage-entry generalization — is this document's).

Required evidence kinds are DECLARED PER NODE at graph-definition time (spec field
`completion: { requires_evidence: [kind, ...], clearance: ... }` — new node sub-block,
schema ritual applies). Fewer kinds than declared = refused (`EvidenceBudgetUnmet`);
extra kinds = accepted and folded as `unverified_extra: true` on those entries — logged,
never stronger proof (issue text, verbatimly honored).

### 2c. `completion_cleared` / `completion_rejected`

```rust
CompletionCleared {
    execution_id: OpaqueId,
    /// Envelope sequence of the CLAIM this countersigns (same identity discipline).
    claim_seq: u64,
    verifier: ClearanceVerifier,
    // MachineReplay { manifest_hash: WireHash }  — clearance re-hashed the evidence
    //   bundle against the node's declared manifest; deterministic, replayable.
    // Countersign { identity: OpaqueId, key_fingerprint: WireHash } — human clearance;
    //   identity MUST be in the node's declared identity set (2e).
}
CompletionRejected {
    execution_id: OpaqueId,
    claim_seq: u64,
    verifier: ClearanceVerifier,
    reason_code: String,               // registry codes, same table as refusals (2d)
}
```

### 2d. `completion_refused` — refusal is a journal event, with a registry

```rust
CompletionRefused {
    execution_id: OpaqueId,
    node: OpaqueId,
    /// The wait the refused claim NAMED (may be stale — that is often the reason).
    claimed_wait_seq: u64,
    reason_code: String,
}
```

Registry (closed vocabulary, one const table, conformance-tested):
`StaleRendezvous | DuplicateCompletion | EvidenceBudgetUnmet | HashMismatch |
UnknownWait | UnknownIdentity | ClearanceExpired | NotWaiting | SignatureUnverifiable`
(the ninth added by amendment: J's custody finding, 2e). A graph that cannot
finish leaves a legible trail (issue requirement) — and the refusal-vs-refuse split
follows the M09 rule: a refused CLAIM is a faithfully recorded mistake (journal event,
replay succeeds); only an UNINTERPRETABLE journal (a cleared naming a claim_seq that is
not a claim) is fold-Corrupt. Recorded-not-refused for operator errors; refused-replay
only for impossible logs.

**Refusal mechanics (the #74 pattern, verbatim):** the command layer decides from ONE
read — replay, check the wait is open at `completes_wait_seq` with matching node, check
evidence kinds vs the spec, check identity vs the declared set — then appends with the
sequence PINNED FROM THAT SAME READ. A rival landing after the read trips the store CAS
and the command re-derives (bounded retry) or refuses. No append-layer fold validation
is added (row 8 of §1: that layer deliberately has none). Same-key idempotent retry of a
REFUSED claim replays the refusal event — durable honest answer (the #83 S4 lesson:
the stored outcome is written by what actually happened, never a fabricated success).

### 2e. Countersign identities as journaled state

Declared identity set lives in the GRAPH SPEC (published + sealed through the existing
governor path — D-039: one entry road). Runtime changes (rotation, revocation) are
EVENTS, not spec edits:
`clearance_identity_registered { execution_id, identity, key_fingerprint }` /
`clearance_identity_revoked { execution_id, identity }`. The fold's registry =
spec-declared set ∪ registered − revoked, at each sequence — so "who could countersign
at sequence N" is answerable from the journal alone (kill-bar item 3's property, applied
to identity). Verification of an actual signature against key material is OUT of the
fold: the fold compares a JOURNALED fingerprint to a JOURNALED fingerprint, never
touching key material — that is what keeps membership-at-N a pure function of the log.

**CUSTODY — AMENDED (J's lane-2 finding, adopted).** My first text said cryptographic
verification is "the command layer's job at append time, like evidence sealing today"
and MARKED custody as riding the existing keyring. J read the keyring and found it is
`KeyringDocument { format_version, key_id, authentication_tag }` — ONE key id
authenticated by a tag, i.e. sealing machinery for the events key, not a multi-identity
public-key store. So my sentence claimed an existing home for work that has no home:
legal-vs-produced, in my own design. Corrected disposal, J's and better than the MARK:

1. Fold + registry (lane 2): journal-only, no key material, fully guarded.
2. Signature verification: **a DECLARED GAP, refused at the site that would perform
   it** — the command layer must REFUSE to append a `Countersign` clearance it cannot
   verify, with the named code `SignatureUnverifiable` (2d), until custody lands. A
   declared refusal at the executing site beats a field with no consumer
   (declared-gap-vs-hidden-gap, and it means MachineReplay clearance is fully usable
   in lane 2 while Countersign is honestly unavailable rather than silently trusted).
3. Custody shape when it lands, named so the next lane inherits a decision: per-identity
   public keys as verified children of the anchored keyring directory, reusing
   `open_and_verify` / `verify_child_identity` (symlink-never-followed, bounded reads),
   NOT a widened `KeyringDocument`.

### 2f. DLQ and sweep events

```rust
DlqRouted   { execution_id, claim_seq: u64, reason_code: String }
DlqRedrive  { execution_id, claim_seq: u64 }   // back to Claimed; fold REBASES the stage
                                               // clock from THIS event's occurred_at (2b')
DlqReturned { execution_id, claim_seq: u64 }   // return-to-sender: wait REOPENS with a
                                               // FRESH wait_within deadline (2b')
SweepPerformed { execution_id, as_of: PersistedTimestamp }
OverdueException { execution_id, node: OpaqueId,
                   /// Sequence of the STAGE-ENTRY event whose deadline lapsed — the
                   /// EPISODE identity (see idempotency grain below).
                   episode_seq: u64,
                   claim_seq: Option<u64>, policy: EdgePolicy }
```

`sweep(as_of)`: **as_of is an ARGUMENT, journaled, never read from a clock** (issue).
The verb computes overdue = every stage-entry deadline (2b': waits, claims, redrives,
dlq occupancy where declared) strictly before `as_of`, emits one `OverdueException` per
overdue EPISODE in deterministic order (episode sequence order), then `SweepPerformed`.
Replay reproduces the identical exception set BY CONSTRUCTION because the fold's
instants and the sweep's as_of are both journal data.

**Idempotency grain, pinned (review F4): the mark is per EPISODE = the stage-entry
event's sequence.** One exception per episode, ever — a later sweep at any as_of emits
nothing for a marked episode; a RE-ENTRY (redrive, returned) is a NEW episode and is
again exception-eligible once ITS deadline lapses. NAMED CONSEQUENCE, intention not
default: an `advisory`-policy exception that nobody acts on does not re-fire within its
episode — escalation (re-fire cadence, advisory-to-blocking promotion) is a seeded
edge-policy extension, not silent v1 behavior. The alternative grain (per episode ×
as_of) re-emits on every sweep and was rejected as noise that trains operators to
ignore the channel — the #19 cry-wolf lesson applied to exceptions.

**The guarded risk, now a property of the RULE (2b'):** there is one sweeper, one
arithmetic, and no exempt stage — a parked WAIT (claimed or not), a parked CLEARANCE,
and a re-driven claim all carry stage-entry deadlines the same computation reads. A
design review that finds any customs state the sweep cannot reach rejects the
implementation (issue: quarantine cannot park).

### 2b''. The opt-in boundary (review G2, orchestrator-decided — owner's rule applied)

Customs is DECLARED per node; a graph with no `completion:` block keeps today's behavior
— which is the 0/4 behavior. That boundary is real and is handled in three named parts,
none silent:

1. **Load-time warning (authoring surface):** when a node whose kind can produce a
   NeedsInput/WaitingInput outcome declares NO customs block, graph load/lint emits a
   named warning — the gap is loud at authoring time without forcing migration of
   M09-era graphs (compat respected).
2. **Named non-coverage cell in the sealed acceptance:** "graphs without customs
   declarations preserve the 0/4 waiting behavior — MEASURED AND STATED, not implied."
   The acceptance run says what it did not cover, in its own results, so the
   completion claim cannot silently annex undeclared graphs.
3. **Default-on is the direction, seeded:** a named seed for flipping customs to
   default-on in a future milestone once the machinery is proven — the direction is
   default, the timing respects compat (owner's product-first rule, applied by the
   orchestrator's decision on this review).

## 3. Where the readiness fold lives (and does not)

One change, one place: `satisfies_dependents`/`edge_gates` (ready.rs:79-87) learn one
new fact from the projection — **a predecessor in `WaitingInput` whose open wait has a
CLEARED completion satisfies its dependents; Claimed-not-cleared does NOT** (the
false-ready cell, red-first). Mechanically: the fold, on `completion_cleared`, records
the node's outcome transition (WaitingInput -> Succeeded via the existing
NodeOutcomeRecorded discipline OR a fold-internal cleared set consulted by ready_set —
DECISION: the fold emits the state transition itself, keeping `ready_set(spec, states)`'s
signature UNCHANGED so both call sites — both drivers — inherit with zero edits; the
projection's node_states map is already the one input they pass).

Nothing else changes in dispatch: `dispatch_plan`/`ready` (dispatch.rs:28-52) consume
the same states map. The two-dispatch-drivers rule STILL applies to the guard story
(§5): the trap-guard fixture runs on BOTH drivers because both CALL the shared
derivation — the sabotage that proves the guard must be shown red on each path, exactly
as #159's sealed acceptance demands, and the parallelism divergence (D §3) is untouched
and un-inherited: readiness is derivation, parallelism is mechanism.

Status-as-scan-history (§159 layer 4): the per-node timeline
(dispatched -> parked -> claimed -> cleared/rejected/dead-lettered -> consumed) is a
fold-derived `Vec<CustomsScan>` per node on the projection, rendered by ONE shared
struct that both `execution status` (CLI) and the serve status route serialize — plus
the issue's fixture diffing the two serializations byte-wise (the D-039 parity fixture).

## 4. DLQ as a node (topology, not a queue)

The dead-letter node is DECLARED in the spec — `type: dead_letter`, a new `NodeType`
variant joining the STRUCTURAL set that `classify::work_kind` REFUSES to execute
(core/runtime/src/classify.rs:35-44's Err(Unsupported) arm, where Fork/Join/Timer
already live): it is never dispatched, never runs, never completes. It accumulates:
`DlqRouted` folds the claim into the DLQ node's scan history; the node's STATE stays
outside the terminal set so a graph with occupied DLQ cannot satisfy a finish predicate
that requires it drained (edge from DLQ to the finish gate with the edge-declared
policy). `dlq_redrive`/`dlq_return_to_sender` are ordinary mutation verbs (CLI + HTTP +
MCP through run_idempotent_mutation — idempotency, actor attribution, and the #83
setup-before-commit shape all inherited). LEGAL-VS-PRODUCED note, pre-empted: adding a
NodeType that classify refuses means the table admits a value execution never produces —
that is the POINT here (structural node), and the doc comment says so at the variant, so
the #94-family audit finds a decision, not a gap.

Edge policy for exception propagation: new optional edge field
`on_overdue: blocking | advisory | auto_return` (schema ritual; absent = blocking, the
conservative default, stated in the schema's CHANGELOG entry).

**Named decision — dlq_within_seconds is OPTIONAL (§2b'), and absent means DLQ occupancy
raises no time exception.** Grounds: the DLQ is the exception state itself — the
exception already fired to route there, occupancy is VISIBLE in status and — GIVEN a
finish predicate that requires the DLQ drained, whose grammar is owed to #153 — BLOCKS
completion by topology (loud by construction, unlike the silent 0/4 park F1 names; the
loudness claim is conditional on that predicate existing and says so). A second timer on the exception queue is escalation policy — seeded with the
advisory-escalation extension above, not defaulted silently. A node that wants
timed DLQ occupancy declares the budget and gets the same arithmetic as every stage.

## 5. Guard story (house rules applied, not restated)

- **Red-first, the IDENTITY-AT-SEQUENCE cell — AMENDED to its discriminating form
  (J's lane-2 finding).** My sealed cell said "clearance by an identity outside the
  registry-at-that-sequence is refused (UnknownIdentity)" — the PROPERTY is right and
  the ARRANGEMENT was not named, so the cheapest test satisfying it (a
  never-registered identity) passes against three broken implementations J enumerated:
  second-pass validation against the FINAL registry (revocation becomes retroactive),
  the same in reverse (**a clearance signed by someone who could not sign at the time
  is ACCEPTED** — the security-relevant one), and a surface answering "can X sign?"
  from the CURRENT registry. That is the adjacent-fixture/guessable-value rule this
  document already carries, applied against my own cell: an expected value derivable
  without doing the work. **The required red is now the sharp form: X registered at
  sequence c, clearance signed by X naming a claim at sequence b, b < c, and the FINAL
  registry CONTAINS X — must refuse `UnknownIdentity`.** The never-registered variant
  is kept as a cheap companion, never as the headline.

  **SECOND AMENDMENT (J, found while WRITING the fixture, not while planning it) — both
  cells were still under-specified in ways only the keyboard reveals:**

  - **R1 needs its PRECONDITION asserted, not assumed.** The test must FIRST assert that
    the identity IS present in the final registry, and only then demand the refusal.
    Without that landmark the test also passes in a world where the identity was never
    registered at all — i.e. it silently degrades into the cheap companion it was
    written to outrank, and stops measuring what its own name claims. (Absence-guards-
    need-presence, applied to the arrangement rather than to the assertion.)
  - **R2 needs its PAIR.** "A clearance survives a LATER revocation" asserts SURVIVAL,
    so a sabotage that ignores `revoked` entirely leaves it GREEN — the guard cannot
    see the very field it exists to constrain. Co-required member asserting the
    opposite: `a_revoked_identity_cannot_clear_a_later_claim` (revoke X @a, clearance
    by X @b, a < b ⇒ refuse). One member asserts revocation does not reach backwards,
    the other asserts it does reach forwards; neither alone distinguishes a fold that
    reads `revoked` from one that ignores it.

  Both come from the ORDERED WALK; a last-state view answering a per-sequence question
  is #88's named cause one layer up.
- **Red-first, the F1 cell (review-added, now co-equal with false-ready):** an
  UN-CLAIMED wait past its wait_within deadline must be swept into OverdueException —
  today NOTHING in the tree expires a wait (D re-derived: matures_in_seconds is wake-
  lease machinery, all sweep call sites are wake/lease), so the red is expressible the
  moment the fold carries stage deadlines. This is the 0/4 state's own guard; a customs
  implementation green on claims and red-less here reproduces the measured hole with
  more ceremony.
- **Red-first, the false-ready cell** (issue seals it): downstream of a CLAIMED-not-
  cleared wait must report not-ready — today's code has no claim events, so the red is
  the fixture asserting not-ready under a synthetic claimed state; it needs the events
  to exist to even express, so the honest first red is at the FOLD grain: replay a
  journal with claim-but-no-clearance, assert `ready_set` excludes the dependent.
  Sequence-grain assertions throughout (claims pin wait sequences — the
  adjacent-fixture rule: expected values derived from the arrangement's own appends,
  never literals that a fold bug could coincidentally satisfy).
- **Trap-guard precondition on BOTH drivers** (sealed): stale-wait claim produces
  `CompletionRefused{StaleRendezvous}` AND leaves the node parked — fixture must be
  constructable BEFORE the fix per the house trap-guard rule, and run against sync CLI
  and async runtime paths both (two-dispatch-drivers: a sabotage shown red on one
  driver proves nothing about the other).
- **Replay identity** (sealed): property test — fold(journal) == fold(replay(journal))
  state-identical across claims/clearances/rejections/sweeps; evidence hash mismatch at
  replay is deterministic failure. Conformance suites extended (BOTH twenty-five
  variant tests renamed — the named cost from §1).
- **Clearance-sweep guard** (the issue's mandatory risk): arrangement with a cleared-
  never claim past its instant; sweep(as_of) MUST emit its OverdueException. Sabotage:
  exempt Claimed state from the overdue computation -> guard falls at its own assertion.
- **Refusal registry conformance**: every reason_code producible by the command layer
  appears in the registry const and round-trips the wire (legal-vs-produced: grep the
  emitters, not the table).
- Panic-site-named sabotages per guard, sealed casualties, measured nulls recorded —
  the M10 ledger standard, assumed not restated.

## 6. Lanes and sequencing (consuming D's synthesis)

Four implementation lanes as the issue names, in dependency order:
1. **Event family + fold + readiness** (layers 1+2's substrate) — everything else
   consumes it; also #153's precondition ("completion verb with evidence" IS this).
   **BOUNDARY, ratified (orchestrator + A) because §6 was ambiguous and the ambiguity
   was load-bearing:** lane 1 delivers the deadline ARITHMETIC in the fold (stage entry
   + spec budget, §2b') AND the `overdue_exception` / `sweep_performed` pair as EVENTS
   the fold folds. Lane 3 delivers the VERB — the `sweep(as_of)` command, its surfaces,
   and the caller decision. Reason this is not bookkeeping: the F1 cell (an un-claimed
   wait past its deadline is sweep-VISIBLE) is lane 1's own sealed guard, and it is the
   0/4 state's guard; if the arithmetic lived entirely in lane 3, lane 1 could not
   express the cell it is sealed against, and the milestone's motivating hole would have
   no owner until lane 3 landed.
2. **Clearance + identity registry** (2c/2e) — after 1; machine-replay clearance needs
   only the evidence hashes; countersign needs the keyring lane decision (MARKED).
3. **DLQ + sweep** (2f, §4) — after 1; independent of 2's countersign half.
4. **Status scan-history** (§3 tail) — after 1; closes #133/#134's surface half.
Cross-cut with #153: lanes 1+2 land BEFORE the process-executor driver consumes them
(D §2's table row 3); the parallelism collision (D §3) is explicitly NOT this design's
to resolve — customs adds no concurrency and reads no `parallel_limit`.

## 7. Risks carried visibly

- **Schema ritual ×9 variants + 2 node/edge spec fields**: old-digest-first, both
  catalogs, CHANGELOG, omit-when-absent on every optional — the M09 capturedArming
  ritual, nine times. Cost named; a lane that shortcuts it fails review at R1.
- **Fold growth**: customs state enlarges the projection every consumer folds. The #87
  frozen-projection-digest gate (acceptance-map) may move if any frozen demonstration
  ever records customs events — same fixture-dependent caveat as M09's, checked per
  demonstration before re-record (re-observe, never re-reason).
- **Two conformance suites hard-count variants** — renamed in lane 1's commit, called
  out in its body (author-summary rule: the diff shows it, the body says why).
- **Clock discipline**: exactly ONE instant source exists in the family — the envelope's
  occurred_at (claims derive clearance instants from it; sweep takes as_of as data).
  Any implementation reading SystemTime in fold or sweep is a blocker, not a nit.
- **PROCESS FINDING, earned three times on this document: a sealed cell is a HYPOTHESIS
  until someone writes its fixture.** All three amendments to lane 2's cells (the
  discriminating UnknownIdentity form, R1's missing precondition, R2's missing pair)
  were found by the agent going to WRITE the fixture, none by the agents who planned,
  reviewed, or sealed them — including me, twice, on cells I authored while holding the
  rules that each violation breaks. The house rule "trap-guard before the fix" should
  therefore extend one step earlier for design lanes: **trap-fixture before the SEAL** —
  a cell whose fixture nobody has attempted is not yet evidence about anything, and the
  cheapest moment to learn that is before it is written into an issue as sealed.
- **DECLARED LIVENESS DEPENDENCY (review-carried): nothing forces sweep to be called.**
  as_of-as-argument is right for replay and it means the anti-parking property rests on
  a CALLER that this blueprint does not create. Until one exists, "quarantine cannot
  park" is discipline, not mechanism (#152's own distinction). Candidate callers, seeded
  for the lane-3 issue to decide: operator/cron invocation; piggyback on mutation routes
  (the wake-sweep precedent — every successful mutation already fire-and-forgets a
  sweep); a serve-side periodic tick. The property's honest statement until then:
  "no state the sweep cannot SEE" (mechanism, delivered) + "the sweep gets called"
  (liveness, owed).
- **MARKED, not designed here**: countersign key custody (rides keyring), the finish-
  predicate grammar the DLQ edge feeds (belongs to #153's gate-graph), Studio rendering.

## 8. What this blueprint does not decide

Issue numbers per lane (orchestrator files), the exact reason-code string table beyond
the eight named (implementation lane freezes it in a const with its conformance test),
whether `dead_letter` nodes may be auto-materialized when any node declares customs
(v1: DECLARED only — auto-materialization is governor territory, later), and the
process-executor driver's consumption of cleared-events (belongs to #153's lane, which
reads this design's layer 1+2 as its API).
