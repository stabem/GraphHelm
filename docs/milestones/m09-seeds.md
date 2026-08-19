# M09 seeds — what is still standing when M08 closed

> **Status:** open work, not history. The milestone record for M08 is
> [`ask-once-and-sleep.md`](ask-once-and-sleep.md); this file is deliberately separate, because
> a milestone record is a closed story and a seed is unfinished work, and mixing them leaves the
> next reader unable to tell which is which.

M08 closed after nine blind-judge runs. The defect that named it is dead, with the proof in
bytes. What follows is what the ninth run still said, plus two items the pair found on itself.

## How the naming finding ended, stated precisely

The finding M08 existed to remove — the one-glance answer permanently reading `unknown` — is
gone. The ninth run's transcript settles it without anyone being believed:

    8   POST amend-budget  200  attention="needs_you"  silenceUnevaluated=[]
    10  POST amend-budget  200  attention="needs_you"  silenceUnevaluated=[]
    11  GET  status        200  attention="needs_you"  silenceUnevaluated=[]

In run eight the read at that position reverted to `unknown` with the node repopulated, twice.

An objection still occupies that finding's number, and it must not be read as the same
complaint weakened. It **changed species**:

- **Before:** an assertion that the mechanism fails. That is a defect, and a defect dies to
  evidence.
- **Now:** an argument about a default — why must the operator declare a per-node bound before
  the monitor can speak at all? That makes no factual claim measurement can refute, so no
  amount of evidence retires it. It is answered by a decision, not by a fix.

Recording it as a "downgrade from critical to high" would invite a later reader to treat it as
nearly solved. It is not nearly solved; it is a different kind of question.

## Seeds, in the order they are worth taking

### 1. Evidence that will not open is not evidence — RESOLVED in M09 (#75/#77)

Two journals committed under `docs/acceptance/` fail integrity verification, and they fail at
the parent commit — measured, pre-existing, nobody's regression. This ranks first despite
being the oldest, because those files are cited as proof in an acceptance document. A product
whose thesis is "history that reproduces" cannot have committed history that does not open;
the failure contaminates the argument, not just the files.

**Closed:** a new test (`tools/acceptance-map/tests/committed_stores.rs`) opens and replays
every committed store by directory rather than re-hashing it through a binding that could miss
one; the missing shape was two empty directories git cannot carry, restored on open. Landed on
`main` as `df5e431` (#77, closes #75).

### 2. What is the default when nobody declared anything?

Today the answer is `unknown` — honest, and expensive. The judge's standing objection is that
requiring per-node configuration before the surface will answer is the work the surface exists
to avoid. This is a product decision with real alternatives (a declared default that says it is
a default; refusing to start work whose silence cannot be judged; a per-graph rather than
per-node bound), and each alternative has to survive the same rule: absence must never be
laundered into calm.

### 3. A run wedged in the queue never wakes anyone — RESOLVED in M09 (#70)

Silence is evaluated only for `running` nodes. A node that failed, requeued, and then sat in
`queued` for over two minutes reads as healthy — even after a bound was explicitly declared for
it. This is the same shape as the wedge rule's deliberate refusal to treat `Running` as stuck,
one state over, and the two cases genuinely differ: forcing `running` to mean "stuck" would
make the rule lie, while `queued` after a retryable failure is not progress by any reading.
That distinction only surfaced because someone tried it and the world answered.

**Closed:** silence is now keyed on whether a node has been REACHED, not on `state == Running`.
Landed on `main` as `efd85d0` (#70).

### 4. The doorbell cannot ring for silence — RESOLVED in M09 (arming work)

The wake lease rings on content appends past a cursor, so a silent hang produces no ring by
construction — the one condition an operator most needs to be woken for. Seven consecutive
judge runs named this. It is the oldest surviving finding in the set.

**Closed:** arming now declares how long quiet may last (`maturesInSeconds`); the wait matures
on that bound with no further content required, and the matured timeout IS the evidence of
silence — proven live in the second judge story's paid run (issue #83 aside, the arming
mechanism itself fired as designed). Five commits on `issue-m09-arming-the-alarm`
(`abd2f2c..53d212d`), not yet on `main` as of this writing — see the milestone close record for
landing status.

### 5. `needs_you` carries no remedy and no urgency

`silence_unevaluated` reasons carry an operation and a remedy; `silent_node` reasons are bare.
The operator learns they cannot sleep, and not what to do or how bad it is. The machinery to
fix this already exists — it is the same `Remedy` carried one variant over.

### 6. A counter that gives a false negative on a landed action — RECONFIRMED in M09

`acceptedMutations` stays `0` after amendments are accepted and the head moves. An operator
using it to confirm their intervention landed is told it did not.

**Still open, fresh evidence:** the M09 second-story paid judge run reproduced this
independently — `acceptedMutations` reported `0` across the entire incident, through five
head-sequence advances (17→35) and multiple accepted mutations
(`docs/acceptance/m09-judge-run-2026-08-19/verdict.json`, finding 6, medium severity). Not
fixed this milestone.

### 7. Monitoring inflates the counter it is read with

Arming a wake lease advances `headSequence` while `contentHead` stays put, so a read-only act
moves the number an operator reads as progress — and the number the remedy's freshness token is
built from. `contentHead` exists precisely for this and is not used everywhere it should be.

### 8. Auditing the judge should be routine, not exceptional

`serve --read-audit` made the judge auditable, and the pair used that once: run seven's F8
claimed two fields were duplicated "verbatim" and the payload had "~20 top-level fields";
measured from the served bytes, the fields differ by a discriminator and there are 14. The
finding stood on substance and overstated in wording.

That check should run on every verdict, before acting on it. A finding we act on because it is
right must not also teach us to trust numbers nobody checked. This one is about the pair, not
the product, and it belongs here because it will otherwise be rediscovered the expensive way.

### 9. A flaky test on the path seed 4 attacks — RESOLVED in M09 (#73)

`wake_http::a_sleeper_wakes_on_a_peer_append_with_zero_requests_in_the_window` fails by
assertion — not the transport-level timeout of the known flake family — at **12/13** measured
runs (9/10 isolated, 3/3 in-suite; source: `.factory/h-agent-base-measurements.md`), not the
"roughly three in four" first estimated; H's measurement calls it near-deterministically red at
this base, not a classic flake. Whether it fails at the parent commit too is a **registered
prediction, unmeasured** (prediction ledger C-P1, N>=10 required for a verdict) — not a stated
fact. It sits in the wake path, which is exactly what seed 4 is about, and the two were taken
together, as this line predicted.

*(CITE-or-MARK note, applied to this line itself: the original "roughly three in four" named no
invocation, N, or base, and is withdrawn as UNCITABLE rather than as disproven — the number was
never entitled to be cited, not shown wrong. See seed 8 above ("Auditing the judge should be
routine, not exceptional") for the rule this same document states and this line originally
broke — cited by SEED NUMBER rather than by line, deliberately: this file's own line numbers
shift on every edit, which is exactly the trap a coordinate citation falls into. A prior
version of this note cited `m09-seeds.md:92-93`; that citation went stale the moment this
paragraph was inserted above it, which is the demonstration, not just the risk.)*

**Closed:** fixed by a condition-wait replacing the timing-dependent immediate `wake-lease`
read. Landed as part of #73 (`576e553` on `issue-m09-arming-the-alarm`, not yet on `main` as of
this writing — see the milestone close record for landing status).

### 10. Surface the mis-burn to attention

`wake_mis_burns` is now populated by the fold — every consumption that burned an arming other
than the one it named gets recorded (`at_sequence`, `captured_arming`, `live_arming`, keyed by
session, last-wins) — and nothing yet tells an operator. The acceptance shape is the point, not
a detail: a guard for this seed must prove the operator SEES the mis-burn, not that the field
EMITS it — the exact distinction this milestone's oracle-grain audit measured elsewhere (the
belt reports green with 14 of 15 consumptions missing; a populated-and-never-read field is the
same shape of defect in miniature). Deferred deliberately: the fix that populates the field
kept its own property measured; operator-surface work needs its own oracle design (what the
operator sees, when, what proves they saw it), and shipping either unmeasured or doubled the
change. Source: issue #74's PR (`d10916b`).

### 11. A CLI that can print a schema digest — RESOLVED in M09 (#78/#85)

`graphhelm schema catalog` could compare digests and refuse on mismatch, but could not PRINT
one — so every schema ritual that changed a schema needed a number the tool had no way to
produce, and paid for it with a throwaway test written, run once, and deleted, repeated each
time. **Closed:** `schema digest` prints the canonical digest directly. Landed on `main` as
`326799b` (#85, closes #78) — this seed was raised and resolved within the same milestone.

### 12. The rendezvous-EQUAL burn's remaining slice — and what can never be known about it

Issue #74's recorder-side discriminator closes this defect going forward. Two things stay
open, one of them permanently: the fold-side check is forward-only by design (`capturedArming`
is absent on every consumption committed before the fix, and the fold treats absence as
permissive — inventing a mismatch from a field never written would make all pre-fix history
look defective). And no analysis can ever decide whether this defect fired in already-committed
history — replay can say which lease a consumption burned, nothing can say which lease the
sweep MEANT to burn, because only the live side of that comparison was ever written down. The
strongest claim the old data can support is "the precondition was present, no incident was
observed," never "no incident occurred" — a future reader must not search the archives, fail to
find the incident, and conclude it never happened. The precondition IS present in real
committed history (one session armed the same rendezvous four times running, verified in the
archived pair store).

### 13. `ring()` collapses every error into one reason, and `wake_wait` never consults the receipt

`ring()` matches every I/O error, on both the pipe open and the write, into a single
`StaleRendezvous` — a deliberate simplification for the operator-facing reason, but it means a
sleeper that is genuinely ALIVE and momentarily unavailable (`ERROR_PIPE_BUSY`) is classed
identically to one that is truly gone. If that case is reachable, the consequence is silent: the
lease is consumed and RECORDED consumed with reason `stale_rendezvous`, blaming a live sleeper
for dying, and no later append rings it because the lease is gone — `wake-wait` reports its
deadline passed and never consults the receipt to learn why (`wake_wait.rs`). UNMEASURED
whether `ERROR_PIPE_BUSY` is actually reachable against this sidecar's single-shot pipe
creation — the classification collapse is a verified fact; the hazard's reachability is a
hypothesis and must stay labelled as one until measured. Source: found by L, verified by C.

### 14. Two tooling seeds, named rather than filed as code defects

- **A citation validator exists as a parked proposal, not yet generalized.** K's tool
  (`.factory/k-agent-cite-validator-proposal.md`) resolves a `file:line` citation against a
  named base and prints the line it actually names — the exact class of defect this milestone
  paid for repeatedly (one worker opened 4 of 40 wrong citations found this milestone by
  reading; the rest surfaced only by a wider radius-mapping sweep). Honest state: hardcodes one
  file/base today; generalizing it, and deciding working-tree-vs-`git show` resolution (the
  latter would have PREVENTED the defect it is named for), is the remaining work. If it becomes
  a CI gate, it needs a red first, per this milestone's own vacuous-red rule.
- **A cross-crate dependency has no name in either crate.** The wake recorder's honest count
  depends on `request_digest` covering `expected_next_sequence`, stated nowhere near either
  site. Measured: blinding that field fells 5-7 PRE-EXISTING guards elsewhere in the crate — the
  property is INCIDENTALLY COVERED, not unguarded, so it is a naming/diagnosability problem, not
  a coverage gap. A maintainer who breaks it by accident gets 5-7 unrelated-looking reds and
  diagnoses backwards. Cure: a declaration at the edit site naming the dependent and the
  executable statement that enforces it, not a new test.

## What is NOT here, and why

No seed asks for a new blind-judge run to "prove" a fix. Nine paid runs bought the findings
above; the recorder makes the judge auditable but does NOT make his scenario reproducible for
free — a node in flight, silent, and unbudgeted requires the real model call. That limit was
found by trying, and it is written here so the next planner does not budget on the belief that
verification became free.
