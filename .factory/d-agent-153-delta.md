# D — #153 read against the lanes I hold

Base: this worktree at `f06dd85`, which contains `origin/main` `cf2d417` merged. Every citation
below re-grepped against that tree, not offset from an earlier reading. **READ-ONLY — nothing here
was measured.** The slot belongs to J (#118); no cargo was run.

Input to the M11 assembly. Nothing here is a decision, and #153's bar is sealed — this note argues
about what the bar will *encounter*, never about moving it.

---

## 1. Kill-bar item 2 is unreachable on the CLI surface, and #153 names CLI parity as a precondition

The two drivers do not mean the same thing by parallelism.

**Sync driver** (`apps/cli/src/commands/execution/driver.rs:159`) — the plan is executed in a
`for node in &plan` loop that calls `executor.execute(node, 0)` and blocks on it. The limit from
`parallel_limit` only widens **how many nodes the plan may contain**; nothing runs at the same
time. This is the driver `start` and `resume` use (`start.rs:10`, `resume.rs:10` — the async
driver appears in those files only inside doc comments, `start.rs:125`).

**Async driver** (`core/runtime/src/driver.rs:590`) — `in_flight.spawn(...)` into a `JoinSet`.
Real concurrency. This is the HTTP/serve path.

So the same policy value buys plan *width* on one surface and actual *concurrency* on the other.

**What this does to #153:** item 2 (gate-graph ≤ 60% of serial wall-clock) can only be met through
the async/HTTP path. #153 also lists, as a precondition, surface parity per D-039 — *"whatever verb
starts/completes/replays the gate-graph behaves identically on CLI and HTTP"*. Those two pull
against each other with today's drivers: identical behaviour means serial on both (item 2 fails at
~100%), or the sync driver gains concurrency it does not have. Not a defect in #153 — a collision
between two of its own items, worth resolving on paper before the executor is written.

**This implicates my own change, which is why I am raising it.** #101 (merging now) hoisted the
`max_parallel` conversion into one `parallel_limit` function. That was right, and it does not touch
this. Single-sourcing a *value* does not single-source its *effect* — and the hoist now makes the
divergence look closed: a reader who greps the policy finds one implementation and one call from
each driver, and concludes parallelism is unified. The policy is. The parallelism is not. Same
two-drivers family as #80 and #123, but this time nothing is duplicated; the divergence is in what
each site does with an identical answer, which is the harder variant to grep for.

## 2. Separate finding, softer, do not bundle it with the above: an unset budget is silently serial

`parallel_limit` reads `None => 1` (`core/execution/src/dispatch.rs:41`) — serial, and stated in
that function's doc as a policy rather than a default (I wrote that line).

A gate-graph authored without `budgets.max_parallel_model_calls` therefore runs one node at a time.
It would pass item 1 (verdict agreement 10/10) perfectly, fail item 2 at ~100% of serial, and emit
no error anywhere — the graph is behaving exactly as specified.

The reason this is worth naming now rather than at measurement time is #153's own escape hatch:
item 2 *"may be re-derived BEFORE the first measured run"* if stage discovery shows less inherent
parallelism than expected. An unset budget produces precisely the observable that argument
consumes — low parallelism — sourced from an authoring omission instead of from the gate's
structure. Two different causes collapsing into one legal number, which is the failure mode where
a sealed bar gets moved for a bad reason while everyone is being honest.

**Cheap pre-code guard, no new capability required:** (a) the gate-graph spec states its budget
explicitly, and (b) the first measured run records **observed maximum concurrent nodes**, not only
wall-clock. With both, "serial because unset" and "serial because dependencies" are separable
before anyone argues about the bar.

## 3. Verified rather than assumed: item 3's concurrency evidence is already intrinsic

Events carry `occurred_at`, a persisted canonical RFC 3339 UTC timestamp
(`core/protocols/src/event.rs:106`, `core/protocols/src/persistence.rs:457`). Node overlap is
therefore reconstructible **from the journal alone** — no sidecar — which is the property item 3
demands, and it comes free rather than needing to be built.

Two caveats, both mine to state:
- **NOT MEASURED / NOT VERIFIED:** I checked the envelope, not the emitters. That each node emits
  both a start and a finish event carrying that timestamp is the thing to confirm before relying
  on this.
- Do **not** plan to read concurrency off a counter: `in_flight_nodes` is driver-local state
  (`core/runtime/src/driver.rs:439`) and is never recorded as an event. Timestamps are the
  intrinsic record; the counter is not a record at all.

## 4. Softest, a naming question, explicitly not a defect

The only parallelism knob in the spec is `max_parallel_model_calls`
(`core/protocols/src/graph.rs:64`). #153's nodes are process executions — cargo stages that make
zero model calls — so whoever authors the gate-graph sets a field named for model calls to control
compile concurrency. Legal today, and it fuses two budgets that M11 may later want apart. Product
call, no blocker, filed here separately so it cannot inherit the severity of §1.

---

## What this changes in `d-agent-m11-engine-synthesis.md`

That file closes by saying, in its own words, that it did not read the M11 proposal — it is
bottom-up from three lanes. #153 is now that frame, so the deltas:

- **#114 / #129 (deploy) drop off the critical path.** #153 needs a *process-executor* node driver;
  a deploy node is not on the route. My synthesis argued the #93 → #94 → #129 → #114 ordering hard.
  That ordering is still correct *within* that lane and was wrong as an M11 priority claim.
- **#87 c2 changes character, not position.** Still independent of everything. But the contention
  exposure I owed forward from the c1 review — the cache's critical section is O(suffix bytes)
  because the file read happens under the mutex — becomes the exposure most likely to be hit first,
  since #153 is the first workload with genuine concurrency and resource contention. Independent
  lane, newly relevant risk.
- **#101 unchanged.** Closing; needs nothing.

## What this note does not establish

- **Nothing measured.** No cargo run, no timing, no execution. All four findings are reads of code
  at a named base.
- I did not verify that a future process-executor driver would sit on the async path. It probably
  would, and if so §1's tension is #153's to resolve rather than the driver author's — but that is
  an assumption, flagged as one.
- I read #153 only, not the M11 proposal or the PRD. A conflict between this note and either of
  those should be resolved against them, not against me.
