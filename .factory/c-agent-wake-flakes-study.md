# C Agent — mechanism study: the two wake_http flakes

Read-only study on branch `issue-m09-arming-the-alarm` (tip 53d212d), checked out as
`c-study-arming-the-alarm` in worktree c-agent-e82f40. NO build, NO test was run (W1
measurement in progress). Everything below is from reading code; every failure claim is
labeled HYPOTHESIS (NOT MEASURED) unless it is a structural fact visible in the source.

ASCII-only. File references are to this branch's tree.

---

## Shared mechanism map (both flakes live on this machinery)

### The store's locking, as it actually is on this branch

- `LocalEventRepository::open` (core/events/src/local.rs:318) takes root-exclusive +
  named-lock-exclusive, runs recovery (validate anchors, load, sync, reconcile orphans,
  publish active markers), then RELEASES BOTH locks before returning (local.rs:390-397).
  **An open handle holds NO lock between operations.** Any doc claiming "the handle's
  lock spans X" is describing per-OPERATION locks, not handle-lifetime locks.
- Every operation goes through `with_lock` (local.rs:540): in-process mutex (per-instance,
  irrelevant across handles) -> root-handle exclusive lock -> named `repository.lock`
  file lock, SHARED for reads / EXCLUSIVE for appends (since 53d212d), `validate_anchors`
  before and after the operation, then release. One acquisition per method call.
- `append_locked` (local.rs:644) re-loads state under the exclusive lock and REFUSES a
  stale sequence: `actual != request.expected_next_sequence()` -> `SequenceConflict`
  (local.rs:684). This is the store's only optimistic-concurrency backstop.

### The sweep (apps/cli/src/commands/serve/wake.rs)

Every successful mutation route (`run_idempotent_mutation`, serve/mod.rs:738, spawn at
:806) fire-and-forgets `wake::sweep`. Sweep is three phases, EACH with its own store
open and its own lock acquisitions, with unlocked gaps between all of them:

1. Phase 1 (spawn_blocking): open store, `list_streams` (shared), `read_replay_stream`
   (shared), fold, compute `content_head` = max sequence of non-wake events, collect
   leases with `cursor < content_head` (wake.rs:84-122).
2. Phase 2 (async): ring each rendezvous. Missing pipe -> instant `StaleRendezvous`.
3. Phase 3 (spawn_blocking): `record_consumptions` (wake.rs:150) — open store,
   `list_streams` (shared), `read_replay_stream` (shared) = the LIVENESS RE-VALIDATION,
   fold, filter to leases still live with the SAME rendezvous, `next_sequence` (shared),
   `append_atomic` (exclusive) at that sequence.

**Structural fact:** the comment at wake.rs:137-139 and :167-170 claims the re-validation
and the append are atomic ("The handle's exclusive lock spans read and write"). On this
branch that is FALSE. They are four separate lock acquisitions with open windows between
them. `open` releases its locks (see above), so nothing spans them.

### The fold's poison rule (core/events/src/projection.rs:990)

`WakeLeaseConsumed` for a session with no live lease -> `ReplayError::Corrupt` -> the
WHOLE stream refuses every later replay. A double-consume is not a wrong counter; it is
a permanently poisoned stream. Also: the consumption event carries NO rendezvous id, so
the fold cannot distinguish "consumed the lease you think" from "consumed the re-armed
replacement" — only the recorder's rendezvous-equality filter defends that, outside the
fold.

### The sidecar (apps/cli/src/commands/wake_wait.rs)

Reads its OWN session's lease once (fresh store open, dropped before blocking), derives
rendezvous + absolute deadline from the lease, creates the named pipe first-instance,
blocks on connect+read under `tokio::time::timeout(declared bound)`. Exit 0 rung, 3
matured, 2 refusal. Note the STALE doc at wake_wait.rs:38-42 ("repository holds an
OS-level exclusive lock for the handle's lifetime") — no longer true on this branch;
harmless to behavior (dropping early is still right) but wrong as stated.

### Arming (apps/cli/src/commands/execution/wake.rs)

`armed_cursor = cursor.unwrap_or(RAW head)` (wake.rs:76-77) — raw head, not contentHead.
In both flaky tests prior history is all content, so the two coincide; the arm-triggered
sweep (every mutation spawns one, including the arm itself) is not-due by
`cursor < content_head` being false. Fine today; a latent trap if bookkeeping ever
precedes an arm.

---

## Flake 1: `concurrent_sweeps_never_double_consume_a_lease` (~1 in 3 on base)

(Taking this first because its mechanism is fully visible in source.)

### Test shape (apps/cli/tests/wake_http.rs:947)

15 rounds: arm lease (no pipe -> stale path = tightest window), two signal POSTs
released by a real `Barrier(2)`, **fixed 600 ms sleep**, then oracle = `execution
status` must still replay (`ok == true`); end belt = journal-wide `consumed <= armed`.

### Timing assumptions (assumes-timing-instead-of-waiting-for-a-condition list)

- T1. `sleep(600ms)` at wake_http.rs:986 as "sweeps have finished their follow-up
  appends". Nothing waits for the consumption events to land. A phase-3 append that
  lands late is checked only by the NEXT round's oracle or by the belt.
- T2. The end-of-test belt `consumed <= armed` can false-PASS a double-consume: one
  double (2 consumes / 1 arm) plus one round whose consumption was legitimately dropped
  nets to <= armed. The per-round replay oracle is the real blade; the belt is coarser
  than it looks.
- T3. Implicit: round r+1's arm assumes round r's sweeps are done (600 ms). A straggler
  sweep from round r is defused only by the rendezvous-equality filter in the recorder
  (`rdv-race-r` != `rdv-race-r+1`) — which NO unit test currently pins (the two tests in
  serve/wake.rs cover rival-consume-drop and live-record, not rendezvous swap).

### Ranked failure hypotheses

- **F1-H1 (primary). HYPOTHESIS (NOT MEASURED): the validate/next_sequence gap lands a
  double-consume.** Interleave, two racing recorders R1 R2 (one per barrier'd signal):
  R1 validate-read (lease live) -> R1 next_sequence = N -> R1 append @N (consume, ok)
  -> R2 HAD validate-read BEFORE R1's append (lease live) -> R2 next_sequence = N+1
  (fresh, read AFTER R1's append) -> R2 append @N+1 SUCCEEDS -> two consumes, one lease
  -> fold Corrupt -> oracle `ok:false`.
  The SequenceConflict backstop only catches the interleave where BOTH read the same N.
  The idempotency key embeds `next` (wake.rs:200), so the keys differ in exactly the
  dangerous interleave — the dedup defeats itself.
  What decides it: on a failing run, the tempdir journal shows two `wake_lease_consumed`
  for one `wake_lease`, consecutive sequences, both actor `system-wake`. W1 can capture
  a failing run's journal (test prints the status `value`; the journal path is the
  test tempdir — capture before cleanup, or re-assert with the journal dumped). A
  deterministic decider: a test-only failpoint pausing the recorder between its
  validate-read and next_sequence while a rival consume is injected — current code goes
  red, single-acquisition code cannot.
- F1-H2. HYPOTHESIS (NOT MEASURED): late phase-3 append from a PREVIOUS round crossing
  T1's 600 ms and colliding with the next round. Reading says the rendezvous filter
  defuses it (drop, not double), so this should produce a false CALM, not a failure —
  unless combined with F1-H1's window. Decide by timestamps in a captured failing
  journal (which round's rendezvous the doubled consume names).
- F1-H3 (weak). HYPOTHESIS (NOT MEASURED): the failure is not a double-consume at all
  but a transient Storage/lock error making `execution status` itself fail (`ok` absent/
  false without Corrupt). Decide from the failing run's printed `value`: Corrupt-shaped
  refusal vs storage-shaped error. The assertion message already prints it — the
  captured text alone settles H3 vs H1.

### Draft sabotage list for the eventual fix (each must be shown to fail individually)

Assuming the fix is the single-acquisition conditional append (see stash assessment):

- S1. Revert the recorder to the two-acquisition shape (validate in one lock, append in
  another) -> the deterministic failpoint interleave test MUST go red (this is the pin
  on the actual defect; the 15-round statistical test alone cannot be the guard).
- S2. Make the in-lock decision ignore liveness (record unconditionally) -> the existing
  `a_rival_consume_between_read_and_record_appends_nothing` unit test falls.
- S3. Match by session only, drop the rendezvous equality -> NEW guard needed: stale
  capture (rdv-A) vs re-armed lease (rdv-B, same session) must record NOTHING; today no
  test pins this filter.
- S4. Key the consumption idempotency WITHOUT the sequence (fixed key) -> a guard that
  two legitimate consumptions in different rounds both land must fall (dedup would
  swallow the second).
- S5. In the fold: tolerate consume-without-lease (skip Corrupt) -> the oracle test must
  fall — proves the oracle measures the poison and not a coincidence.
- S6. Replace the 600 ms sleep with a bounded wait-for-consumption condition, then
  sabotage the wait to zero -> the guard must still measure the same property (i.e. the
  property must not silently depend on the sleep).

---

## Flake 2: `a_sleeper_wakes_on_a_peer_append_with_zero_requests_in_the_window`

**Rate: 12 failures in 13 — 9/10 isolated, 3/3 in-suite, from H's per-run table
(`.factory/h-agent-base-measurements.md`), measured at 53d212d.** Fails by assertion, not by
the transport-level timeout of the other known flake.

This heading previously read "~3 in 4". That figure was inherited from
`docs/milestones/m09-seeds.md` seed 9 and repeated here without measurement; H's table refutes
it, and at 12/13 the guard is not behaving as a flake at that base — it is near-deterministically
red. Corrected rather than quietly replaced, because this file was already shared and the wrong
number had started travelling into the close doc. Found by N, who told me instead of working
around it.

### Test shape (wake_http.rs:690)

MCP `wake_arm` through a counting TCP proxy (bound 30 s) -> spawn real `wake-wait`
sidecar -> condition-wait for pipe visibility (10 s bound, wake_http.rs:733) ->
snapshot proxy connection count (`at_sleep`) -> waker POSTs signal direct at serve ->
sidecar must exit 0 within 10 s -> `at_wake == at_sleep` (zero polling) -> MCP `events`
re-read sees the waker's event -> immediate GET `wake-lease` must show `live:false`,
`lastConsumed.reason == "rung"`, `atSequence > armedCursor`, `head >= rang_at`,
`contentHead <= head`.

### Timing assumptions

- T4. **No wait between the ring and the final GET `wake-lease`.** The consumption is
  two-phase BY DESIGN (the byte arrives before the consume append is durable —
  serve/wake.rs module doc). The only cushion is the incidental cost of `sidecar.wait()`
  reaping + one `mcp_via` process spawn. Other tests in this file wait for
  `wake_lease_consumed` in a bounded loop (e.g. :376-387); this one asserts immediately.
- T5. `started.elapsed() < 10s` prompt-wake bound — a load bound, not a condition.
- T6. Pipe-visibility loop bound of 10 s covers sidecar spawn + store open + pipe
  create; under full-gate load a cold process + store recovery can stretch.
- T7. `replies[1]` indexing on MCP output — a slow/failed initialize panics on index,
  which reads as a test failure but is not an `assert!`.
- T8. `at_sleep`/`at_wake` on the proxy assume the accept-loop thread has drained the
  kernel backlog of every arming-era connection before `at_sleep` is read.

### Ranked failure hypotheses — which assertion actually fires is UNMEASURED and is the
single most valuable thing W1 can capture (the assertion text names the mechanism):

- **F2-H1. HYPOTHESIS (NOT MEASURED): T4 — the GET lands before the sweep's phase-3
  consume append.** Fails as `"the lease burned on the ring"` (live:true) or
  `lastConsumed` null. Mechanically open on every run; the question is only whether the
  mcp_via spawn gap really outruns a spawn_blocking store-open+append under load.
  **[SUPERSEDED BY MEASUREMENT — see "My prediction was killed" below.]** The rate
  guessed here came from the uncitable seed-9 figure; H measured 12/13, and the mechanism
  named in this hypothesis was afterwards confirmed as the failing assertion.
- **F2-H2. HYPOTHESIS (NOT MEASURED): ring lost or late -> sidecar exits 3 ->
  `"the sidecar must exit 0 on the ring"`,** or exits 0 late -> `"the wake is prompt"`.
  Requires the ring's pipe open/write to fail while the pipe exists, or >10 s
  scheduling stall. Named-pipe semantics make the create-to-connect gap benign (a
  client connect before ConnectNamedPipe queues; the server then gets
  ERROR_PIPE_CONNECTED = success), so a genuinely lost ring needs an error path we did
  not find in reading — if W1's capture shows exit 3, this jumps to primary and the
  ring() error kinds need instrumenting (which io error, not just StaleRendezvous).
- F2-H3. HYPOTHESIS (NOT MEASURED): T8 — a backlogged arming connection counted after
  `at_sleep` -> `"the sleeper placed ZERO requests"` fails. Reading says every arming
  request completed (so was accepted) before the window opens; would need an extra,
  never-served connection from the HTTP client. Decide: on failure, log the delta and
  the accept timestamps.
- F2-H4. HYPOTHESIS (NOT MEASURED): T6 — `"the sidecar never created its rendezvous"`
  panic under load (sidecar store-open slow, or its `read_own_lease` gets a transient
  Storage refusal -> exit 2 -> pipe never appears). If W1 sees this text, instrument
  the sidecar's stderr (the test currently pipes and discards it — capture it).
- F2-H5 (weak). T7 index panic on MCP replies under load.

Note: the poisoned-stream mechanism of flake 1 is NOT a plausible cross-infection here —
this test triggers exactly one due sweep (the arm-sweep is not-due by the fixed-point
cursor), so there is no second racing recorder.

### What the eventual fix's sabotage list must contain (draft, to be finalized after
W1 names the assertion)

- S7. If H1: the fix is a bounded wait-for-`wake_lease_consumed` before the GET (a
  condition, not a sleep). Sabotage: make the wait return immediately -> the guard must
  go red under a deliberately delayed phase 3 (test-only delay hook in the sweep), NOT
  merely "flake returns statistically".
- S8. If H2: instrument `ring()` to record WHICH io error produced `StaleRendezvous`
  (today every failure shape collapses into one reason — wake.rs:57-61 — which is
  honest for the operator but blind for diagnosis). Sabotage: force the error path ->
  the instrument must name it.
- S9. If H4: capture sidecar stderr in the test and assert on the refusal code, so exit
  2 stops masquerading as "pipe never appeared".
- S10. Whatever the fix: it must keep the zero-requests measurement intact — sabotage by
  adding one API call inside the window and observe the proxy counter assertion fall
  (proves the measurement still measures).

---

## Stash assessment: `candidate-fix-flake3-atomic-conditional-append-UNPROVEN`
(stash@{0} on F:\github\GraphHelm, diff read in full; NOT applied)

Two changes bundled:

### Change A — `append_atomic_if` + recorder rewrite (the actual fix)

New store method (local.rs): decision callback (`build(events, next) -> Option<batch>`)
and `append_locked` run under ONE `with_exclusive_lock` acquisition. Recorder passes its
liveness+rendezvous filter as the callback.

**Safety argument: HOLDS, by reading.** All writers go through the exclusive named-file
lock; the decision now reads state under the SAME acquisition that appends, so the
F1-H1 window is gone by construction, not by timing. Only `record_consumptions` writes
`WakeLeaseConsumed` in production (grep: serve/wake.rs:214 is the only site), so one
converted call site covers the invariant. The `next`-embedding idempotency key becomes
sound (next cannot move under the lock). Residual holes: none found for writers using
the store API; a process writing the journal WITHOUT the store API was always outside
the model.

**But its HISTORY claim does not hold.** The comment says the old two-acquisition shape
was "safe only because every store OPEN took an exclusive lock", broken when 53d212d
widened reads. Reading refutes the mechanism: `open` RELEASES its locks before
returning, on parent and on this branch alike, so the open-time lock never spanned the
recorder's validate->append gap. The window exists at the parent too — consistent with
the flake being measured on base. The fix is right; the story about when the bug was
born is wrong, and it matters because it implies reverting 53d212d would restore
safety, which reading says it would not. W1's base-vs-branch rates can settle this
empirically (prediction registered: the flake reproduces on the parent of 53d212d).

**What would prove it:** (1) deterministic red — failpoint pause between validate and
next_sequence on the OLD shape with an injected rival consume; observe double-consume
land; same harness on the NEW shape cannot even express the pause (the window is one
acquisition). (2) The statistical test's rate: W1's base number vs post-fix (expect ~1/3
-> 0 over a large N; "0 in N" needs N stated). (3) Sabotage S1-S5 above.
**What would refute it:** any second production writer of consumptions bypassing
`append_atomic_if` (none today; add a grep-guard or debug_assert), or a journal writer
outside the store API.

### Change B — shared-lock fast path for `open` on a Complete layout (bundled perf)

`initialize_root_locked`: layout-Complete opens take the SHARED named lock (claim:
storm 25s -> 16s). **This is NOT needed for the fix and carries its own hazard:**
`open_inner` runs recovery — `reconcile_orphans` and `publish_active_marker` — which
WRITE. Under Change B those writes can run under a SHARED lock, concurrently with other
opens and with readers whose model is "shared = no writer in the middle". Maybe the
writes are idempotent and rename-atomic; nothing in the stash argues it, and no guard
covers two concurrent recoveries racing an orphan cleanup. Verdict: split it out.
Change A must land alone; Change B needs its own falsifiable guard (concurrent opens on
a store WITH an orphan + active-marker divergence, plus the storm measurement) and its
own review. Bundled, a flake-rate improvement could not even be attributed.

Also worth saying: Change B's comment repeats Change A's wrong history ("was not safe
to do until the wake sweep stopped depending on it" — the sweep never actually depended
on it; it was unsafe with or without).

---

## ADDENDUM (same session, after reading B's #55 memo): the fix design changed

B Agent's memo (`.factory/b-agent-issue55-memo.md`, worktree b-agent-20b716) reached the
SAME window independently — B's "window 3" is this report's F1-H1, derived from #55
archaeology rather than from the test. Two blind readings agreeing is the strongest
evidence either of us has; both also independently found that the "one lock spans
validate and write" claim was never true, in EITHER lock regime.

B proposes a smaller fix than the stash: pin the sequence from G1's own history read, so
the liveness decision and the sequence pin come from one read and a rival append after it
always trips the existing CAS (local.rs:684). Verified before endorsing:
`next_sequence == max(history.sequence) + 1` exactly — local.rs:1372-1377 sets next as
expected + batch length, local.rs:1106-1111 refuses any batch whose first sequence is not
the expected one, so per-stream sequences are contiguous and the two are the same number
(empty stream: 0+1 == unwrap_or(1)). Same stream_key on both sides.

**This report now favors B's design over stash change A**, on three grounds, the first
decisive:

1. **The seam must survive the fix.** Under stash A the window lives INSIDE the exclusive
   acquisition, so no rival append can be injected there — it blocks on our own lock and
   lands after our append, which models nothing real. The deterministic red test cannot be
   written in an honest shape. Under B's design the seam ("after the decision read, before
   the append") persists post-fix, and sabotage (restore a second `next_sequence` read)
   turns it red. Section F1's S1 is rewritten accordingly: sabotage the PIN, not the
   acquisition count.
2. Surface: stash A adds a public store API (`ConditionalBatch`, closure semantics) for one
   call site — `record_consumptions` is the only production writer of `WakeLeaseConsumed`.
3. Stash A review finding, recorded whether or not it is used: its `std::cell::Cell`
   out-param is dead weight — `append_atomic_if` already returns
   `append_locked(...).map(|events| events.len())`, which is the same number.

Known cost of B's design, stated so it can be attacked: the CAS is not consume-specific,
so ANY unrelated append in the window drops a legitimate consumption. Judged benign and
self-healing (lease stays live, the byte already crossed, the next content append
re-sweeps), and spurious wakes are safe by design (serve/wake.rs:8-10). If that judgement
is wrong, stash A comes back.

Invariant put to B for approval before any code is written, deliberately NOT phrased as
"one lock" — that phrase is what lied for two milestones:

> At most one `wake_lease_consumed` enters the stream per arming, and the decision that a
> lease is still live is made against the SAME read that pins the sequence the consumption
> appends at. Any append by anyone between that read and ours makes ours fail
> (`SequenceConflict`) and drop silently, never blind-retry.

Open item also put to B: `WakeLeaseConsumed` carries no rendezvous id, so the fold cannot
distinguish burning the intended lease from burning a same-session re-armed replacement.
The recorder's rendezvous-equality filter (wake.rs:180-185) is the only defense and no test
pins it — sabotage it and every existing test stays green (this report's S3).

## READY-TO-TYPE SPEC for the flake-3 red test (C holds this pen; nothing typed yet)

Written while blocked on two gates (B's APPROVED/amend on the invariant, W1's base
numbers). Specified to the line so the code is minutes once cleared. Verified against the
tree, not assumed.

### The seam, and why it is placed there

A test-only hook fired ONCE between the liveness decision and the sequence pin. Production
keeps the same code path — a no-op closure, not a `#[cfg(test)]` copy, because a test that
exercises a duplicate of the path proves nothing about the path:

    pub(crate) fn record_consumptions(events, execution, consumptions) -> usize {
        record_consumptions_inner(events, execution, consumptions, &|| {})
    }
    fn record_consumptions_inner(..., after_validation: &dyn Fn()) -> usize

At the seam no lock is held (`with_lock` releases per call, local.rs:540-587; `open`
releases before returning, local.rs:390-397), so the hook's rival append — made through a
SECOND store handle — takes its locks and lands. That is the window, executed rather than
argued.

### The test (in-module, next to the two existing #55 guards at wake.rs:251/338)

`a_rival_consume_between_validation_and_the_sequence_pin_appends_nothing`

Fixture: reuse the existing red's shape exactly (scope exec-race style, `WakeLease` armed
at sequence 1, no execution-start event — the existing test proves `replay` succeeds on
that shape). Captured due-list = that lease.

Hook body: open a second handle, append `WakeLeaseConsumed` for the SAME session at its own
`next_sequence` (2). Legitimate at that instant — the lease is live — so it models a rival
recorder that won, not a corrupt write by the test.

Assertions, both required:
- `recorded == 0`
- `graphhelm_events::replay(...)` on the stream is `Ok` (the oracle the live incident failed)

### The red this must produce BEFORE any fix code (predicted, to be OBSERVED)

Current order: list_streams -> read_replay_stream (G1, lease live) -> filter passes ->
[SEAM: rival consumes at 2] -> `next_sequence` reads 3 (fresh, post-rival) -> append at 3
SUCCEEDS. Two consumptions, one arming. Predicted failure: `recorded == 1` AND replay
returns the fold's corrupt error — both assertions fail, unambiguously. If the red does not
appear, F1-H1/window-3 is wrong and the whole design stops there.

### The fix, in the shape the red demands

Pin `next` from G1's own history instead of a second store read:

    let next = history.iter().map(|e| e.sequence).max().map_or(1, |s| s + 1);

`max`, not `last` — ordering of `read_replay_stream` is journal order and the pin must not
depend on that being sequence order. Equivalence to `store.next_sequence` verified:
local.rs:1372-1377 sets next as expected + batch length and local.rs:1106-1111 refuses any
batch whose first event sequence is not the expected one, so per-stream sequences are
contiguous; empty stream gives 1 on both sides.

Post-fix the same interleave trips the existing CAS (local.rs:684): expected 2, actual 3 ->
`SequenceConflict` -> `append_atomic` errs -> `return 0`, stream clean.

### Sabotage list, rewritten for this design (each observed to fail INDIVIDUALLY)

- S1'. Restore the pin as a fresh `store.next_sequence(...)` call -> the new red test falls.
  (This replaces the old S1: sabotage the PIN, not the acquisition count.)
- S2. Record unconditionally, dropping the still-live filter -> existing
  `a_rival_consume_between_read_and_record_appends_nothing` falls.
- S3. Match on session only, drop the rendezvous-equality compare -> NOTHING currently
  falls. This is the unpinned only-defense; a new guard is needed (stale capture rdv-A vs
  re-armed lease rdv-B, same session, must record nothing). In this PR or a named seed —
  B's call as reviewer; if seed, the seed must name this sabotage as the one that stays
  green today.
- S4. Fixed idempotency key without the sequence -> a guard that two legitimate consumptions
  in different rounds both land must fall.
- S5. Fold tolerates consume-without-lease -> the oracle assertion must fall, proving the
  oracle measures the poison and not a coincidence.

### AGREED INVARIANT (B: APPROVED with amendment; this wording is now pinned)

> At most one `wake_lease_consumed` enters the stream per arming, and the decision that a
> lease is still live is made against the SAME read that pins the sequence the consumption
> appends at. Any append by anyone between that read and ours makes ours fail
> (`SequenceConflict`) and drop silently, never blind-retry. A lease already consumed
> before that read is dropped by the liveness+rendezvous filter and never reaches the
> append at all.

B's amendment is the second sentence, and its reason is worth keeping: the CAS covers the
after-read window, the filter covers the before-read window, and an invariant naming only
the CAS invites a later reader to delete the filter as redundant. TWO blades, named.

### Review gates B will hold this fix to (agreed)

- G1. Deterministic red on the CURRENT two-read shape, observed before any fix code. Must
  reproduce at BOTH the efd85d0 and 53d212d shapes; if it passes on either, both analyses
  die and the premise is re-derived rather than patched.
- G2'. Post-fix the SAME seam stays expressible and GREEN: rival appends at the seam, our
  append trips the CAS, nothing recorded, stream replayable. Sabotage (reintroduce a second
  `next_sequence` read) turns it red. The guard survives the fix — that is the point, and it
  is why this design beat stash A.
- G3. The three existing #55 guards stay green.
- G4/G5. S1'-S5 each observed red individually; the rendezvous-swap guard is NEW and lands
  in this PR (B's reviewer ruling, below).
- G6. Stash change B split out, not shipped here.
- G7. Commit states the window predates 53d212d.
- G8. Single-writer check: `record_consumptions` is the only production writer of
  `WakeLeaseConsumed` (verified by grep today; the guard makes it durable).
- G9. The pin derivation carries a comment citing local.rs:1372-1377 and :1106-1111 —
  contiguity is load-bearing, and that line is where it breaks if batching ever changes
  sequence assignment. Optional `debug_assert` against `next_sequence()` in test builds
  only; NO second read on the production path.

### Rendezvous guard: IN this PR (B's ruling)

Reason accepted: the fix edits the region the filter lives on, and shipping an edit to
unpinned behavior while seeding its pin for later means reviewing a filter change with no
test that could catch its deletion. New in-module guard: stale capture rdv-A vs re-armed
lease rdv-B, same session -> records NOTHING; sabotage = match on session only -> red.
The protocol gap itself (`WakeLeaseConsumed` carries no rendezvous id, so the FOLD cannot
see the distinction) stays out — that is a schema change, seeded separately.

### The drop-is-benign argument, with one precision B's version overstates

B retracted the starvation objection on this chain: every append that trips our CAS is
itself a mutation, every successful mutation spawns its own sweep (serve/mod.rs:806, in the
`Ok` arm only), so the tripping event IS the retry vehicle; the chain terminates at
quiescence when the last append's sweep finds no rival.

Precision, verified: the retry vehicle exists only for SERVE-MEDIATED mutations. A direct
store append (a CLI command, or the recorder's own consume append) spawns no sweep. So the
honest claim is "self-healing under serve traffic", not "self-healing under any append".
This is not a new hole — with pure CLI usage nothing rings at all, since the ring is
serve-only — but the fix note must say the narrower true thing. Nothing is lost or lied
about in the delayed case either: the byte already crossed, and a lease reading `live` with
no receipt yet is honest rather than calm-laundering.

### Landing conditions carried from the orchestrator

The commit must correct the history claim explicitly: the window predates 53d212d, and
reverting that commit restores nothing. Stash change B stays parked with its
recovery-writes-under-a-shared-lock hazard on record.

## TYPED (orchestrator cleared "TYPE IT"): red test in the tree, UNBUILT and UNRUN

`apps/cli/src/commands/serve/wake.rs`, +144 lines, nothing else touched. No fix code.

- `record_consumptions` becomes a thin wrapper passing a no-op closure to a new private
  `record_consumptions_inner(..., after_validation: &dyn Fn())`. The seam is called once,
  after the still-live filter and its early return, immediately before the `next_sequence`
  pin. Production runs the same path a test runs — no `cfg(test)` copy.
- New in-module guard `a_rival_consume_between_validation_and_the_sequence_pin_appends_nothing`:
  arms a lease at #1, captures the due-list, and the seam closure opens its own handle and
  appends a rival `WakeLeaseConsumed`. Asserts `recorded == 0` and that the stream still
  replays.

One assumption was checked instead of inherited. The sibling guard drops its store handle
"so the recorder can take its own lock", which reads as a requirement — if it were one, the
rival's mid-call open would hang and the test would produce a timeout rather than a red.
`core/events/tests/read_concurrency.rs:128` settles it: eight threads each open their own
handle concurrently and pass. A live handle holds no lock between operations, so the drop is
defensive, not required. Cited in the test.

PREDICTED RED (to be observed once cargo is cleared): validation passes, the rival burns the
lease at #2, `next_sequence` then returns #3 — genuinely current, so the CAS has nothing to
refuse — and the append lands. `recorded == 1`, the first assertion fails; the replay oracle
would fail too. If it comes back GREEN, window 3 does not exist and the fix has no premise.

KNOWN CONTRADICTION LEFT IN PLACE ON PURPOSE: the doc comment at wake.rs:146-149 still
claims the handle's exclusive lock makes re-validation and append atomic, while the new doc
at :165-168 says no lock is held there. Correcting it now would assert the red test's
conclusion before measuring it. The fix commit corrects those lines together with the
history note — flagged to the reviewer rather than quietly fixed or quietly left.

## SEED (found by B, verified here): the rendezvous-EQUAL burn — a silent missed wake

Not in this PR. Pre-existing at efd85d0, unrelated to window 3, and it SURVIVES the window-3
fix. Registered here because it is sharper than the defect this PR removes.

Sequence: a lease is armed on rendezvous X; a sweep captures it as due and rings it; the
sleeper wakes and RE-ARMS the same session on the SAME rendezvous X — which is not a
contrived case but this factory's own production convention, where rendezvous ids are fixed
per agent; then the first sweep's delayed phase 3 replays, finds a live lease for that
session whose rendezvous is X, and burns it.

Verified against the code: the filter (wake.rs:180-185) compares session and rendezvous and
nothing else, and `DueLease` (wake.rs:42-46) carries no arming identity at all. Every
comparison the recorder can make passes. The fold cannot catch it either — a consumption of
a LIVE lease is legal, so no `Corrupt`, no refused replay, nothing loud.

Why it outranks window 3 in severity even though window 3 is the one being fixed: window 3
corrupts the stream, and a refused replay is unmissable. This one leaves a sleeper blocked on
a rendezvous that will never ring again, because the lease that would have rung it is gone.
It surfaces as `wake-wait` exiting 3 at the declared horizon — which the sleeper reads as
"my deadline passed, nothing happened" — while the store's own receipt says `rung`. Two
surfaces answering one question differently, and the one the operator acts on reports calm.
That is absence laundered into calm, which is the rule this milestone exists to enforce.

CORRECTION to B's proposed shape, and the reason it is worth stating: B wrote that the fix
needs the protocol change (a consumption event that carries arming identity), which would
make this expensive and easy to park. It does not. `DueLease` is a `pub(crate)` struct
inside `apps/cli`, and phase 1 already holds the history it would need — the sequence of the
`WakeLease` event that produced the live lease is a scan away, and phase 3 replays that same
history anyway. Carrying it in `DueLease` and comparing it in the filter is an internal
change with no schema, no wire format, and no event touched. Note also that comparing the
cursor instead would NOT work: re-arming at the reported `contentHead` is a documented fixed
point, so a re-armed lease can carry an identical cursor.

Excluded from this PR for scope and TDD reasons — it is a distinct defect needing its own
observed red — not because it is expensive.

### Two defenses, and only one of them is this seed

B and I separated these after the correction above, because bundling them is what made the
whole thing look like a schema change:

1. **Recorder-side (this seed, and the actual fix).** The filter compares an arming identity,
   so a capture from before the wake cannot match the lease armed after it. Internal only:
   `DueLease` is `pub(crate)` in `apps/cli`, no event, no wire format, no schema.
2. **Fold-side (stays a separate, lower-priority seed).** For a replay to DETECT the mistake,
   the consumption event would have to carry arming identity — that one is a real schema
   change. It is defence-in-depth against a writer that bypasses the recorder, and nothing
   today does; the recorder-side fix closes the actual missed-wake hazard on its own.

### Implementation shapes for whoever takes it (verified, not assumed)

`WakeLeaseState` (projection.rs:239-257) carries `cursor`, `rendezvous_id` and `matures_at`
— confirmed by reading the struct. It does NOT hold the arming's sequence. So both sides of
the comparison must be derived by scanning history for the `WakeLease` event that produced
the live lease: phase 1 for the captured one, phase 3 for the current one. Both phases
already hold the full history and already replay it, so this is one pass each and no extra
store read.

The alternative shape, if someone would rather pay once: give `WakeLeaseState` an
`armed_at_sequence` field. The filter then becomes O(1), and the glance surfaces could name
"armed at #N" for free. Both shapes are legitimate; the implementer should measure rather
than pick from taste.

DEAD END, named so nobody walks it twice: comparing the CURSOR does not discriminate.
Re-arming at the `contentHead` the surface reported is a documented fixed point (guard:
`arming_reports_the_head_the_doorbell_compares_and_re_arming_with_it_is_a_fixed_point`), so
a re-armed lease can legitimately carry an identical cursor. The arming sequence is strictly
monotone per stream; the cursor is not.

### Its red is TYPED, and parked outside the crate on purpose

`.factory/c-agent-rdv-equal-red-draft.rs` —
`a_capture_from_before_the_wake_never_burns_the_lease_armed_after_it`. PR 2 moves it verbatim
into wake.rs's test module.

It is NOT in wake.rs today because it is defect-driven and therefore RED by construction,
and the window-3 PR must pass `cargo test --workspace` to land. A knowingly-red test in that
crate would fail PR 1's gate on a defect PR 1 does not claim to fix. The usual escape,
`#[ignore]`, is worse than parking it: an ignored test measures nothing while looking like
coverage. Unbuilt either way, so nothing is lost by the wait.

## SEED (found by L, verified here): every ring failure is classed "the sleeper died"

Parked, no pen action. Same family as the oracle finding — a legality-blind silent failure.

CODE FACT, verified at wake.rs:55-61: `ring()` matches `Err(_)` on both the open and the
write and maps every one to `StaleRendezvous`. There is exactly one bucket for "the pipe is
not there", "the pipe is busy", and every other io error. The doc at :48-50 states this as
intent ("anything else is equally a dead rendezvous"), so it is a deliberate simplification
rather than an oversight — but nothing today can distinguish a sleeper that is GONE from one
that is ALIVE and momentarily unavailable.

CONSEQUENCE if the second case is reachable: the lease is consumed and RECORDED consumed
with reason `stale_rendezvous`, so a live sleeper's doorbell is disconnected and the receipt
blames the sleeper for dying. No later append rings it, because the lease is gone. It waits
out its horizon and `wake-wait` reports the deadline passed — and that path never consults
the receipt (wake_wait.rs:85-101). Silently lost doorbell rather than a missing one, and the
same shape as the rdv-equal burn: legal log, calm surface, sleeper never woken.

UNMEASURED, and it is the whole question: whether `ERROR_PIPE_BUSY` is actually reachable
against our sidecar. The sidecar creates its pipe with `first_pipe_instance(true)` and
`max_instances(1)` and is single-shot, and a client that opens before the server calls
`connect()` is queued by Windows rather than refused — so the obvious busy window may not
exist. Reaching it plausibly needs a second concurrent ringer on the same rendezvous. The
CLASSIFICATION collapse is a fact; the hazard's reachability is a hypothesis and must be
labelled as one until someone measures it.

This raises the priority of something already in this report as S8: instrument `ring()` to
record WHICH io error produced `StaleRendezvous`. That was registered as a diagnostic gap for
flake 2; L's finding makes it a correctness question too. One change, two independent
motivations, and it is cheap.

## FINDING: a gap in test NAMES is not a gap in coverage

Cost me a test I had just written, and it is the most transferable thing I produced.

N read the events crate and found that nothing pinned one axis of idempotency divergence —
same key, same events, only the expected sequence moved. The neighbouring guard varies event
CONTENT at a fixed sequence, so the other axis appeared unguarded. I verified that reading and
wrote the missing test. It passed.

Then I sabotaged for it, because a guard nobody can break measures nothing:

| sabotage | result |
|---|---|
| replay arm drops the digest comparison | 2 failed — mine, and `exact_retry_is_resolved_before_sequence_and_divergent_reuse_fails_closed` |
| `request_digest` blind to `expected_next_sequence` | 7 failed — mine and six others, including `concurrent_writers_serialize_and_only_one_claims_the_expected_sequence` and `identical_idempotency_key_is_independent_across_streams` |

**No sabotage fells my test alone.** Every way of removing the property takes existing guards
down with it. So the invariant was never unguarded — it is held by tests whose names say
nothing about it, and the "gap" was in the naming.

The rule, which M sealed and which is worth more than the test would have been: **a gap in
test names is not a gap in coverage, and only a sabotage list tells them apart.** Reading the
suite tells you what is *named*. Breaking the property tells you what is *protected*. Anyone
scoping work off "nothing tests X" is reading names unless they have sabotaged for it.

The mechanism question survived — it feeds the S4 explanation, so the seed keeps its consumer
— but the motivation "add a test to close a coverage gap" is dead, and I booked that against
my own twelve lines rather than letting them stand as having plugged something.

One limit of the corroboration, M's, and it is a real one: my test and N's are independent of
**test-writing error**, since two authors wrote two files. They are NOT independent of
**premise error** — both exercise the same path and both authors read the same source. Agreeing
instruments cover the first and can never cover the second.

## FROZEN PREDICTIONS (registered with M's ledger before any cargo run)

Kept here too so they survive independently of message history. Nothing below has been
executed.

| # | Test | Predicted | First assertion to fire | Kills the claim |
|---|---|---|---|---|
| P1 | `a_rival_consume_between_validation_and_the_sequence_pin_appends_nothing` | FAILS: `recorded == 1`; replay returns the fold's corrupt error | `assert_eq!(recorded, 0)`, left 1 right 0 | GREEN — window 3 does not exist, my F1-H1 and B's window 3 die together, premise re-derived from a journal |
| P2 | `a_stale_capture_never_burns_the_lease_that_replaced_it` | PASSES today: `recorded == 0`, session-p still live under rdv-new | — | Red today (filter does not work as read), or green under the session-only sabotage (guard measures nothing) |
| P3 | `a_capture_from_before_the_wake_never_burns_the_lease_armed_after_it` | FAILS: `recorded == 1`, session-p gone — **and replay still SUCCEEDS** | `assert_eq!(recorded, 0)`, left 1 right 0 | GREEN — the seed dies rather than being patched |

P3's replay-succeeds clause is the detail that separates it from P1: burning a live lease is
legal, so nothing refuses. That is the whole reason one failure is loud and the other silent.

THIRD OUTCOME for P1, pre-registered so it cannot be mistaken for flakiness: a HANG. That
would mean a live handle does hold a lock between operations, refuting both readings. It is
REPORTED as a finding, never patched with a timeout or a sleep.

Two older predictions, restated so they freeze in one place: mine, that the
`concurrent_sweeps` flake reproduces at the PARENT of 53d212d; and B's, independently
derived, that P1 fails at BOTH the efd85d0 and 53d212d shapes.

**Status of the parent prediction: FOOTNOTE, no consumer. Retired by me, not overruled.**

It was registered to test whether the widening reopened #55. That question is settled by
reading — locks are taken and released per call, verified independently at both commits — and
no rate can touch a structural fact. The fix landed and closed the window regardless. So
nothing downstream waits on the answer.

Worse, the belt cannot answer it: it reproduced the race 0 times in 13 at the tip, so a parent
run compares 0/N against 0/N. B predicted that in advance, and the scheduled run is cancelled
as uninformative.

> **Run-verification of the 0/13** (added after the M09 gate red, where a harness that never
> started a test reported three clean iterations). A zero is the one result a completely dead
> instrument reproduces perfectly — every other number is at least proof that something happened —
> so a 0/N owes its reader the evidence that it ran at all.
>
> This one has that evidence, but the evidence lives elsewhere in this document and was never
> written next to the number: the same belt was separately observed **passing** with the sweep's
> recorder call deleted (S0, row "delete the recorder call, belt alone" — green, 37.9s). A harness
> that never executes cannot produce a pass. So the belt demonstrably ran, and the 0/13 is a real
> zero rather than an absence of measurement.
>
> Note this control also carries the 0/13's actual meaning, which is the more important half: the
> belt runs and reproduces nothing **even when the code under test is gutted**. The zero was never
> weak evidence about the parent — it was evidence that the belt does not measure this defect at
> all. That is what moved the lane to the deterministic seam.

By my own rule — a claim nobody consumes is a claim not worth a measurement — this drops to a
footnote, and I am saying so rather than keeping a prediction of mine alive because it is mine.

The one thing that WOULD have a consumer, named so the footnote is honest: if anyone ever
doubts the structural reading itself, the instrument is the deterministic seam test run at the
parent, not the belt. It would convert a twice-verified reading into a measurement. Nobody
doubts it today — B accepted that half transitively after verifying the mechanism at both
commits — so it stays unrun, and that is a decision rather than an omission.

## PR 2 (#74): what the second lane produced beyond its fix

Merged locally as `d10916b`. The fix itself is in the commit; these are the findings that
outlived it, each of which cost a wrong prediction or a cut guard.

### A fixture whose input comes from the thing it checks cannot detect that thing breaking

My first guards hand-wrote the discriminator that production copies off the projection. Under a
sabotage corrupting the fold, the live lease moved and the hand-written capture did not — so they
disagreed, stale captures were dropped, and **two guards passed for a reason production would
never reproduce.** I predicted the wrong casualties and was right about the numbers only by
accident.

The rule M drew out of it is the durable half, and it cuts both ways: hardcoded fixtures are
blind to production drift; projection-built fixtures are blind to fold corruption. **Different
blindness, not less.** So "rebuild every fixture to match production" would have been a
regression sold as a fix — the accidental fold-detection the hardcoding provided had to be
replaced by a guard aimed at the property, not simply deleted.

### The vacuous red

The mirror of the vacuous green, and worse, because **nobody audits a red.** "It went red under
sabotage, so the guard measures something" was our proof-of-meaning inference all week. It is
false when the guard fails at an earlier line than its own assertion.

Measured: removing `skip_serializing_if` felled four tests, every one at an `append`, none at any
assertion. The guard the sabotage was aimed at never executed.

Consequence adopted as a standing rule: a sabotage receipt is **red at the guard's own assertion,
named by panic site** — not merely red. Panic-site reporting stopped being courtesy and became
the evidence.

### Redundant today is not redundant permanently

I argued for cutting my own wire-absence assertion: no single sabotage can fell it, and an
unfellable check is decoration. The reviewer ruled the same way. M's counter reversed both of us.

Decoration is a check that can **never** fail. That one cannot fail **today**, for a reason
exactly one edit deep — a schema type. The day it is relaxed the assertion becomes a
single-sabotage blade with nothing behind it, and a guard whose redundancy is documented survives
the change that makes it necessary again. A removed one does not.

The correct move was neither keeping it silently nor cutting it: it was keeping it with the
measurement that proves it dormant, and the note that its own sabotage needs two edits and **has
not been run**.

### A redundant blade blinds the sabotage of the blade that matters

Measured with the variable isolated — same fixture, same sabotage, one thing changed at a time —
and it is **partial**, which is narrower than I first argued. The extra blade blinds the guard
where it still discriminates, and cannot blind the guard where it does not — which is precisely
the case it fails to catch.

I overstated by generalising from the case the argument was built on, and the measured version
had to be walked back before it shipped.

## MY PREDICTION WAS KILLED

I registered, before any run: my fix alone lowers the sleeper flake's rate but **does not
reach zero**, and zero in N>=10 kills my reading. H measured `aac0d67` — my fix, without A's —
at **0 failures in 10 isolated runs**, against a 9/10 isolated base. The kill condition fired
on the terms I set.

So the sentence dies: **"the race is structural, not contention"** was wrong. The lock
acquisition my fix removed from phase 3 was not an amplifier of the failure — on this
evidence it was its active cause.

What survives, stated carefully so it is not read as rescuing the claim: the *window* is still
there by reading. The test performs its `wake-lease` GET with no wait after the ring, and the
consume is two-phase by design, so nothing in the code orders those two events. That is a
reading, and it now has a measurement standing against its practical consequence.

**The over-reading to refuse: this does not make A's fix unnecessary.** Two reasons, neither
of which is my prediction in disguise. First, 0/10 bounds the rate; it does not establish
zero. Second, and more important, A's condition-wait removes the test's dependence on timing
altogether, where my change only removed one contended lock acquisition from one side of the
race. A guard that passes because the machine happens to be fast is the thing this whole
milestone exists to refuse. If anyone cites this 0/10 as grounds to drop A's fix, that
citation is exactly the mistake N warned about with the belt's clean sheet.

### The mechanism nuance, marked rather than claimed

Recorded because the orchestrator raised it after M's scoring, and recorded WITH its status
because it runs in my favour and therefore needs the tighter label.

**Code fact, verified:** the pin fix did not remove the two-phase race. What it removed from
phase 3 is one `next_sequence` call — and that is not merely a lock round-trip, it is a full
`load_state`, which reads the journal from disk under a shared lock, with `validate_anchors`
either side. So the fix NARROWED the window the receipt GET was outrunning. It did not order
the two events; nothing in the code does.

(Precision on the wording it arrived in: this is one lock acquisition carrying a state load,
not "one store open". The store is opened once at the top of the recorder and the open was
never in that window.)

**Not established, and it must not be read as rescue:** that "structural, not contention"
could still be true at a rate below what N=10 can resolve. Zero in ten excludes rates above
roughly 26%; a real 5% rate would sit inside that blind spot comfortably. That possibility is
UNRESOLVED — not supported, not refuted.

I raised the rule-of-three bound myself, in the section below, as a fault in my own kill bar.
It cannot now be turned around and spent as evidence for the claim it was offered against. The
sealed kill stands on the terms I wrote; a bound that was a confession five minutes ago does
not become a defence because someone else says it back to me.

**The instrument that would settle it is deterministic, not more runs:** A's delay hook
(#72), which forces the ordering rather than sampling it. That is the same preference this
whole report has been arguing for — a seam that makes the window executable beats any number
of stochastic samples, which is exactly how window 3 was settled while 0/13 said nothing.

### The bar I set was weaker than I made it sound

Registering this as a fault in my own prediction design, not as an escape — the kill stands
either way, and relitigating a threshold after seeing the result is the move I have spent all
day objecting to when others might have made it.

"Zero in N>=10" sounds like it establishes zero. It does not: 0 in 10 leaves a rule-of-three
upper bound near 30%. I chose a bar whose name overstated what it could show, and I chose it
for a claim of my own. The honest bar would have named the rate it could exclude — "under 5%
requires roughly 60 clean runs" — or would have been framed as a bound rather than a zero.

I got this right for other people's predictions today and wrong for mine, which is the whole
reason the ledger holds the wording rather than the author.

M named the general form, and it is better than mine: **an observational trigger for an
inferential conclusion** — legitimate, falsifiable, fires honestly, and licenses less than its
own sentence claims. Mine was the instance; that is the class. If the claim is about a RATE,
the trigger has to be about a rate too, with an N that could see it.

Two further things belong here, both of which cut against me:

**M ruled the narrowing; I did not argue for it.** When the same statistic was offered back to
me as a partial rescue I refused it, because I had introduced it as a fault in my own bar. That
it now stands as the record is the ledger-holder's doing, and the distinction matters more than
the narrowing does.

**And my own prediction had no provenance at all.** M asked me to confirm C-LC's wording
because it had reached the ledger as someone else's quotation of me. The reason is mine: I
never put it in an artifact. It lived in two chat messages, in two languages, with slightly
different elaborations, and it did not enter this document until after it died.

So the rule I proposed for other people's documents — cite your numbers, mark your unmeasured
claims — does not cover a *prediction* that never reaches a document at all. I spent the same
afternoon calling seed 9's figure uncitable for travelling without provenance, while my own
sealed claim travelled through chat with none. That gap in the rule was found by standing on
the wrong end of it.

M sealed it as a rule and added the observation that makes it more than a confession: this was
the **one** prediction that bypassed the ledger, and it is the **one** whose wording had to be
reconstructed from a third party's quotation. Every row registered directly is verifiable
against its author. The ledger's value was demonstrated by the single row that skipped it,
which is stronger evidence for the mechanism than any row that used it.

The operative form: a prediction sent to a peer "so you can hold me to it" is a **promise, not
a record**. The peer's copy, the author's memory and the eventual quotation can all drift, and
nobody notices while the prediction is still alive.

### The grep-as-gate sweep, and its result

Ordered after the kill: find everything derived from the dead sentence. Searched all four of
my durable artifacts — this study, the parked PR-2 red, the issue draft, and the landed commit
body — plus the published issue.

**It had not propagated.** The claim appears in none of them. Issue #71 contains no reference
to it; the commit body's subject is window 3, whose evidence is deterministic red, green and
five sabotages, all independent of contention and untouched by this result. The only artifact
correction needed was one hypothesis paragraph that had quoted the uncitable seed-9 rate, now
marked superseded.

The claim lived where it should have: in M's ledger as a registered prediction, and in
messages. That is the difference between a hypothesis kept in the place built to score it and
a number let loose into documents — which is the same lesson as seed 9, arriving from the
other direction, and this time it worked.

## LANDED AND GREEN

`aac0d67` is the tip of `issue-m09-arming-the-alarm`. The full house gate reports
`[gate] GREEN - every stage passed` — 22 stages, 260 test-result lines, no failures, no
panics, exit 0, and the log scanned for failure markers rather than the exit code trusted.

The PostgreSQL matrices ran for real (throwaway cluster, 43 Postgres tests, both the ignored
matrix and the non-C collation one), so this is a **full** gate and not the partial kind that
has to be labelled as such.

Two things the gate showed that are NOT evidence, recorded here so nobody later mistakes them
for it: `wake_http` passed, and `constructor_bounds_reconciliation_catalog_locks` passed. Both
are registered flakes. Against H's base table (12 failures in 13 — 9/10 isolated, 3/3 in-suite,
in `.factory/h-agent-base-measurements.md`) a single `wake_http` pass is a roughly 1-in-13
event, consistent with my prediction that the fix lowers the rate AND equally consistent with
luck. One run cannot separate those, so it scores nothing. The N>=10 run does.

### A stale number in a committed doc, found by being corrected

I told N the sleeper guard failed "roughly 3 in 4". N checked against H's measurements: 12/13.
My figure was not an estimate of mine — it came from `docs/milestones/m09-seeds.md`, seed 9,
and I passed it on without ever measuring it.

The same seed also states the guard "fails at the parent commit too" as settled fact. That is
exactly the claim registered in the ledger as an unmeasured prediction with a binding N>=10.
So the artifact the next planner reads first contains an uncitable number and an untested claim
stated as fact — and the reason I repeated one of them is that inheriting a number from a
document feels nothing like guessing.

"Uncitable" rather than "wrong", which is N's correction and the better claim: the doc never
names its base, rates can differ across commits, and I had no measurement of my own to set
against it. H's figure carries invocation, N, base and a raw per-run table; seed 9's carries
none of the three. Where they disagree the one with provenance wins. That version survives
someone turning up with a run at a different base; "the doc is wrong" would not.

### The rule this produced, and the four lines that argue for it

N found the part that settles it. Seed 8 closes at m09-seeds.md:92-93 with **"A finding we act
on because it is right must not also teach us to trust numbers nobody checked."** Seed 9 begins
at :96 and states an unchecked number at :99-100. Four lines apart, same document, same author.
Verified by reading it.

That is not carelessness worth a swipe. Whoever wrote line 92 believed it — they wrote it — and
then did the thing four lines later. It is proof that the rule had a sentence and needed a
MECHANISM. The board's evidence rule (invocation, N, base, raw list) is that mechanism for
what agents send each other; nothing applied it to committed docs, and the cost of that gap is
measurable here: the number travelled doc → my study → my message → N's audit, and only a
per-run table stopped it.

The mechanism proposed, deliberately cheap so that it is not dropped — an unreadable rule gets
abandoned, which is worse than none, because people go on believing there is one:

> A number in a durable document must **cite** its source, even loosely ("H's table", "run 7").
> A claim that has not been measured must be **marked** unmeasured.
>
> A doc may say "roughly three in four". It may not say it without saying whose.

Two clauses because seed 9 fails in two distinct ways: the rate is unprovenanced, and the
parent claim is unmeasured-but-stated. One clause would have caught only half of it.

Applied to this file rather than proposed and exempted: every number here now cites its source
(H's table by path, my own runs by invocation and N, the #56 commit body for the belt's own
admission), and every unverified claim carries its marker. Two citations were missing when I
first wrote this section and were added when I scanned for them — which is the point of having
a mechanism instead of an intention.

Applied to my own landed commit too, and left alone deliberately: its "0 times in 13" carries
an N and the isolated/in-suite split, so it is checkable against H's table, but it does not name
whose measurement it was. That is the weaker half of the same disease in a gated commit. Not
worth amending a landed, verified commit for; worth naming rather than exempting, and the
milestone record should carry the attribution when it is written.

## OUTCOME: everything above was run. What the predictions actually scored.

Commit 185e457 on `c-study-arming-the-alarm`, reviewed and [APPROVED] by B with one condition
still outstanding (S4, below). Nothing pushed, no PR — the milestone branch lands as a whole.

| run | result |
|---|---|
| P1 red, before any fix | FAILED, `left: 1 right: 0` — the exact predicted signature, no hang |
| P2 pin-guard, before fix | passed, as predicted for a behaviour-pinning guard |
| green, after the fix | 4/4 wake guards, 24/24 CLI unit suite, clippy clean |
| #55 belt, run alone | green (33.8s) |
| S1' restore the second sequence read | only the new pin guard falls |
| S3 drop the rendezvous compare | only the new rendezvous guard falls |
| S6 recorder bails early, unrelated reason | see below — the important one |
| S0 delete the recorder call, belt alone | **green** (37.9s) |

P1 CONFIRMED: window 3 is real, and the hang outcome that would have refuted the lock reading
did not occur.

### The two results that outgrew the fix

**S6.** With the recorder returning immediately for an unrelated reason:

    a_live_lease_consumption_still_records ....................... FAILED
    a_rival_consume_between_read_and_record_appends_nothing ...... ok
    a_rival_consume_between_validation_and_the_sequence_pin ...... FAILED (missing receipt)
    a_stale_capture_never_burns_the_lease_that_replaced_it ....... ok

The pre-existing #55 deterministic red passes with the recorder **completely dead**. N
predicted this from counting the recorder's twelve `return 0` sites; it is now observed rather
than argued. My own guard fails only because it carries N's last-word assertion — without it,
it would have passed too, and would have retired silently the first time the recorder broke
elsewhere.

**S0.** The 15-round race test passes with the sweep's recorder call **deleted entirely**.
N's registered prediction, confirmed.

Together: #55's proof chain is a race test that never once reproduced the race — its own fix
commit (0df65da, #56) records that in its body — plus a
deterministic guard that survives the recorder's death. The fix was real; nothing in the tree
was measuring that it stayed real. That is the finding below, with numbers instead of an
argument, and it re-prices greens banked in earlier milestones.

### How every run above was made (provenance, per N's evidence rule)

Machine: this worktree, `c-study-arming-the-alarm`, base 53d212d. **N = 1 per condition.**

    # the four wake guards (red, green, S1', S3, S6)
    cargo test -p graphhelm-cli --bins serve::wake::tests -- --test-threads=1
    # the full CLI unit suite
    cargo test -p graphhelm-cli --bins --locked
    # the belt, alone (G3 and S0)
    cargo test -p graphhelm-cli --test wake_http --locked \
        concurrent_sweeps_never_double_consume_a_lease
    # the lint gate
    cargo clippy --workspace --all-targets --locked -- -D warnings

`--lib` does not work here: `graphhelm-cli` is a binary crate and cargo answers "no library
targets found". `--bins` is the incantation.

Sabotages, by what was changed rather than by line number, since lines move:

| id | change |
|---|---|
| S1' | in `record_consumptions_inner`, replace the pinned `next` (derived from the decision's own history) with a `store.next_sequence(...)` call placed after `after_validation()` — the pre-fix two-read shape |
| S3 | in the still-live filter, replace the rendezvous-equality test with `projection.wake_leases.contains_key(&lease.session_id)` |
| S6 | insert `if true { return 0; }` at the top of `record_consumptions_inner`, before the store is opened |
| S0 | in `sweep`, replace the phase-3 `spawn_blocking(record_consumptions(...))` with a discard of its arguments |

**What N = 1 does and does not buy.** For the deterministic sabotages (S1', S3, S6) one run is
enough: the interleaving is forced by the seam, not raced for. For S0 it is enough for the
claim actually being made — that the belt CAN pass with the recorder deleted, which is all
"this guard does not detect a dead sweep" requires, since a single miss establishes
non-detection. It would NOT support the stronger claim that the belt always passes in that
state, and this report does not make it.

### The last two runs, and a seed that came out of one

**Wrong-reason sabotage** (right lease, reason forced to `StaleRendezvous`): only
`a_live_lease_consumption_still_records` falls, `left: StaleRendezvous, right: Rung`. Before
the assertion B required, that sabotage was invisible — the count was still 1.

**S4**, keying the consumption idempotency without the sequence: **nothing falls.** All four
guards green, and the belt green too. B predicted exactly this, and the mechanism is B's: an
idempotent replay returns the prior batch without appending, so a second consumption is
swallowed in silence, the count still reads one, the lease stays live and every oracle holds.

That makes the sequence component of the idempotency key one more only-defense with no test —
the same shape the rendezvous comparison had before this change. Per B's ruling it is a
**seed**, named in the commit body already measured, not a widening of this PR.

Final state: commit aac0d67, 4/4 wake guards, 24/24 CLI unit suite, belt green, clippy clean.

### Two verification errors of mine, kept here on purpose

Both would have made me report a state worse than reality — the safe direction, but wrong.

1. `git commit --amend --only` with no paths rewrote the message and silently left the staged
   assertion out of the commit. Caught only because the insertion count did not move.
2. Checking whether the assertion had landed, I used `Select-String -SimpleMatch` with a
   pattern containing escaped parentheses. Under `-SimpleMatch` the backslashes are literal,
   so it matched nothing, and I nearly reported the assertion missing when it was present.

The lesson is the one this whole report is about, turned on the tooling: a check that can only
fail in one direction is not a check. Both were caught by a number that disagreed with a
claim, which is the same move as the rest of this document.

## FINDING: the #55 family's oracle is blind to its own worst case

Surfaced when M noted that P3 is the only prediction on the board describing a SILENT
failure. It is worse than a one-off.

Every guard in this family uses **"the stream still replays"** as its oracle — the two
in-module reds, the 15-round belt test, and the new window-3 red alike. That oracle can only
see failures that make the log ILLEGAL. A burn of a live lease is perfectly legal: the fold
accepts it, replay succeeds, and the operator's own surface reports calm while the store's
receipt says the lease rang.

So if P3 confirms, the conclusion is not merely "one more defect". It is that the family's
oracle cannot detect the failure mode the product cares most about, and every green it has
ever produced means less than it reads. The wedge-and-silence work of M09 exists precisely
because absence must never be laundered into calm; here the TEST STRATEGY does the
laundering.

This is a finding about how we measure, not about the product, and it does not wait for the
second PR.

## Amendments registered after sealing, before any run

- **P1's kill needs an N.** "Reproduces at the parent of 53d212d" was registered without a
  sample size, so a non-reproduction could not be scored at all — absence of reproduction is
  not evidence of absence at an unstated N. With the flake near one in three: zero hits in
  ten runs is meaningful (~1.7% under p=1/3) and scores as a kill; zero in three is not
  (~30%) and must score uninformative. Minimum for a kill: N >= 10 at the parent.
- **The "predates" claim cannot be killed by a measurement, and the ledger was asked to say
  so.** Two clauses with different evidence classes were being scored as one. That the window
  predates 53d212d is STRUCTURAL — read from `with_lock` taking and releasing per call and
  `open` releasing before returning, verified independently at both commits. No timing result
  can make a released lock held. That "reverting 53d212d therefore restores nothing" is
  EMPIRICAL and is the clause a parent-rate measurement can hit; if the widening did raise
  the hit rate, reverting reduces exposure without closing the window. A kill there makes the
  commit's claim incomplete, not false, and only the second clause gets rewritten.

## The three measurements that decide everything (for W1 / the fix workers)

1. **Capture the failing assertion TEXT of flake 2** (nocapture on a failing run). The
   message names the mechanism: "lease burned on the ring" -> H1; "sidecar must exit 0"
   -> H2; "ZERO requests" -> H3; "never created its rendezvous" -> H4.
2. **Capture a failing run's journal for flake 1** (dump journal.jsonl on oracle
   failure): two consumes for one arm at consecutive sequences = F1-H1 confirmed;
   a storage-shaped refusal instead = H3.
3. **Run flake 1's test at the parent of 53d212d** vs branch tip: same rate = the
   stash's "born when reads went shared" story is dead and the fix note must be
   rewritten before it teaches the wrong lesson.

## M09 gate red: diagnosing a flake without a working instrument

The gate on the merged tree (`d10916b`) went RED at the last stage, "PostgreSQL matrix under a
non-C collation": `admin_operator_binds_pool_profile_and_source_identity` panicked at
`backup_restore.rs:2148:14` on `restore_from_path(...).unwrap()` with `Err(InvalidRestore)`.
Every other stage was green, including all twelve CLI suites and the FIRST PostgreSQL matrix.

### Three facts that separate "my merge" from "not my merge" without re-running anything

1. **The same test passed at L373 and failed at L1238 in the same run.** Both stages run the
   identical command on the identical binary. Any deterministic, collation-independent breakage
   takes both. It took one. This alone excludes the entire class my diff could belong to, and it
   cost zero machine time — it was already in the log.
2. **The diff touches no PostgreSQL code and no SQL.** The only new ordering structure is a
   `BTreeMap<String, _>`, ordered by Rust byte order and locale-independent by construction.
3. **The failure site is timeout-saturated.** The operator is built with a 30s process timeout and
   the restore path maps elapsed -> `InvalidRestore` at many sites. The stage took 92.65s, last in
   a long gate, machine hot and disk-pressured.

`InvalidRestore` is a deliberately redacted, heavily-overloaded error — good for not leaking
operational detail to a caller, and precisely why the red carries no diagnosis on its own.

### The instrument that nearly invented a bug

The isolation harness written to separate "load flake" from "real collation bug" returned 0/3.
Under the reading declared BEFORE the run, 0/3 meant "deterministic under the non-C locale →
pre-existing collation bug". The test had never run: the argument vector went through
`Start-Process -ArgumentList` as one comma-joined token and cargo read it as a toolchain name.

This is a **vacuous red**, the category from PR 2 — but a strictly worse instance, and the reason
it deserves its own row. A vacuous red normally only fails to prove something. This one **pointed
at a specific wrong answer**, because a broken harness and a genuine deterministic bug are
indistinguishable from the exit code: deterministic-fail is what both look like. Nothing about the
result *looked* wrong. Only the executed-test count did.

The fix is structural, not attentional: the harness classifies on the runner's own
`<test-name> ... (ok|FAILED)` line, has three outcomes instead of two (PASS / FAIL /
**HARNESS-BROKE**), and refuses to compute a rate over iterations that measured nothing.

**Transferable rule:** assert the test RAN before reading the result. It is the same rule as
"sabotage must be red at its own assertion, named by panic site", moved one layer out — that rule
governs the guard, this one governs the harness that runs the guard. An exit code cannot tell
"ran and failed" from "never started", so it must never be the thing a rate is computed from.

### The trap issue #19 names, kept in view

#19: "A gate that cries wolf under load trains the operator to re-run until green, which is how a
real regression eventually slips through." An isolation re-run here is a *measurement of which red
this is*, not a re-roll for green. The correct output is a rate handed to whoever decides, not a
push. #19 registers `constructor_bounds_reconciliation_catalog_locks` at `:2268`; this failure is
a third instance of the same class at an **unregistered** site, `:2148` — new information for the
issue, not a duplicate of it.
