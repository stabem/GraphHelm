# #80: the three tests that certify the bug, and the override that does not exist

Written by D (writer, M10 #80), 2026-08-19, while blocked on a ruling. Every claim below was
established by reading consumers, not by inferring from a predicate. Where I inferred, I say so.

## Status of claims

Nothing here is retracted at the time of writing. If that changes, the retraction is written **in
place**, next to the claim, in capitals. Do not trust an enumeration of dead claims kept somewhere
else — grep this file for RETRACTED / WITHDRAWN / SUPERSEDED.

## 1. The fix breaks three currently-green tests

All three reach `examples/graphs/manual-override-deploy.yaml` through the `resume` helper in
`apps/cli/tests/execution_cli.rs:830`, which hardcodes that graph.

The graph is `implementation -> deploy`: one edge, `type: data`, no `condition`. Every fixture these
tests use parks `implementation` in `Blocked` (fixture `failure`, driven to the no-progress bound) or
`WaitingInput` (fixture `unknown`). Neither state is in `satisfies_dependents`
(`core/execution/src/ready.rs:27`, which admits only `Succeeded | Waived | Skipped`), so the edge
gates and `deploy` is edge-unsatisfied for the entire test.

| Test | Line | What it asserts today | Under the gate |
|---|---|---|---|
| `pause_holds_ready_work_and_resume_completes_it` | :867 | `deploy` succeeds while `implementation` stays `blocked` | `deploy` stays `Queued`; fails |
| `resume_never_redispatches_a_waiting_node` | :1006 | `succeeded == 1`, i.e. `deploy` ran | fails |
| `the_operator_story_runs_end_to_end_and_replays_byte_identical` | :1085 | the Task 6 story: resume "force-dispatches it to completion" | fails |

Blast radius, as far as I checked: `heldNodes` appears in tests only at :880, :1018 and :1190, and
:1190 is inside the story test. **NOT CHECKED, and not claimed safe:** `runtime_http.rs:495` and
`gate_http.rs:440` also pause and resume, over HTTP, against different graphs.

These are not stale tests that drifted. They assert this behaviour deliberately, and :1067-1083
explains at length why it is correct. Rewriting them is a decision about what the product does, which
is why this file exists instead of a patch.

## 2. The justification those tests give has no consumer

The story test's doc comment (:1070) credits the behaviour to
`deploy.userOverrideAllowed: true` — "a human overrides a blocked predecessor".

Every occurrence of that field, across the whole repository:

- `core/governor/src/externalize.rs:689` — validates the value is a bool.
- `core/governor/src/externalize.rs:1136` — iterates it with `userEditable` in a key list.
- `core/graph/src/persistence.rs:1856` — permits the key under `node_configuration`.
- `core/protocols/src/diagnostic.rs:326` — names it in a diagnostic list.
- `schemas/node.schema.json:25` and the 1.0.0 release copy — types it as a boolean.
- `docs/graph-engineer/GRAPH_DSL_SPEC.md:119` — documents that it exists.
- `core/governor/tests/safe_publication.rs:281` — a test setting it to `false`.

Validation, persistence, schema, docs. **No scheduler, driver, or dispatch consumer anywhere.**

The graph's `manualOverride` policy block is a separate mechanism and also not this one: its consumers
are `core/policy/src/evaluator.rs` (obligation discovery — `bypassedRequirements` become obligations)
and `core/governor` safe-publication sealing. That is publication-time governance deciding whether a
GRAPH MAY BE PUBLISHED, not whether a node may be dispatched.

The decisive check, and the cheapest one:

```
rg -i override core/execution core/runtime
```

**No matches.** The string does not appear in either crate in any case. There is no dispatch-time
override mechanism in this system.

So `deploy` does not run because an override authorised it. It runs because `pause` held it on bare
state, `resume` force-recorded `Started`, `(Paused, Started) => Queued`, and the driver's retry chain
never checked edges. The field is decoration sitting on top of the exact defect #80 reports — and the
decoration talked a test into certifying it.

This is the shape of #79 again: the vocabulary lives on one surface and the mechanism lives somewhere
else entirely. The difference is that here the mismatch did not merely confuse an operator, it
produced a green test asserting the bug.

## 3. What the ruling is actually about

After this gate there is no way, anywhere in the product, for an operator to force a gated node to
run.

That may be correct. `Waived` and `Skipped` DO satisfy dependents (`ready.rs:27`, whose own comment
grounds them in D-019 sovereignty), so the operator already has a real, recorded, replayable lever
for exactly this situation, and it names what was given up. An accidental override that leaves no
record is strictly worse than that.

Or the example graph and `GRAPH_DSL_SPEC.md:119` mean an override was intended and never built — in
which case #80 closes a hole and simultaneously deletes the only thing that ever delivered the
feature, and that deserves its own issue rather than a silent test rewrite.

- **(a)** Rewrite the three tests to assert the gate holds and `deploy` stays `Queued`; open an issue
  recording that no dispatch-time override exists. *(D's lean.)*
- **(b)** Treat the override as intended, in which case the fix needs a carve-out and whoever writes
  it needs to know what authorises the carve-out before writing a line.

## 3b. Sealed predictions — written before any cargo run

Sealed at typing time, against base `0f4e7fe`, to be run with `CARGO_TARGET_DIR=D:/graphhelm-target-m10`
(shared warm cache, slot queued behind C). Each has a stated way to be wrong. Postmortem, if any,
gets written in the terms below and not in softer ones.

**P1 — the unit split.** `dispatch_candidates`'s five original tests split 2 RED / 3 GREEN before the
one-condition fix: RED = `a_queued_node_behind_an_unfinished_predecessor_is_not_a_candidate`,
`a_queued_node_whose_predecessor_was_invalidated_is_not_a_candidate`. GREEN =
`a_queued_node_whose_predecessor_finished_is_still_a_candidate`,
`a_queued_root_node_is_always_a_candidate`, `the_ready_half_of_the_union_is_unchanged`.
FALSIFIED IF any listed GREEN is red, or any listed RED passes, before the fix.

**P2 — the failure-edge pair.** Of the two added later,
`a_queued_failure_handler_dispatches_when_its_source_failed` PASSES pre-fix (the buggy union admits
every `Queued` node) and `..._is_not_a_candidate_when_its_source_succeeded` is RED pre-fix.
FALSIFIED IF the first is red pre-fix — that would mean the union is not the unconditional insert I
read, and my account of the defect is wrong.

**P3 — the gate delays `deploy` by one driver pass rather than blocking it.** This is the load-bearing
prediction for the rewritten flagship story. At resume, `implementation` is `Ready` and rootless so it
dispatches; `deploy` is `Queued` and gated so it is excluded from that pass; `implementation` succeeds;
the driver re-derives candidates on its next pass, `deploy`'s edge is now satisfied, and it dispatches.
Both reach `Succeeded` and the aggregate completes.
FALSIFIED IF the story ends with `deploy` still `Queued` — which would mean the driver does not
re-derive candidates per pass, and `approve_is_not_a_dead_end_once_the_condition_is_fixed` (:729)
should fail too. **That test is the tell: if P3 is wrong, :729 goes red, and it is currently green.**

**P4 — blast radius is four tests in one file, and the two HTTP tests are unreachable.** Walked rather
than assumed, per the instruction not to hope:
- `runtime_http.rs:495` — SAFE, and not because of the graph. The test never re-drives after
  `approve`: it verifies the refusal lifts by reading status, deliberately "rather than re-driving
  into the still-hanging fake provider". No dispatch decision is taken after the gate could matter.
- `gate_http.rs:440` — SAFE structurally. Its graph is a single `quality_gate` node with
  `edges: []`. With no incoming edges, `satisfied_with` releases unconditionally, so the gate cannot
  change any candidate set in that test. Its phase-1 refusal is the GATE precondition, an unrelated
  mechanism.
FALSIFIED IF either goes red.

**P5 — `approve_is_not_a_dead_end_once_the_condition_is_fixed` (:729) stays green, unmodified.** It is
the fourth test in the blast radius and the template the rewritten story now follows.
FALSIFIED IF it needs any edit to pass.

## 4. Two empty populations, and a retraction of my own

Both bear on whether the attention half of #80 should ship at all.

**Clause one — the Invalidated/Cancelled narrowing.** `NodeOutcome::Invalidated` has no production
emitter (every occurrence is a test). `NodeOutcome::Cancelled` has one, `cancel.rs:94`, which cancels
every non-terminal node in the same sweep and so cannot leave a live dependent behind. The apparent
second path, `driver_contract.rs:245`'s `GatewayError::Cancelled`, maps at `executor.rs:167` to
`NodeOutcomeReason::Cancelled` — a different type, no reachability.

**Clause two — the wedge predicate. THIS PARAGRAPH CORRECTS A CLAIM I MADE AND SENT TO TWO SESSIONS
BEFORE CHECKING IT.** I claimed the gate creates a reachable wedge with no voice, because
`advances_without_the_operator(Queued)` (`attention.rs:449`) is unconditionally true and would
suppress `WedgedQuiescence`. **RETRACTED.** The emitter (`attention.rs:659`) reads
`claims_running && !anything_advances && reasons.is_empty()`, and I had not read the third conjunct.
Any other reason already suppresses WedgedQuiescence, so the `Queued` arm only decides an outcome in a
run where nothing else spoke — and no reachable predecessor state permits that. Failed raises
FailedNode; Blocked raises BlockedNode; WaitingInput raises WaitingInputNode; Cancelled takes the
dependent with it; Invalidated cannot happen; Draft, Linting, Ready, Running, Queued and
WaitingCapacity each make `anything_advances` true, so the branch is never entered; `Paused` forces
the aggregate status out of `running`, so `claims_running` is false.

The error is worth recording as a method note, because it is the fourth of its kind in this milestone
and the first one caught before it reached code: **reasoning from a predicate without reading what
consumes the predicate.** It also happened to argue for shipping something I had spent the previous
hour arguing against, which is the direction these errors keep pointing.

In the scenario the suite actually reaches, `deploy` ends `Queued` behind a `Blocked`
`implementation` and the operator hears `BlockedNode` — pointed at `implementation`, which is the
thing that actually needs fixing. Nobody is left calm and nobody is misdirected.
