# Milestone 07 — The One-Glance Answer

**What this milestone was.** The first GraphHelm milestone whose entire backlog was written
by the product judging the product. M06's dogfood ended with the blind judge refusing the
one-glance story on the owner's subscription and recording four findings on-stream. M07's
scope was those four findings and nothing else, and its closing rule was that the same
judge, on the same story, had to stop making them.

## The four findings and what they became

**F1 (critical) — the surface reported a green all-clear on a wedged execution.**
The fix is an extraction, not a field. Three copies of "is anything wrong?" existed
(`execution::render`, the monitor page, and `recovery`'s inline condition). One pure
function now owns it — `graphhelm_execution::attention` — and `render` and the monitor both
CALL it. `attentionRequired` is derived from the reasons (`!reasons.is_empty()`), never
declared beside them, so the field and its justification cannot drift apart. The monitor's
header states the verdict in words: `needs you: review was interrupted and never triaged`
or `can sleep — nothing is waiting on you`.

**F2 (high) — `nodeStateCounts` had no failure dimension.**
All sixteen lifecycle states are emitted, zero-filled. This REVERSED a documented
guarantee: `api_http.rs` pinned the omission on purpose, so the assert was inverted with its
reasoning rewritten in place rather than deleted.

**F3 (high) — `retryable_failure` carried no cause.**
`NodeOutcomeRecorded` gained an optional `reason` from a closed vocabulary — the fourteen
route classes named one-for-one with the gateway taxonomy, plus `empty_reply`,
`malformed_judgment`, `judge_refused`, `gate_refused`, the four tool dispositions, and
`fixture_scripted`. Closed, never free text: the durable stream must not carry provider
prose, so the class rides the event and the text seals to Evidence under D-036. The
gateway-error arm — which knew the most and recorded the least — now seals its material too.

**F4 (high) — `wake_status` could not say whether the alarm had fired.**
The fold keeps the last consumption per session, recorded from the CONSUMPTION event, never
reconstructed from the lease being removed. `wake_status` returns `live`, `cursor`, `head`,
`contentHead` and `lastConsumed`, so "rang at #N", "burned as stale at #N" and "never armed"
stop collapsing into `live:false`.

## The closing rule, and what it caught

The rule was that M07 could not close on our opinion that we had obeyed. It closed on the
judge's findings no longer including F1–F4. Three real runs on the owner's subscription
were needed, and the rule earned its keep twice:

1. **The wedge rule was dead code in production.** A real execution never emits
   `simulation_started`, so `simulation_status` stayed `None` for the whole run and the
   wedge arm demanded `Some(Running)`. It could only ever fire for simulation-driven
   stories — never for the live run an operator watches. Both agents shipped this; both
   agents' tests used fixtures with the status set by hand. The judge found it by reading
   `status: null` on a live probe.
2. **`status` itself was null on every read of a live run**, and the monitor printed
   `unset`. The one field named for the run-level verdict carried nothing. Two asserts that
   pinned `null` were inverted: they were right about the events and wrong about the
   operator.

**The verdict trail** (`docs/acceptance/m07-run-2026-08-17/`): run 1 — 8 findings, 1
critical; run 2 — 7 findings, 2 critical; run 3 — 7 findings, **zero critical**, and the
judge crediting a fix in its own words: *"wake_status does correctly separate contentHead
(12) from head (14)"*. In run 3 the judge quotes `reason "tool_exited_non_zero"` from the
event log (F3 answered), reads the `failed` bucket (F2 answered), reads `lastConsumed`
(F4's field present), and never repeats the wedge complaint (F1 answered). Its remaining
findings are new dimensions, recorded as the M08 seed.

## Honest limits

1. **The judge never passes, and the closing rule does not ask it to.** Refusal-with-findings
   is its charter; a bare pass may be unreachable by construction. The criterion is
   withdrawal of the named findings, which is checkable — but it means "the judge approved"
   is a sentence this project cannot say.
2. **F4's receipt was proven by tests, not by the live run.** Both attempts to arm and ring
   an operator alarm during the run failed for harness reasons — the serve drives the whole
   execution inside `/start` on a current-thread runtime, so a concurrent request cannot be
   served, and arming *before* the start is refused because no execution exists yet
   (`alarm-refusal.txt` is committed rather than hidden). The lease the judge inspected was
   its own. `rung`/`stale_rendezvous` receipts are covered by `wake_http`'s choreography.
3. **`wake_last_consumed` only grows.** A lease leaves the map when it burns; its receipt
   never does. Combined with the MCP `sessionId` being a per-process nonce, distinct
   sessions accumulate without bound in a long-lived execution. Not fixed on purpose:
   "keep the last N" is a policy that can hide the exact receipt an operator is looking for,
   and guessing a policy is worse than declaring a limit.
4. **A retried failure is invisible in the glance, by design and not by accident.** A node
   that failed and was re-queued needs no operator, so it raises no attention — but the
   judge is right that repeated flapping accumulates no visible signal. Retry is a counter,
   not a lifecycle state, so F2's remediation did not cover it. M08 seed.
5. **No liveness or time data in the glance.** `status` carries no `startedAt`, no
   `lastEventAt`, no heartbeat, so "healthy and working" and "stopped emitting" still render
   identically. The judge raised this in all three runs. M08 seed, and the largest one.
6. **The doorbell cannot be slept on from the MCP surface.** `wake_arm` and `wake_status`
   are exposed; the blocking wait is a CLI sidecar. An MCP client can only poll — the exact
   opposite of what the doorbell exists for. M08 seed.
7. **Two agents, three surfaces, one blind spot.** Both defects the closing rule caught were
   invisible to fixture-driven tests because the fixtures set by hand exactly the field
   production never sets. Cross-review did not catch it either; the live run did.
8. **The new invariant is not anchored in the acceptance map, and the map is right to
   refuse it.** "No surface may recompute the attention verdict" is proven by living tests
   (the monitor's one-truth guard, the API parity guards — all three fail under sabotage),
   but I tried to add it as a map clause and the grounding refused: the map mirrors §8 of
   the product requirements, and its count is pinned at exactly six. Anchoring the
   invariant therefore means adding a §8 promise to the spec, which is an owner-level
   decision about what the product guarantees, not something to slip in at the end of a
   milestone by bumping a number until the assert agrees. Recorded here so the gap is
   visible, and proposed for M08 as a spec change rather than a test edit.
