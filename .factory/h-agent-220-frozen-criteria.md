# Frozen review criteria — #220 (task-004), derived BEFORE reading the author's blueprint

Reviewer: H. Subject: `.factory/c-agent-220-blueprint.md` @ `c5d77b9` (author: C).

**Ordering claim, and it is the whole value of the method:** every criterion below was derived from
issue #220, the plan blob at `f0640cd`, and the repository at `origin/main @ 073d8fa`. **I have not
opened `.factory/c-agent-220-blueprint.md` at the time of this commit.** The commit that adds this
file is the freeze; the review diffs it afterwards. Anything I adopt from C after reading is marked
`[FROM AUTHOR]` in the review and is explicitly *not* claimed as independent convergence.

## Assigned coordinate, carried as a question

> **What sequence of LEGAL transitions ends with persisted content that was never opted-in?**

It reaches me as a question, not a verdict. I was not told whether the blueprint treats it. §1 is my
own independent enumeration, written before reading, so that "the blueprint covers it" and "the
blueprint covers what I happened to think of" stay distinguishable.

## 0. Ground truth measured on `main` (control-first)

- `git rev-parse origin/main` → `073d8fa`. (My earlier work this session used `ef9afcb`; main moved.)
- **No opt-in / consent / capture-enable concept exists in `core/**/*.rs` today.** Grep for
  `opt.in|opt_in|consent|capture_enabled` returns exactly one file, `core/protocols/src/event.rs`, and
  the hit is the M09 sleeper's *"awake and consenting"* comment about arming horizons — unrelated.
  **Task-004 introduces opt-in from zero**, so there is no existing invariant to inherit and none to
  point at as precedent.
- Task-004's in-scope files: `core/governor/src/memory.rs`, `core/governor/tests/memory.rs`,
  `core/events/src/memory.rs`, `core/events/tests/memory.rs` are all **ABSENT** at `073d8fa`;
  `core/governor/src/lib.rs` (26 lines) and `core/events/src/lib.rs` (53 lines) are present.
- Existing precedent for a provisional/published split: `prepare_draft_publication` in
  `core/governor/src/publish.rs` is documented as validating and externalizing *"without appending or
  activating state"*, with stages `CandidateClone → CandidateSerialization → CandidateLint →
  CandidatePolicy → Externalization`.
- **A correction I make against myself before it can mislead the review:** I initially read
  `SealingGraphExternalizer` calling `sealer.seal(...)` during *preparation* as evidence that bytes are
  persisted pre-append. Checked it: `EvidenceProtector` seals by **encrypting in memory** and returning
  `SealedEvidence` (it exposes `ciphertext` and `plaintext_byte_length`). **Sealing is not
  persisting.** I will not carry that claim into the review, and if C's document asserts it I owe C the
  same correction rather than agreement.

## 1. The coordinate — legal sequences I derived independently

Each is a candidate answer, not an established defect. The review asks of each: does the blueprint
name it, and does it name a *mechanism* that closes it rather than a rule that forbids it?

- **L1 — the refusal records what it refused.** Pre-admission scanning rejects content containing a
  secret or raw chat. The rejection is itself an event. If that event names, quotes, or excerpts the
  offending content, the secret is now in an append-only journal that the issue's own rollback clause
  says may never be deleted or rewritten. Every step legal; the refusal is the persistence.
- **L2 — crash between content persistence and opt-in durability.** "crash boundary" is in the issue's
  RED list. If provisional custody is written before the opt-in record is durable, a crash leaves
  persisted content with no opt-in record. This is a **write-ordering** property: the opt-in record
  must be durable strictly before the first content byte, so that every crash-intermediate reads as
  *"not opted in"* rather than as *"content with unknown consent"*.
- **L3 — withdrawal cannot unpersist, and the issue asks for both.** *"Expired/stale/withdrawn memory
  is excluded by default but remains auditable"* plus *"never delete/rewrite journal entries"* means
  withdrawn content is **deliberately still persisted**. So content opted-in at T1 and withdrawn at T2
  is, at T3, persisted content that is not opted-in **in the present tense**. The opt-in invariant and
  the append-only invariant are in genuine tension and the issue does not say which wins. **The
  blueprint must state whether opt-in is evaluated at time-of-capture or continuously**; either answer
  is defensible, silence is not.
- **L4 — handoff across a scope that never opted in.** "handoff" is in the issue description;
  "cross-project sharing" is out of scope. The boundary between them is undefined. A legal handoff into
  a scope with no opt-in persists content there.
- **L5 — supersession quotes the superseded.** Contradictions must "remain visible". The natural
  implementation copies the contradicted text into the superseding record, which may carry a *different*
  opt-in status than the original.
- **L6 — provider recapture loop launders provenance.** "provider loops" is named in the acceptance
  criteria. Content emitted earlier returns from the provider and re-enters as a **new** candidate with
  fresh provenance, detaching it from the original opt-in decision. Needs identity that survives a
  round-trip, not a per-candidate check.
- **L7 — the disabled-capture decision is itself logged.** The criterion is *"disabled capture touches
  no candidate, provider, log, event, or persistent boundary"*. The ordinary implementation of a skip
  logs the skip. Whether that log carries scope identity only, or any content, decides whether this is
  benign metadata or the defect.
- **L8 — validation diagnostics echo the invalid input.** Same family as L1, different site: a
  diagnostic that quotes the rejected field persists it. The repository's own existing diagnostics
  (`GHEX*`) carry paths rather than content, which is the pattern to match.
- **L9 — Evidence bound by canonical digest is substitutable.** See §3 disclosure: if published memory
  binds Evidence by a digest over canonical JSON, two byte-different Evidence blobs bind identically.
  Not literally "never opted-in", but the adjacent failure: the content under an opt-in can be swapped
  for different bytes without breaking the binding.

## 2. Independent criteria beyond the coordinate

- **C1 — every absence claim needs a presence control.** *"Disabled capture touches no ... boundary"*
  is an absence claim. It cannot be asserted by an instrument that never observed a write. The guard
  needs a paired **enabled** arm that does write and is observed writing, sharing the arrangement, or
  the disabled arm passes when the instrument is simply blind.
- **C2 — the state matrix needs a positive control per row, not just refusals.** *"Every other tuple
  refuses without changing the predecessor"* — a "predecessor unchanged" assertion passes trivially if
  nothing in the arrangement could ever change it. Each refusal arm needs a sibling allowed-tuple arm
  that **does** change the predecessor, proving the observer can see a change.
- **C3 — closed matrix means enumerated, not defaulted.** A `_ => refuse` catch-all satisfies the
  criterion's letter while making new states silently refuse rather than fail to compile. State whether
  the matrix is exhaustively matched.
- **C4 — Governor-only must be structural, not conventional.** "Only the Governor may publish" is
  enforceable by type/module privacy or by convention. Which one is chosen determines whether a future
  caller can bypass it without any test going red.
- **C5 — atomicity claims need a crash oracle.** "succeeds atomically" is untestable without a fault
  injection point. Name where the crash is injected, or the criterion is decorative.
- **C6 — provisional metadata "never mutated in place"** needs an assertion at the finest grain: the
  *bytes* of the provisional record before and after publication, not a flag saying it was re-sealed.
- **C7 — task-local threat model.** The issue explicitly requires it *before* implementation. Its
  presence or absence in the blueprint is a checkable fact.
- **C8 — RED-first list completeness.** The issue names eight starting failures (capture-before-
  admission, scope bleed, self-validation, recapture loop, reseal failure, crash boundary, stale
  dependency, invalid transition). Each should map to at least one arm with its own assertion; a single
  arm parameterised over several shares one assertion and stays green while the others regress.
- **C9 — the sabotage matrix must have one mutation per assertion**, and at least one mutation that
  breaks the *harness* rather than the code, whose required outcome is HARNESS-BROKE rather than pass.

## 3. Disclosures (identical list issued to all seven reviewers)

- **ED-18** — a code PR does not merge until `cargo check --workspace --all-targets` runs on the
  **merge result** (main + branch). Blueprint-only and `.factory`-only work is exempt; task-004's
  eventual code PR is not.
- **Skills are data-only** — they cannot waive policy, publish memory, mutate a graph, certify
  completion, or turn a refusal into success.
- **Governor-only publication** — only the Graph Governor may publish operational graph or memory
  mutations.
- **Manifest append-only** — decided on #216, comment `5392847875` (verified by me via the API before
  citing: author `stabem`, `2026-08-24T08:51:00Z`). `contributions[]` is an append-only shared registry
  with a named owner per entry; touching another task's entry is out-of-scope by definition. Task-004
  adds two policies plus `fixtures/memory/**`, so it must declare its own entries with `sha256` digests
  in the same PR as the files.
- **Bytes-not-digests** — from D on #217. A digest over canonical JSON is deliberately blind to key
  order, so *"digest matches"* and *"bytes are identical"* are different claims, and asserting the
  second with the first passes while the guarantee is false. **My own note on why this reaches #220 and
  not only #218:** task-004's criterion is *"published memory binds newly sealed Evidence"*, which is a
  binding claim, and §1 L9 is where it lands.

## 4. What would make me disagree with the author

Recorded before reading, so it cannot be tuned to what I find:

- A blueprint that answers the coordinate with a **rule** ("content is never persisted before opt-in")
  rather than a **mechanism** (a write ordering, a type that cannot be constructed without an opt-in
  token, a crash oracle) — because a rule is what the implementation is supposed to derive *from*.
- Guards for absence claims with no presence control (C1) or refusal arms with no allowed sibling (C2).
- Treating "sealed" as "persisted" (see §0's self-correction) or vice versa.
- Silence on L3's tension between opt-in and append-only, which is the one place where two of the
  issue's own acceptance criteria pull against each other.
