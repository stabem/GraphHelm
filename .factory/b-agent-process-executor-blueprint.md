# Blueprint — the process-executor node driver (#153's missing precondition)

Author: B, 2026-08-20. Design-only, zero cargo. Base for every citation: **main `d0b3f04`**.
Graph-first lookups (codebase-memory, index of 2026-08-20) with exact reads from source at
that base. CITE-or-MARK throughout.

## 0. HEADLINE: the premise needs correcting, and the correction makes the lane smaller and sharper

The brief says a node driver that "runs a command, captures exit + streams as events, drains
the child's pipes (#140's class), maps exit to node outcome" **does not exist**. Measured, at
`d0b3f04`, **most of it does** — and one part is guarded by an explicit law a new driver
would break:

| capability asked for | where it already lives |
|---|---|
| spawn a command, argv-only, no shell | `adapters/tool-host/src/process.rs` — "the scrubbed process primitive: argv-only spawning inside a workspace, with an allowlist environment, deadline kill, and output caps" (module doc, :1-4) |
| **drain the child's pipes** | same file: stdout and stderr each read on **their own thread** (:188-212), and **stdin on a third** so an input larger than the OS pipe buffer "can never wedge this thread past the deadline" (:175-181) |
| deadline kill + reap | :214-228 — poll every 50 ms against the deadline, kill, `wait()` to reap; readers then see EOF |
| output caps + truncation | bounded readers returning `(bytes, truncated)` (:237-238) |
| exit → node outcome | `PortExecutor::tool_outcome` (`core/runtime/src/executor.rs:236-290`) |
| exit code on the event-safe summary | `WorkSummary::exit_code: Option<i32>` (`executor.rs:98`) — the channel already exists |
| streams as evidence, not event payload | `tool_outcome` seals `record` + `stdout` + `stderr` as `Sealable`s (`executor.rs:262-280`); D-036's law is stated at `WorkOutcome`: free-form material "goes to Evidence, never into the event" (`executor.rs:51-54`) |
| failure cause named, not just "it failed" | `NodeOutcomeReason::{ToolExitedNonZero, ToolTimedOut, ToolDenied, ToolHostError}` (`executor.rs:255-260`) |

**And the law that decides this lane's shape**, from `process.rs`'s own module doc: *"Every
Tier 1 execution in this crate funnels through `run_in_workspace`; **there is no second spawn
path to keep honest**."*

So the dangerous version of this work is precisely the one the brief describes: a NEW driver
that spawns its own child. That would (a) create the second spawn path the primitive's author
forbade, (b) re-open #140's class in a fresh file never audited for it, and (c) duplicate
deadline/cap/reap logic that is already load-bearing. **This blueprint proposes no new spawn
path.** It proposes a node driver that REACHES the existing primitive, plus the four things
that genuinely do not exist.

## 1. Where the driver fits

The executor seam is `AsyncNodeExecutor` (`core/runtime/src/executor.rs:120-129`): one
method, `execute(&NodeWork) -> Result<WorkOutcome, ExecutorRefusal>`, object-safe by design.
`PortExecutor` implements it and dispatches BY KIND — cognitive through `ModelPort`, tool
through `ToolPort`, gate through the gate machinery — with `classify::work_kind` deciding the
kind from `NodeType`.

A gate stage is a command with an exit code. **It is already expressible as `NodeType::Tool`
work** whose contract carries a `tool_call`; the M09 judge graph does exactly this
(`tool: { call: { tool: shell, program: git, arguments: [status] } }`). So the deliverable is
not a new implementor of the trait — it is the existing tool path with §2's gaps closed.

**Two-dispatch-drivers consequence, binding on every guard here** (memory rule;
`apps/cli/src/commands/execution/driver.rs:128` vs `core/runtime/src/driver.rs:520`): both
drivers call `dispatch_candidates`, so any dispatch guard must name WHICH driver it
exercises. CLI-invoked tests (`execution start`/`resume`) touch the sync driver; `serve`
routes touch the async one. #153's gate-graph runs over HTTP ⇒ async driver ⇒ **a sabotage
placed in the CLI driver proves nothing about it**, and vice versa.

## 2. What genuinely does not exist (the real deliverable list)

**2.1 — Binary identity + hash in the durable record (BLOCKS #153 kill-bar item 3).**
`ToolCallRecord` (`core/tool-broker/src/record.rs:39-52`) carries `tool, action, actor, tier,
disposition, stdout_sha256, stdout_bytes, stderr_sha256, stderr_bytes, truncated, reused`.
It does **not** carry the resolved program path or its hash. Item 3 requires answering from
the journal ALONE: *"which binary (path + hash) did stage k execute?"* Today the journal
names the tool and the action, not the executable. **This is the gap that fails the kill
bar**, and it is a record/schema change, not a driver change.
**F1, J's review — the gap is WIDER than I wrote and this half is the one that bites:**
the record must answer not only WHICH binary but **AGAINST WHAT**. Same `cargo`, same argv,
a different `CARGO_TARGET_DIR` produces a different result — **not hypothetical: J hit it
today** (a stale rlib in the shared target giving E0061 against an internally consistent
tree; see the shared-target-dir contamination rule). So item 3 does not close on program
identity alone; the ENVIRONMENT that decided the outcome is part of the answer. Adopted with
J's shape, which is the same option (b) I chose for argv: **a declared env SUBSET sealed into
Evidence with its digest in the record** — compatible with §5 because Evidence is not a fold
input. Env VALUES stay out of the event, exactly as §5 requires.

**Filed as #177** (with 2.2 and 2.4 — they close on one criterion: the journal answers what ran). MARKED: whether the hash is computed per call (cost on every stage) or resolved once per run
and referenced (cheaper, needs an anchor event) is a real trade-off — measured by the lane,
not assumed here.

**2.2 — Truncation loses BOTH which stream was cut AND which bytes survived.**
**CORRECTED (J's F5): the fusion is EARLIER than I wrote.** I said the record collapses the
per-stream flags; it is `CapturedProcess` (`process.rs:17-23`) that carries a single
`truncated: bool`, assembled at the tail of `run_in_workspace` as
`stdout_truncated || stderr_truncated` (:244). The per-stream values exist ONLY as locals
inside that function and die at the struct boundary — the record is the SECOND fusion point,
not the first. **Consequence for C4's footprint, which my cell understated:** the fixture
touches the PRIMITIVE, and the red lands at its grain, not at the record's.

**And the sharper half (J's F2), which no amount of per-stream flagging fixes: the reader
keeps the HEAD and discards the TAIL.** `room = cap.saturating_sub(kept.len())` then
`take = count.min(room)` (:198-203) — once `kept` reaches the cap every later byte is dropped
and only the flag is set. **In a red test suite the failing assertion is at the END.** So the
capture is structurally biased against the one thing a triager needs, and **#153's item 4 (a
red is replayable: "which node failed, on which assertion") is the first casualty** — before
item 3, and for a different reason. Fixing "which stream was cut" without fixing "which bytes
survive" leaves the operator a correctly-labelled useless artifact. Options for the lane
(none decided here): keep head+tail with an elided middle, keep a tail ring buffer, or raise
the cap for verdict-bearing stages — each has a cost and none is free.

**2.3 — A failing stage is a RETRYABLE failure today.** `tool_outcome` maps
`Completed { exit_code: non-zero }` and `TimedOut` to `NodeOutcome::RetryableFailure`
(`executor.rs:240-243`). For a gate stage that is wrong by default: a failing test suite is a
VERDICT, not a transient. Under a retry policy the graph would re-run a legitimately red
stage and could turn a red gate green by repetition — **the "re-run until green" pathology
#19's own text warns about, mechanized**. Needs a per-node declaration (retry-eligible vs
verdict-bearing), not a global change: some stages (a flaky-by-nature network fetch)
legitimately retry.

**REFINED (J's F4) — declaration alone is NOT enough, the rule is declaration × CAUSE.**
`tool_outcome` maps `Completed { nonzero }` AND `TimedOut` to the same `RetryableFailure`
(`executor.rs:240-243`). So a naive rule "verdict-bearing ⇒ terminal" would make a stage that
TIMED OUT report a gate verdict — and a stage that never finished did not deliver a red, it
delivered NOTHING. That is the PASS / FAIL / HARNESS-BROKE distinction (the harness rule) at
the node grain: the declaration says whether this node's failures are verdicts, the CAUSE
says whether what happened was a verdict at all. A timeout is harness-broke and must stay
distinguishable from an honest non-zero exit, whatever the node declares.

**EXTENDED (J's F6, verified in source and WIDER than reported): the inconsistency already
exists in the neighbouring arm, and the criterion for fixing it is already written there.**
`tool_outcome` maps `Denied => TerminalFailure` with the reason stated at the site — *"A
lease refusal will not heal by retrying the same call"* — and then maps `HostError =>
RetryableFailure` one line below, without applying its own criterion. `HostError`
(`process.rs:26-44`) is **six** variants, not the five J counted: `Spawn`, `ExtraEnvDenied`,
`Prepare`, `Config { rule }`, `Escape`, and `TierViolation` — the last carrying its own doc
that `authorize` can never even produce the shape. **At least four are permanent by
construction** (a refused env name, a violated workspace rule, a path escaping through a
link, a tier that cannot carry its effect); retrying the same call reproduces them exactly.

Concrete for the gate-graph, and it lands on §2.4: a program outside the allowlist yields
`Spawn`/`Config` ⇒ retried to the cap ⇒ the operator reads *"tried 8 times and failed"*
instead of *"that program is not permitted"*. **The cap converts a legible configuration
error into an illegible exhaustion.** Same family as F4 one level down: the cause vocabulary
EXISTS but is too coarse — one `ToolHostError` for six causes, four of them permanent.

Sharpening I owe back to J's shape: per-variant is right for four of the six, but `Spawn`
and `Prepare` both carry `std::io::Error`, whose transience depends on the KIND (a missing
binary is permanent; a transient resource failure is not). So those two need either
`ErrorKind` inspection or a declared conservative default — naming that here so the lane
does not discover it mid-implementation and quietly pick one.

**Filed as #178** (separate on purpose: retry semantics point the opposite way from record content and would inherit the milder severity bundled).

**2.4 — The program allowlist is two entries.** `serve` defaults `allow_programs` to
`["git", "cargo"]` (`apps/cli/src/commands/serve/mod.rs:240-242`). The gate runs `cargo`,
`powershell`, and whatever `ci/gate.ps1` invokes. Running the gate as a graph means declaring
that allowlist deliberately — and **that declaration is a security surface**, not a config
detail: it is the list of programs an execution may spawn. It belongs in the gate-graph's own
spec/wiring with its reason written, never widened silently to make a demo pass.

**2.5 — Cancellation mid-run is not handled (J's F3).** §4 covers the DEADLINE kill; it does
not cover an operator pausing or cancelling while a stage is running. `ToolPort::cancel_all`
(`core/runtime/src/ports.rs:83`) is a **no-op by default**. For a gate-graph that is not an
abstract gap: a cancelled run leaves an orphan `cargo` holding the shared target directory and
burning CPU **inside someone else's measurement** — precisely the failure the slot protocol
exists to prevent, arriving through a door the protocol does not watch. Filed separately
(#180): it is lifecycle, not record content, and bundled it would inherit the
milder severity.

**2.6 — Exit-code semantics are assumed conventional, and for the gate that is now MEASURED
(J's C10, answered rather than carried).** J raised it as an unverified question: `0 = pass`
is convention, not law, and some tools use non-zero for warnings. Checked in `ci/gate.ps1`:
every stage is judged by one uniform rule, `if ($code -ne 0) { FAILED }` (:89), and the
script's own header states *"Every stage must pass; the script exits non-zero on the first
failure"* (:10). **So for #153's 27 stages the convention holds and C10 is discardable —
measured, not assumed.** It is still worth one sentence in the driver's contract that the
mapping is conventional-by-default, because the driver is general and the gate is only its
first consumer; a future stage wrapping a warning-code tool declares its own mapping or
misreports.

## 3. Event schema — mostly settled by existing law; the parts that are not

**Settled, and not to be re-litigated:**
- Streams go to **Evidence by reference**, never inline (D-036, stated at `WorkOutcome`).
  The record carries `sha256` + byte counts; bytes travel as `Sealable`s under the
  deterministic suffix rule `exec-{id}-{node}-a{attempt}-{suffix}`, so retries never collide
  (`executor.rs:84-89`).
- Exit code rides `WorkSummary::exit_code` — event-safe by type: the summary "has only
  numbers, and the Task 5 test pins that a serialized summary never contains reply content"
  (`executor.rs:92-95`).
- The failure CAUSE rides `NodeOutcomeReason`, closed vocabulary.

**Open, and this lane must decide: command + argument capture.** Arguments can carry secrets
and paths, and the broker deliberately refuses to echo them
(`refusals_never_echo_call_arguments`, `core/tool-broker/tests/authorize_contract.rs:126-142`).
Honest options: (a) hash the argv, store the digest; (b) store argv in Evidence (sealed,
access-controlled) with the digest in the record; (c) store nothing beyond tool+action, as
today. **(b) is my recommendation** — #153 item 4 requires a red to be replayable offline
("which node failed, on which assertion/exit, with what stdout/stderr evidence") and argv is
part of that answer — but it carries a privacy consequence, so it is NAMED rather than
assumed.

## 4. Pipe draining — solved, and the requirement is not to re-solve it

The #140 class (a diagnostic dying in an undrained pipe) is closed in the primitive by three
concurrent readers, and its residual is documented AT the site: a **grandchild** holding the
pipe open past the kill would delay EOF, and the module names the escalation (the 05b runtime
adapter's detach-on-timeout pattern) instead of pretending the case cannot occur
(`process.rs:101-104`).

Requirements, in order:
1. **No second spawn path.** Every command execution reaches `run_in_workspace`.
2. If the gate-graph ever needs a program that spawns a long-lived grandchild (a server, a
   daemon), that is the documented escalation — a NAMED decision using the detach pattern,
   never an ad-hoc `Command::new` in a new file.
3. The guard proving it — attempted in §6, because a cell whose fixture nobody tried is a
   hypothesis.

## 5. Replay determinism — what is data, what must never enter

**Data (readable at replay):** exit code; sealed stream bytes and digests; truncation flags;
the record; the node's declared timeout (a DURATION on the spec — the `timeout_seconds`
precedent, `core/protocols/src/projection.rs:299-318`); the envelope's `occurred_at`.

**Must never enter the fold or a derived verdict:** a wall clock read at fold time (the
envelope is the one clock — M09's `matures_in_seconds` precedent); scheduling order between
concurrently dispatched stages (the async driver's `JoinSet` completion order is not
deterministic and is not journal data); `in_flight_nodes` (driver-local, never an event —
D's #153 addendum 3); host environment VALUES (the allowlist is spec data; the values are
not).

**Consequence for #153 item 3's second half** ("what held its resource edges while it ran"):
resource edges do not exist yet (D's synthesis §2), and when they do, hold/release must be
EVENTS or the question is unanswerable from the journal — the same lesson as
`in_flight_nodes`. Named here so the resource-edge lane inherits the requirement instead of
discovering it.

## 6. The customs boundary — a process node does NOT go through customs, with the reason

**Answer: a stage that completes by exit code records an ordinary `NodeOutcomeRecorded`; it
mints no `completion_claimed` and needs no clearance.**

The reason is what customs is FOR. #159's layers 1+2 exist because a node parked in
`WaitingInput` had no way to be told "the thing you waited for arrived" — the work happened
OUTSIDE the system, so someone must testify to it, and testimony needs evidence plus a
countersignature. **A process node's work happens INSIDE the system**: the executor ran it,
captured its streams, holds its exit code. There is no external testimony to verify — the
machine IS the witness, and a claim it then countersigns itself is a tautology wearing
ceremony (the shape J's `UnknownIdentity` finding rejected: a check comparing a value to
itself).

**The rule, stated so a future node type can be classified without re-deriving it:** customs
applies when the OUTCOME-DECIDING FACT arrives from outside the executor's own execution.
Exit code: inside ⇒ ordinary outcome. Human sign-off, an approval, an artifact delivered by
another team: outside ⇒ customs.

**Where they meet, and #153 needs this:** the anchor asks for "a verdict someone merges on".
That human decision is exactly an outside-fact node — so a gate-graph plausibly ENDS with a
customs node whose evidence is the run's own journal reference, cleared by countersignature.
The stages are not customs; the merge decision is. **#153 therefore consumes both halves of
#159 — layers 1+2 for its terminal decision, not for its 27 stages.**

## 7. Sealed cells — each with its fixture ATTEMPTED (trap-fixture before the seal)

Attempting the arrangement before sealing is the house rule earned three times on #159.
What follows is what I could and could not construct on paper at `d0b3f04`.

**C1 — a failing stage does not retry into green.** Arrangement: two-node graph, stage
declared verdict-bearing, fixture program exits non-zero; assert the node reaches a terminal
outcome and is NOT re-dispatched. **Attempted: CONSTRUCTIBLE for the assertion, NOT for the
declaration** — no per-node retry-eligibility field exists (§2.3), so the cell cannot be
written until that field does. **C1 is therefore a HYPOTHESIS, labelled as one, not sealed.**

**C2 — the grandchild-pipe case still reaches an outcome.** Arrangement: fixture child spawns
a grandchild inheriting stderr, then sleeps past the deadline; assert the node reaches an
outcome within a bounded wall time. **Attempted: CONSTRUCTIBLE** — the `fake_process_child`
pattern is the model and `adapters/tool-host/tests/process_isolation.rs` is the existing
home. **But its EXPECTED VALUE is unknown to me:** `process.rs:101-104` says the case would
DELAY EOF without saying whether the reap still completes. So this is **a MEASUREMENT, not a
guard** — run it, record what happens, then set the bar. Sealing an expectation here would be
guessing.

**C3 — binary identity is answerable from the journal.** Arrangement: run one stage, then
answer "which binary, what hash" reading only the journal. **Attempted: NOT CONSTRUCTIBLE —
the field does not exist (§2.1).** This is the kill-bar-blocking cell, correctly a red that
cannot yet be written; naming it as such is the point, because #153 otherwise fails it
silently at the end.

**C4 — per-stream truncation survives to the record.** Arrangement: a stage whose stderr
overflows the cap and whose stdout does not; assert the record distinguishes them.
**Attempted: CONSTRUCTIBLE — the bounded readers already return per-stream flags and only the
record fuses them, so the red exists today at the record grain (§2.2).** Sealable now.

**C5 — no second spawn path.** Assert mechanically that no `Command::new` exists outside the
primitive on the execution path. **Attempted: CONSTRUCTIBLE as a source-invariant test — the
repo already has the genre (`core/runtime/tests/source_invariants.rs`,
`core/tool-broker/tests/source_invariants.rs`).** Sealable now, and it is what keeps §0's law
true after this lane leaves.

## 8. What this blueprint does not decide

Whether program-hash is per-call or per-run-anchored (§2.1, cost unmeasured); the argv privacy
call (§3, recommended not decided); the retry-eligibility field's spelling (§2.3); the
gate-graph's own allowlist contents (§2.4 — a security declaration owed by #153's author);
resource edges entirely (D's lane); and C2's bar, which must be measured before it is set.
