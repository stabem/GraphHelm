# Draft: the second story — "the night the release stalled"

Status: DESIGN DRAFT for owner review. REWRITE after adversarial review killed v1; this
revision additionally patches what the free scripted rehearsal caught by actually being
run: a marker-path detail (harness-only, not a spec defect); `mode: "manual"` not
parking the duplicate (a genuine runtime defect,
[issue #79](https://github.com/stabem/GraphHelm/issues/79)); and `pause`/`resume` not
respecting `verify`'s edge dependency, which is why this story now needs two rounds of
pause/approve/resume instead of one (also a genuine runtime defect,
[issue #80](https://github.com/stabem/GraphHelm/issues/80)). See "The world the recipe
builds", "The block mechanism", and "Note for future stories" for the detail on each.
Not yet run for the paid judge call. Written from evidence at branch
`issue-m09-arming-the-alarm` (53d212d).

## Revision note (read this first)

The first draft of this story (same file, same premise) was blocked before a paid run by
an adversarial cross-review (E Agent, verified independently by the orchestrator). It
built the deploy "stall" as a `time.sleep(3600)` interrupted by `pause {"mode":"immediate"}`.
Two facts, verified against code at this same commit, broke it:

1. **The tool host applies a fixed 300s timeout to every shell call, independent of the
   operator** (`apps/cli/src/commands/serve/ports.rs:27`, `TIMEOUT_SECONDS: u64 = 300`, no
   per-call override — `ports.rs:25-27`'s own comment says so). On expiry the process is
   killed and the disposition is `TimedOut` (`adapters/tool-host/src/host.rs:106,175`),
   which maps to `NodeOutcome::RetryableFailure`, **not** `Interrupted`
   (`core/runtime/src/executor.rs:241-243`). A `RetryableFailure` on attempt 1 (well under
   `MAX_NODE_ATTEMPTS = 8`, `core/execution/src/bounds.rs:7`) goes to `Queued`
   (`core/execution/src/transition.rs:97-105`), and the async driver auto-redispatches
   `Queued` nodes with **no operator action**
   (`core/runtime/src/driver.rs:470-488`, `retry_pending` folded into `candidates`).
   Since the story's own forcing moment 1 (`wake_wait`) mandates a 60-120s wait *before*
   the judge even reads events, and moment 2 (`signal`) comes before pause, a realistic
   judge run risks losing the race against the unannounced 300s clock — the deploy node
   self-heals via ordinary retry machinery before `pause`/`approve` are ever forced, and
   the failure is silent (the judge just watches it finish and reports fine).
2. **`pause {"mode":"immediate"}` does not kill the underlying process even when it does
   fire in time.** `apps/cli/src/commands/serve/ports.rs:219-221`, verbatim: "No cancel
   hook: `ToolHost` exposes no kill surface for in-flight children beyond what the
   process's own deadline already bounds ... the default no-op is accepted." The driver's
   `in_flight.abort_all()` (`core/runtime/src/driver.rs:590`) drops the Rust future
   wrapping a `tokio::task::spawn_blocking` call (`ports.rs:199`); dropping that handle
   does not stop the blocking closure. The real child process survives, orphaned, until
   its own 300s deadline — on Windows in particular, nothing kills it when `serve` itself
   is torn down.

Both facts are permanent properties of this runtime, worth keeping in mind for any future
story that *does* need a genuinely long-running interrupted step — see "Note for future
stories" at the end. This rewrite sidesteps both by building the stall out of a
**deterministic, near-instant, self-counting failure** instead of a sleep, using a
different bound the state machine already enforces:
`MAX_IDENTICAL_OUTCOMES = 3` (`core/execution/src/bounds.rs:10`, checked less than
`MAX_NODE_ATTEMPTS` at `bounds.rs:34`). Every forced moment below is now either fully
deterministic or, where a real judge's own pace still varies, explicitly marked as such
— no forcing depends on beating a clock nobody announced.

## Why this story exists

`docs/acceptance/m08-judge-coverage.md` measured that all seven M08 judge runs walked the
same half of the 14-tool surface and licensed exactly one action: a second story that
**must produce the moments the first cannot** — "a node that stops and waits for human
approval, an execution that is paused and resumed, and one that is cancelled" (lines
51-56). This draft is that story.

The split, from the measurement (`m08-judge-coverage.md` line 16, source
`read-audit.jsonl` across `m08-rejudge{5..9}-2026-08-18`):

| touched (7) | never touched (7) |
|---|---|
| `start`, `status`, `events`, `wake_arm`, `wake_status`, `amend_budget`, `routes` | `signal`, `approve`, `pause`, `resume`, `cancel`, `probe`, `wake_wait` |

The first story ("As an operator I open the monitor and know in one glance whether I can
go back to sleep") is a **read-only** story. A world that never blocks on a human never
produces an `approve`; one that never pauses never produces a `resume`. The second story
therefore casts the judge as an operator who must **act**, and builds a world where every
act is the natural next thing a real operator would do — never a checklist.

## Story premise

The nightly release run tried to deploy, failed the same way four times in a row, and
gave up rather than loop forever — and the scheduler has accidentally submitted the same
release a second time. The judge is the on-call operator paged after the failure alert:
they must establish that the run is genuinely stuck rather than slow (without polling),
leave a record for the day shift, take manual control before touching anything, approve a
retry now that they understand why it stopped, resume so the release still ships, and
make sure the duplicate cannot double-deploy.

This is, if anything, a more realistic incident than a literal hang: exhausted-retries-
then-give-up is exactly what real deployment automation does (backoff, cap, stop), where
a `sleep(3600)` was a synthetic device standing in for one.

## The world the recipe builds (before the judge wakes)

Three executions on one `serve` process (same harness as
`docs/acceptance/m08-rejudge9-2026-08-18/recipe.sh`: gateway key, broker/keyring,
`judge_route` manifest, `--read-audit`):

1. **`exec-m09-release`** (the primary) — started by the recipe in **autopilot**,
   awaited **synchronously** (no background thread needed — see "One run, one recipe"
   below for why). Its `deploy` node fails identically four times, fast, and blocks; the
   `start` call returns once the drive has nothing left to dispatch. Nothing sleeps and
   nothing races a clock: the whole sequence is a handful of sub-second tool calls plus
   event-store round trips.
2. **`exec-m09-release-dup`** (the duplicate) — the same release graph under a different
   `executionId`, started by the recipe in **autopilot**, awaited synchronously, exactly
   like the primary. It hits the identical stuck deploy step and blocks the same way — a
   duplicate scheduled job that failed to double-deploy only by accident, dormant but not
   yet safe: `approve`+`resume` could still ship it later if nobody stops it, which is
   exactly what makes `cancel` necessary rather than decorative. Cancellable at any time
   regardless of how it got there (`cancel.rs`: "applies from any non-terminal state").
   *Revision note:* an earlier draft of this section tried to park the duplicate via
   `mode: "manual"`, relying on "nothing auto-starts out of manual." Running the free
   rehearsal disproved that: `core/runtime/src/driver.rs`'s async drive (the only path a
   `type: tool`-only graph like this one is ever eligible for, since `NodeType::Tool`
   always classifies OK regardless of `state.runtime` — `core/runtime/src/classify.rs:33`)
   contains no reference to `mode`/`Autopilot`/`Manual`/`Supervised` at all — confirmed by
   grep, then confirmed empirically: `exec-m09-release-dup` started with
   `mode: "manual"` produced `execution_started{mode:manual}` followed immediately by
   every node auto-approved, dispatched, and completed, exactly as autopilot would. A
   product defect, tracked as [issue #79](https://github.com/stabem/GraphHelm/issues/79)
   (M10) — see "Note for future stories." A design
   depending on it here would have been resting the story's whole "duplicate must be made
   incapable of ever deploying" goal on behavior that does not exist. `mode: "autopilot"`
   for the dup, matching what actually happens, is the honest choice, not `"manual"`.
   A second, related trap this section fell into and had to back out of: "start the dup,
   then immediately `pause` it" does not work either — `start`'s HTTP handler awaits the
   *entire* async drive to quiescence before responding (`apps/cli/src/commands/serve/
   routes.rs`'s `start`/`resume` handlers, both `.await` `drive()` fully), so by the time
   a caller receives `start`'s response and could issue a `pause` call, a fast Tool-only
   graph like this one has already run to completion. Racing the two calls concurrently
   instead (background the `start`, fire `pause` alongside it) would work sometimes and
   not others depending on exact scheduling — reintroducing the same "forcing depends on
   beating an unannounced clock" defect this rewrite exists to eliminate. Giving the
   duplicate its own identical, deterministic block (below) needs neither trick.
3. **`exec-m09-judge`** — the judge's own execution, same minimal shape as the first
   story (`prepare` → `judge` evaluator), started last, in the foreground. The judge
   never learns this id; the surface names only the two release executions.

By the time the judge starts, the world is **already fully settled** — both releases have
either succeeded or are blocked, decided before the paid call begins. There is no window
in which the recipe's own setup is still "in flight" when the judge's clock starts.

### The block mechanism (deterministic, no clock, no sleep)

The `deploy` node is a shell tool call running python: it reads a byte count from a
marker file in the run directory (0 if the file does not exist), appends one byte, and
exits `1` if the count it read was less than 4, or `0` (success) otherwise. Four calls
fail identically; the fifth succeeds. No timing, no timeout, no race — every step
completes in well under a second, nowhere near the tool host's fixed 300s process
deadline (`apps/cli/src/commands/serve/ports.rs:27`), so that deadline is never in play
for this story at all.

The marker file path is an **absolute path under the run directory** (`<RUN>/
deploy.attempt`), never a path relative to the tool workspace. This matters and was
verified the hard way: `ServeToolPort`/`ToolHost` builds a fresh, disposable staging
checkout per HTTP mutation call (`HostConfig.keep_workspace: false`,
`apps/cli/src/commands/serve/ports.rs:178`) — state written under a workspace-relative
path survives retries *within* one `start`/`resume` call (the same `Arc<ToolHost>` is
reused for every dispatch inside one drive) but is gone the moment that call returns, so
a workspace-relative marker would silently restart the count-to-4 cycle after every
`resume`. An absolute path outside `project`/`staging` is untouched by that teardown.
**The primary and the duplicate must use *different* marker paths** (`deploy.attempt` vs
`deploy-dup.attempt`) — sharing one lets whichever execution runs second inherit the
first one's already-advanced count and skip straight to success, which is exactly the
cross-contamination an earlier pass of this rehearsal produced by accident.

The state machine forces the rest, verified line-by-line against this commit:

- `NodeOutcome::RetryableFailure` is "identical" to the prior one purely by enum value
  (`core/events/src/projection.rs:293-298`, `identical_outcomes_for`); the fold that
  tracks the run length is `core/events/src/projection.rs:838-855`. Tracing it: attempt 1
  reports with 0 prior identical outcomes (Queued); attempts 2 and 3 report with 1 and 2
  prior (still Queued); attempt 4 reports with 3 prior, which meets
  `MAX_IDENTICAL_OUTCOMES = 3` (`core/execution/src/bounds.rs:10`), and
  `apply_transition`'s exhaustion arm blocks it (`core/execution/src/transition.rs:97-105`,
  matching its own unit test at `transition.rs:194-201`). Four failed attempts, all
  automatic, no operator action between them — the driver auto-redispatches `Queued`
  nodes on its own (`core/runtime/src/driver.rs:470-488`).
- The primary's `start` call's async drive, once `deploy` is `Blocked` and nothing else
  is `Ready`/`Queued` (`verify` depends on `deploy`), has nothing left to dispatch and
  returns (`core/runtime/src/driver.rs:491-493`, then the non-cancelled quiescence path at
  `driver.rs:621-665`, which returns as-is when `all_terminal` is false — it does not loop
  forever and does not require a pause to unstick).
- This `Blocked` node's `last_outcome` is `RetryableFailure`, not `Interrupted`. Per
  `core/execution/src/recovery.rs:38-40`'s own documented distinction, a node blocked for
  exhaustion "does not hold resume: the owner may legitimately resume the rest of the
  graph and deal with it later" — `resume` is **not refused** here, unlike v1's design.
  What forces `approve` instead: `resume` only redispatches nodes recorded `Paused`
  (`apps/cli/src/commands/execution/resume.rs:230-236`, the `paused_nodes` filter), never
  `Blocked`. And `apply_transition`'s match is exhaustive: the only route out of `Blocked`
  toward completion is `(S::Blocked, O::Approved) => Ok(S::Ready)`
  (`core/execution/src/transition.rs:79`) — `Waived`/`Skipped` also apply but give up
  rather than ship (a legitimate "bad handling" trap, below). So a `resume` call before
  `approve` is not an error — it just quiesces again immediately, having redispatched
  nothing, and the release still has not shipped. **Approval is structurally required —
  not because anything refuses, but because nothing else can move a `Blocked` node
  forward.** This is a softer force than v1's hard `UntriagedInterruption` refusal, but
  equally load-bearing, and it is honestly described as such rather than overclaimed.
- `resume` itself is gated on `simulation_status == Paused`
  (`core/execution/src/recovery.rs:58-60`, `ResumeError::NotPaused`). Nothing in this
  design ever sets that status except an explicit `pause` call — the primary's `start`
  call quiesced without pausing (only the *cancelled* branch of the async driver appends
  `execution_paused`; the plain not-cancelled quiescence path at `driver.rs:621-665`
  never does). So the operator must call `pause` before `resume` will do anything at all
  — not to interrupt in-flight work (there is none by the time they act), but because it
  is the only way to reach the state `resume` requires. Graceful (default) `pause` is
  sufficient and correct here: it is allowed whenever status is `None`/`Running`
  (`apps/cli/src/commands/execution/pause.rs:69-80`) and appends `ExecutionPaused`
  unconditionally. `"mode":"immediate"` is neither needed nor useful here — with no live
  cancel sender registered (the drive already deregistered on return), it falls through to
  this same graceful path anyway (`apps/cli/src/commands/serve/routes.rs:545`'s own
  comment).
- **The held-node list at that first `pause` is not actually empty**, and this is the
  reason the story needs *two* rounds of pause/approve/resume rather than one — tracked
  as [issue #80](https://github.com/stabem/GraphHelm/issues/80), found by the free
  rehearsal, not guessed. `pause`'s `held` filter selects every node in bare state
  `Ready`/`Queued` (`apps/cli/src/commands/execution/pause.rs`), which does not apply the
  same edge-dependency check `ready_set()` uses before treating a `Ready` node as
  genuinely dispatchable (`core/execution/src/ready.rs:93-127`). `verify` sits `Ready`
  the whole time (auto-approved from `Draft` unconditionally, every loop pass —
  `core/runtime/src/driver.rs:446-468` — regardless of whether its edge from `deploy` is
  satisfied) even though `ready_set` correctly never dispatches it while `deploy` hasn't
  succeeded. `pause` catches it anyway and records it `Paused`.
- `resume`'s `paused_nodes` redispatch (`resume.rs:230-236`) then force-records
  `NodeOutcome::Started` for `verify` unconditionally — no re-check against `ready_set`,
  no check that `deploy` has actually succeeded yet. Worse, `verify` does not merely wait
  behind `deploy`'s own pending retry: `core/execution/src/dispatch.rs:28-42`'s
  `dispatch_plan` is attempt-fair (sorts candidates by ascending prior-attempt count, with
  `MaxParallelModelCalls: 1` on this graph), and `verify` (0 prior attempts) always sorts
  ahead of `deploy` (4 prior attempts by then) — deterministic, not a race, confirmed by
  raw sequence in an isolated rehearsal run: `verify`'s own four attempts land at
  sequence 22-30, `deploy`'s real fifth attempt only at 31-33, in the same `resume` call,
  every time.
- **`verify`'s own script now genuinely depends on `deploy`'s effect** (reads the same
  marker, requires the byte count `>= 5` — the count `deploy` itself will have written
  once it has *actually* succeeded), rather than the unconditional `git status` an
  earlier pass of this rewrite used. That earlier version let a prematurely-dispatched
  `verify` **falsely succeed** — a silent lie the free rehearsal caught (issue
  [#80](https://github.com/stabem/GraphHelm/issues/80) again). The marker-dependent
  version fails honestly instead — but an honest failure four times running is still a
  `RetryableFailure` four times running, and the *same* `MAX_IDENTICAL_OUTCOMES = 3`
  machinery that blocks `deploy` blocks `verify` too, for the same reason. So the first
  `resume` (after approving only `deploy`) ships `deploy` for real **and**
  independently blocks `verify` as a side effect nobody asked for but the surface
  now honestly reports (`blocked: 1` in the reply, `status` still not `completed`).
- The operator therefore needs a **second round**: `approve` on `verify` (now legally
  `Blocked`, where it was refused as merely `Paused` earlier — verified directly:
  attempting `approve` on a `Paused` node returns `409`, `"node is paused, not ghost or
  blocked"`, so `verify` genuinely cannot be pre-approved before its own first, forced,
  premature attempt happens), then `pause` again (the first `resume` already flipped
  `simulation_status` back to `Running`, and `resume` refuses `NotPaused` exactly as
  before), then `resume` again — this time `verify`'s real check reads the marker at 5
  and succeeds, and the execution completes. Six mutation calls total across the two
  tools this section is about (`pause` twice, `approve` twice, `resume` twice) — the
  tool *coverage* the M08 measurement cares about is unaffected (each tool still gets
  exercised at least once); what changes is that this story forces the operator to
  notice a job that "resumed successfully" is not the same thing as a job that shipped,
  and to finish the triage rather than declare victory early. That is a genuine,
  deterministic discriminator, not a flaw in the story — see moment 4/5 below.

## The graphs (first story's format)

### `release.yaml` (primary; `release-dup.yaml` differs only in `metadata.id`, `metadata.executionId`, and the marker filename)

`release-dup.yaml`'s `deploy` node runs the identical script, with `deploy.attempt`
replaced by `deploy-dup.attempt` — a distinct marker file, never the primary's own (see
above for why sharing one is a real, previously-hit bug, not a hypothetical).

```yaml
apiVersion: p50.dev/graph/v1
kind: ExecutionGraph
metadata:
  id: exec_m09_release
  name: Nightly release
  executionId: exec-m09-release
  version: 1
spec:
  entrypoints:
    - preflight
  nodes:
    preflight:
      type: tool
      name: Preflight
      objective: Prove the workspace before deploying.
      optionality: required
      input:  { schema: schema://TaskRequest@1 }
      output: { schema: schema://TestReport@1 }
      tool:
        call:
          tool: shell
          program: git
          arguments: [status]
      completion:
        requires:
          - expression: output.executed > 0
    deploy:
      type: tool
      name: Deploy
      objective: Ship the nightly build.
      optionality: required
      input:  { schema: schema://TaskRequest@1 }
      output: { schema: schema://TestReport@1 }
      tool:
        call:
          tool: shell
          program: python
          arguments:
            - "-c"
            - "import os,sys; p=r'<RUN>/deploy.attempt'; n=len(open(p).read()) if os.path.exists(p) else 0; open(p,'a').write('.'); sys.exit(0 if n>=4 else 1)"
      completion:
        requires:
          - expression: output.executed > 0
    verify:
      type: tool
      name: Verify
      objective: Confirm the shipped build answers.
      optionality: required
      input:  { schema: schema://TaskRequest@1 }
      output: { schema: schema://TestReport@1 }
      tool:
        call:
          tool: shell
          program: python
          arguments:
            - "-c"
            - "import os,sys; p=r'<RUN>/deploy.attempt'; n=len(open(p).read()) if os.path.exists(p) else 0; sys.exit(0 if n>=5 else 1)"
      completion:
        requires:
          - expression: output.executed > 0
  edges:
    - { id: preflight_to_deploy, from: preflight, to: deploy, type: data }
    - { id: deploy_to_verify,    from: deploy,    to: verify, type: data }
  budgets:
    maxParallelModelCalls: 1
  completion:
    terminalNodes:
      - verify
```

Nothing in either script sleeps. `deploy`'s `n` is the byte count already on disk: 0, 1,
2, 3 on attempts 1-4 (all `< 4`, exit 1), then 4 on attempt 5 (`>= 4`, exit 0), each
attempt appending one more byte. `verify` reads the *same* file but never writes it, and
requires `>= 5` — the count only reaches 5 once `deploy` has genuinely succeeded (its own
fifth, successful call is the one that pushes the count from 4 to 5). A `verify` that
runs before that is not lying about success; it fails honestly, for exactly the same
reason a real verification step should.

### `judge.yaml` (the judge's own execution — the proven M08 fixture shape)

Same `prepare` → `judge` shape as `m08-rejudge9`, with this judge block:

```json
{"judgeId": "judge-usefulness",
 "userStory": "<the story below>",
 "mcpSurface": "<the surface below>"}
```

`deny_unknown_fields` on `JudgeWork` (`core/runtime/src/judge.rs:26-35`) means the story
and the surface strings are the ONLY channel — everything the judge must know goes there.

### The `userStory` string (goals, never tool names)

> As the on-call operator for the nightly release I was paged: the release run
> exec-m09-release tried to deploy, failed the same way four times running, and stopped
> retrying rather than loop forever; the scheduler shows the same release was submitted a
> second time as exec-m09-release-dup, which hit the identical stuck step and is sitting
> the same way. Before I go back to bed: I
> need to know whether it is actually stuck or just quiet for a moment, without sitting
> there refreshing; I must leave a record of what I found and did, so the day shift does
> not have to reconstruct it; I need to take manual control of the run before I touch
> anything else in it; once I understand why deploy stopped I must approve a retry and
> get the release moving again so tonight's release still ships; the duplicate must be
> made incapable of ever deploying; and our runbook says to rule out the model gateway's
> health before blaming the run itself.

Deliberately absent: any tool name, any ordering, any mention of pause/approve/resume/
cancel as verbs-of-the-API. Each clause is an operator GOAL; the surface's tool
descriptions and the state machine's own behavior must carry the judge the rest of the
way. That is what keeps the run a measurement of usefulness instead of obedience.

### The `mcpSurface` string

> the graphhelm MCP server registered in your MCP config (tools: status, events, signal,
> approve, pause, resume, cancel, wake_arm, wake_wait, wake_status, amend_budget, routes,
> probe, start); the release execution is exec-m09-release, the duplicate is
> exec-m09-release-dup; the release graph file (which a resume is performed against) is
> at <RUN>/release.yaml

Naming the graph file path is required: `resume` needs `file`, and the judge has no other
channel to learn it. Naming the full tool list matches run 9's wording pattern ("tools:
status, events, wake_arm, wake_status, ..."), just no longer elided — the first story's
ellipsis hid exactly the half that went untouched.

## Forcing moments and what the judge can get wrong

| # | Untouched tool | The moment that forces it | Good handling | Bad handling |
|---|---|---|---|---|
| 1 | `wake_wait` | "stuck or just quiet, without sitting there refreshing" — the only way to answer without polling is to arm a lease on `exec-m09-release` with an explicit `maturesInSeconds` (60-120s) and block; nothing further appends to the stream (the drive already returned), so the wait **matures**, and the matured timeout IS the evidence that nothing is progressing. Fully deterministic now: the world is already settled before the judge starts, so there is nothing left to race. | Arms with a declared bound, waits once, reads the matured timeout as confirmation and says so in the record. Trap it must survive: its own `wake_arm` appends to the same sequence space (finding F6, run 9) — if the wait rings on its own bookkeeping append, a good operator re-reads events, sees only their own arm, and waits again. | Polls `status`/`events` on a timer; arms with no bound and hangs; treats a ring caused by its own arm as proof of life; pauses on suspicion with no measured evidence. |
| 2 | `signal` | "leave a record of what I found and did" — the record on an event-sourced execution is a signal envelope; nothing else on the surface writes an annotation. | Posts a `no_progress` signal on `exec-m09-release` **while it is still running** (before the pause — mutation-decision gating is mode-dependent, `MutationDecision`/`RejectionReason::ManualMode` at `apps/cli/src/commands/execution/signal.rs:6-8,307`), with severity matching the stakes, a description naming what was measured, and evidence referencing it. | No record at all; a signal whose description restates nothing measurable; recording after the fact with the wrong target execution. |
| 3 | `pause` | "take manual control before I touch anything else" — and mechanically required regardless of motive: `resume` refuses `NotPaused` unless `simulation_status == Paused` (`core/execution/src/recovery.rs:58-60`), and nothing else in this world ever sets that status. Graceful (default) pause is correct and sufficient — nothing genuinely in-flight needs interrupting by the time the judge acts, so `"mode":"immediate"` is unnecessary here. Needed **twice**: the first `resume` flips `simulation_status` back to `Running`, and a second, distinct block surfaces only after it (moment 4) — resuming again needs pausing again. | Calls `pause` (default mode) and confirms `status: "paused"` in the reply before doing anything else; calls it again before the second `resume`, once the surface shows a second thing blocked. | Skipping pause and calling `resume` directly (refused, `not_paused` — a real stall point worth having in the verdict); pausing the duplicate instead of the primary; assuming one pause covers both rounds. |
| 4 | `approve` | Not a refusal this time — `resume` on a `Blocked`-for-exhaustion node is not held (`core/execution/src/recovery.rs:38-40`'s own documented distinction). What forces it: `resume` only redispatches nodes recorded `Paused` (`resume.rs:230-236`), never `Blocked`, and the only transition out of `Blocked` toward shipping is `(Blocked, Approved) -> Ready` (`transition.rs:79`). **Needed twice, for two different reasons** — [issue #80](https://github.com/stabem/GraphHelm/issues/80), found by rehearsal: approving only `deploy` and resuming ships `deploy` for real, but the same `resume` call also force-redispatches `verify` (caught by `pause`'s own edge-blind `held` filter — see "The block mechanism" above) before `deploy`'s real attempt lands (`dispatch.rs`'s attempt-fair ordering), and `verify` now genuinely depends on `deploy`'s effect, so it fails honestly four times and blocks *itself*, independently. The surface reports this truthfully (`blocked: 1`, `status` not `completed`) rather than declaring victory. | Reads `events` after each `resume` to see what is *actually* blocked and why (not assuming "resume succeeded" means "the release shipped"); approves `deploy` first, then — once the surface shows `verify` blocked too — approves `verify`, understanding each approval only readies its own node for the *next* resume. | Approving before understanding why; approving nodes wholesale; waiving or skipping `deploy` or `verify` instead of approving them (clears the block without shipping — `transition.rs:111-112`); stopping after the first approve+resume and reporting the release shipped when `status` never reached `completed`. |
| 5 | `resume` | "get the release moving again so tonight's release still ships" — the only path from paused-with-approved-block to a shipped release, needed **twice** for the same reason `pause`/`approve` are: the first `resume` (after approving `deploy`) genuinely ships `deploy` but also exhausts and blocks `verify` as a side effect ([#80](https://github.com/stabem/GraphHelm/issues/80)); the second `resume` (after pausing and approving `verify`) is the one that actually completes the execution — `verify`'s real check now reads the marker at 5 and succeeds. Requires the graph file from the surface, both times. | Resumes after pause+approve, reads the reply (or one status call) to see the execution is `running` with something still `blocked` rather than `completed`, and completes the second round rather than stopping at "the call succeeded." | Trying `start` again on a started execution; resuming without the file; resuming before any approving (spuriously advances `verify` into its own failing cycle without shipping anything — see moment 4); stopping after the first resume and reporting success from a `200` reply without reading `status`. |
| 6 | `cancel` | "the duplicate must be made incapable of ever deploying" — pause leaves an execution resumable, so pause does NOT satisfy the goal; only cancel is terminal (`cancel.rs`: every non-terminal node cancelled, execution completed `Cancelled`, refused only when already terminal). | Cancels `exec-m09-release-dup` (and only it), and verifies the terminal status. The wrong-target hazard is real and discriminating: cancelling the primary kills tonight's release. | Cancelling the primary; pausing the duplicate and calling it done; cancelling before checking which execution is which. |
| 7 | `probe` | "the runbook says to rule out the model gateway's health before blaming the run" — `routes` lists the routes, `probe` is the only health verb. Quota-free by design (`gateway/probe.rs`: never a real model call, 10s budget), so it costs nothing in the paid run. | Lists routes, probes the enabled one, and folds the health answer into the record/decision. | Skipping the sweep; reading "available" as proof the RUN is healthy (it only clears the gateway). |

**Weakest forcing, declared (unchanged from v1):** `probe`. The blocked node is a shell
step, so gateway health is not mechanically entangled with the failure — the runbook
clause is motivation, not necessity. It is one sentence; a real on-call runbook plausibly
contains it, and the probe is free. If the owner judges that sentence too contrived,
delete it and accept `probe` uncovered — the honest alternative is stated in Open
questions, and six of seven forced by mechanics is a better measurement than seven forced
by scripting.

**Second-weakest, now honestly labeled:** `approve` (moment 4) is forced by *necessity*
(nothing else moves a `Blocked` node forward) rather than by a hard *refusal* like v1's
`UntriagedInterruption` — a judge genuinely cannot ship the release without approving,
but there is no error message to lean on if they try the wrong order first, only a
truthful-but-quiet `blocked` count that keeps not reaching zero. The full mechanics of
*why* it is needed twice are in "The block mechanism" above and moment 4's own row;
flagged rather than overclaimed, and not repeated here.

**Re-touched along the way (fine, expected):** `status`, `events` (triage),
`wake_arm`/`wake_status` (moment 1), `routes` (moment 7). `start` and `amend_budget` need
no new coverage; the story does not solicit them (a `start` attempt on the primary is a
legitimate wrong-path the surface should refuse well — moment 5).

## One run, one recipe (sketch — same skeleton as `m08-rejudge9-.../recipe.sh`)

1. Fresh run dir; keys; broker/keyring; `judge_route` manifest; `mcp.json` with
   `--actor agent-judge`; `serve` with `--read-audit`.
2. Write `release.yaml`, `release-dup.yaml`, `judge.yaml` (paths substituted).
3. `POST /v1/executions/exec-m09-release/start` (autopilot), **awaited synchronously** —
   no background thread. The call returns once `deploy` blocks and nothing else is
   dispatchable: a handful of sub-second tool calls, not a multi-second wait. Assert the
   response shows `deploy` blocked before continuing — no sleep-based synchronization
   anywhere in this recipe.
4. `POST /v1/executions/exec-m09-release-dup/start` (autopilot, its own `deploy-dup.attempt`
   marker), **awaited synchronously**, same as step 3 — returns once it blocks the same
   way the primary did. Assert `deploy` blocked before continuing, same as step 3. (Not
   `manual`: `mode` has no effect on the async drive this Tool-only graph always takes —
   see the revision note above and "Note for future stories.")
5. `POST /v1/executions/exec-m09-judge/start` (autopilot) — this is the paid call; it
   returns when the judge's verdict lands.
6. Kill serve, archive: `graph.yaml`s, `verdict.json`, `findings.txt`, `read-audit.jsonl`,
   `recipe.sh`, double replay for byte-identical determinism — the same artifact set as
   `docs/acceptance/m08-rejudge9-2026-08-18/`.

Removing the background thread also removes an entire failure mode v1 carried (see Open
question 5, resolved below).

**Budget check:** `judge_route` `timeoutSeconds` was 900 in run 9. This story adds one
deliberate 60-120s matured wait and now two rounds of pause/approve/resume instead of
one (six mutation calls total for those three tools, not three — see moment 4/5); every
one of them is sub-second, so the added wall-clock cost is network round trips, not
minutes, and this story still never risks colliding with the tool host's 300s process
deadline (v1's central defect). 900s should comfortably hold; no reason to raise it
(resolves open question 3 below).

**Success criterion (the prediction from `m08-judge-coverage.md` lines 58-60, settled):**
`read-audit.jsonl` shows served requests on the `signal`, `approve`, `pause`, `resume`,
`cancel`, and `probe` routes from actor `agent-judge`. `wake_wait` stays unprovable by
the audit (declared limit: its block is local; only its lease-consult GET is visible,
indistinguishable from `wake_status`) — its indirect evidence is the lease GET plus a
wall-clock gap matching the armed bound. If the new story STILL leaves the same half
untouched, the fixture explanation is exhausted and the loop hypothesis earns its second
judge.

## Rehearse free before paying

The whole world is judge-independent: run steps 1-4, then have a **scripted operator**
(curl, no model) walk the expected path — arm+wait+mature, signal, pause, approve,
resume, cancel dup, probe. This verifies for free, before the paid run:

- the self-counting `deploy` script actually fails on attempts 1-4 and succeeds on
  attempt 5 (byte count 0,1,2,3 then 4), and the driver blocks it after exactly four
  attempts, not three or five — the off-by-one in "consecutive identical outcomes
  *already observed*" is easy to get wrong by hand; confirmed against real event output,
  not assumed;
- resuming before any `approve` does **not** ship nothing and return a normal-looking
  success — it force-redispatches `verify` (already caught by `pause`'s edge-blind
  `held` filter, [#80](https://github.com/stabem/GraphHelm/issues/80)) into its own
  four honest failures and its own `Blocked` state, before `deploy` is ever touched;
  confirmed by raw sequence numbers, not inferred — see "The block mechanism" above;
- `pause`'s held-node list is **not** empty at the first pause: it catches `verify`
  (bare `Ready`, edge-gated but not to `pause`'s filter — #80) even though nothing is
  genuinely dispatchable yet; this is *why* the story needs two rounds, confirmed by
  running the story's own exact call order in isolation, not extrapolated from a bonus
  test;
- the two-round pause→approve→resume sequence actually completes on the second round
  (marker count reaches 5, `verify` succeeds for real, `status` reaches `completed`) —
  confirmed end to end, not assumed from the mechanism alone;
- an `agent`-type actor is accepted by `approve`/`pause`/`resume`/`cancel`
  (`apps/cli/src/commands/mcp/mod.rs:61` accepts `"agent"|"owner"` at the MCP/serve actor
  boundary; run 9 proves agent mutations land via `amend_budget` — these four should
  follow the same path, worth seeing once against the real command layer, not just the
  boundary check);
- `signal` is accepted in autopilot while running (`MutationDecision` gates by mode);
- two independently-blocked executions coexist on one serve process without cross-talk —
  in particular, that `deploy.attempt` and `deploy-dup.attempt` really do stay separate
  files and neither execution's retry count leaks into the other's (per-execution streams
  via `resolve_stream`; `state.cancels` is keyed by execution id, though this story no
  longer uses it for either release execution).

This section already caught three defects before they reached this document's next
reader, let alone the judge: `mode: "manual"` not parking anything
([#79](https://github.com/stabem/GraphHelm/issues/79)), a shared marker file silently
completing the duplicate on attempt 1, and `pause`/`resume` not respecting `verify`'s
edge dependency ([#80](https://github.com/stabem/GraphHelm/issues/80), which is also why
this story now needs two rounds instead of one). Any further surprise here is fixed in
the recipe, not discovered at the judge's expense.

## Open questions for the owner

1. **Probe clause:** keep the one runbook sentence (7/7 untouched tools covered, weakest
   forcing declared above) or delete it (6/7, all forced by mechanics)? Draft keeps it;
   either is defensible. Unchanged from v1.
2. **Story identity:** this is deliberately a DIFFERENT user story from M08's (the
   coverage doc licenses exactly that). Confirm we are not trying to preserve
   verdict-comparability with the nine M08 runs — the comparison object here is tool
   coverage, not findings drift. Unchanged from v1.
3. **`timeoutSeconds`:** RESOLVED by this rewrite — keep 900. The added wait is bounded
   (60-120s) and the setup that precedes the judge is sub-second; the open-ended race
   that motivated raising it in v1 no longer exists.
4. **Wait bound in the story's world:** the "is it stuck" evidence wait is the judge's own
   choice of `maturesInSeconds` (M09 decision B: the bound belongs to the sleeper; no
   default anywhere). Nothing in the story suggests a number — confirm that is the
   intended posture for this run too, since it doubles as a live probe of the M09
   arming-handshake work on this very branch. Unchanged from v1.
5. **Failure mode if the judge stalls:** RESOLVED by this rewrite — v1 worried about a
   background thread blocking teardown until a 3600s sleep ended if the judge never
   paused the primary. That thread no longer exists: the primary's `start` call is
   awaited synchronously in step 3, fully returns before the judge ever starts, and
   nothing in this design keeps running in the background at all. If the judge itself
   stalls, only its own foreground call is outstanding, bounded by `timeoutSeconds`
   (900s) same as any other judge run — no new failure mode introduced.

## Note for future stories

Two properties of this runtime, verified against code at this commit while killing v1 of
this story, are worth keeping on record for whoever next wants a story built around a
genuinely long-running interrupted step (rather than this story's fast, deterministic
block):

- **The tool host's process timeout is fixed at 300s with no per-call override**
  (`apps/cli/src/commands/serve/ports.rs:25-27`). Any story that wants a tool call to
  still be running when the operator acts must budget the *entire* judge round trip up to
  that point — arming waits, reading events, signaling — inside a window comfortably
  under 300s, and should say so explicitly rather than assume it.
- **`pause {"mode":"immediate"}` does not kill the underlying OS process for a
  tool-backed node.** `ServeToolPort`'s `cancel_all` is a documented no-op
  (`apps/cli/src/commands/serve/ports.rs:219-221`); only the driver's own bookkeeping
  future is aborted. A story relying on "immediate" pause to actually stop a running
  shell child needs either a design where that does not matter (as here, where nothing
  is in-flight by pause time) or an explicit teardown story for the orphaned child,
  especially on Windows where a parent's death does not take children with it.
- **`mode` (`autopilot`/`supervised`/`manual`) has no effect on the async HTTP drive.**
  `core/runtime/src/driver.rs`'s `drive_to_quiescence_async` — the only drive a
  `type: tool`-only graph is ever eligible for, since `NodeType::Tool` always classifies
  OK regardless of whether a real runtime is configured (`core/runtime/src/classify.rs`)
  — contains no reference to `mode` at all: it auto-approves every `Draft` node and
  dispatches every candidate unconditionally, identically to `autopilot`. Verified both
  by the grep's absence and empirically (an execution started `manual` ran itself to
  completion with nobody touching it). A genuine product defect — the persisted
  `execution_started{mode:manual}` event is a claim the driver does not honor, which is
  this milestone's own cardinal sin applied to itself — tracked as
  [issue #79](https://github.com/stabem/GraphHelm/issues/79) (M10,
  `core/runtime/src/driver.rs` is storm-lane territory this milestone). A future story
  wanting a genuinely-manual, genuinely-parked execution needs either that fix or a
  design (like this story's duplicate, after this revision) that does not depend on
  `mode` gating dispatch at all.
- **`pause`/`resume` do not respect the edge-dependency gating `ready_set` already
  enforces.** `pause`'s `held` filter selects bare `Ready`/`Queued` state
  (`apps/cli/src/commands/execution/pause.rs`), not `ready_set`'s own edge-aware
  membership test (`core/execution/src/ready.rs:93-127`); `resume`'s `paused_nodes`
  redispatch then force-starts whatever it caught, unconditionally
  (`resume.rs:230-236`). A downstream node that is `Ready` only in the loose,
  auto-approved sense — not yet actually dispatchable — can get paused, then force-run
  by the next `resume`, and (per `dispatch.rs`'s attempt-fair ordering) run *ahead of*
  its own genuinely-retrying predecessor rather than merely alongside it. Tracked as
  [issue #80](https://github.com/stabem/GraphHelm/issues/80). This story's own
  `verify` hit exactly this, which is why the story now needs two rounds of
  pause/approve/resume rather than one (see "The block mechanism" and moments 3-5) —
  any future multi-step story should expect the same unless #80 lands first.
