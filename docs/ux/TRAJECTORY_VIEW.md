# Trajectory view — Studio specification

> **Status:** specification. Nothing here is implemented yet.
> **Owner surface:** Studio (front end).
> **Baseline:** the event vocabulary as of `core/protocols/src/event.rs` (31 kinds).

## The one question

A trajectory answers **"what actually happened, in what order, and why did it end that
way?"** — for one execution, at a glance, without the reader opening a raw journal.

It is the companion of the monitor's one-glance verdict. The monitor answers *do I need to
wake up*; the trajectory answers *what did it do while I slept*. If a reader has to open
`journal.jsonl` to answer either, both surfaces have failed.

## Grounding rule: a trajectory is a projection, never telemetry

The trajectory is **derived from the event store and nothing else**. It is a projection in
exactly the sense `core/events/src/projection.rs` already means: fold the stored envelopes,
render the fold.

This is not an aesthetic preference. It buys three properties no side-channel telemetry can:

1. **It replays.** The same store renders the same trajectory, byte for byte, forever.
2. **It is auditable.** Every row can show the stored envelope, its `eventHash`, and its
   `previousHash`. A reader can verify a row rather than believe it.
3. **It cannot drift from the truth the engine acted on.** Telemetry is a second account of
   the same events, and the first divergence between two accounts is invisible.

**Consequence, and it is binding:** if a fact is not in the store, the trajectory does not
show it. It shows that it is *missing*. See "The absence rule".

## Lanes

Four lanes over one shared time axis. The axis carries **only recorded instants** — no
interpolation, no smoothing, no synthetic ticks. A gap in the axis is a real gap.

| Lane | What it holds | Recorded today? |
|---|---|---|
| **Execution** | lifecycle: started, form declared, mode changed, paused, resumed, completed | **Yes** |
| **Nodes** | one span per node: state changes and the outcome that ended each attempt | **Yes** |
| **Model** | one span per model call, with the reasoning that produced it | **No — see below** |
| **Tools** | one span per tool call, with program, arguments, exit | **Partial — node granularity only** |

### Execution lane

Built from `ExecutionStarted`, `ExecutionFormDeclared`, `ExecutionModeChanged`,
`ExecutionPaused`, `ExecutionResumed`, `ExecutionCompleted`, plus the governance events that
change what the execution is allowed to do: `MutationAccepted`, `GhostNodeProposed`,
`PolicyWaiverCreated`, `GateVerdict`, `GateCertified`.

`ExecutionFormDeclared` is the lane's spine: it names the **complete node set** and each
node's **declared deadline**. That is what lets the trajectory draw a node that has not run
yet — a node absent from the timeline is then a node that never started, which is a
different and far more useful statement than a node the view simply does not know about.

### Nodes lane

One row per node id from the declared form. Each row is a sequence of attempt spans, built
from `NodeStateChanged` and `NodeOutcomeRecorded`.

A terminated span **must** render its `reason` from the closed vocabulary — all 22 of them,
including `ToolExitedNonZero`, `Timeout`, `QuotaExhausted`, `PolicyDenied`, `JudgeRefused`,
`ContextTooLarge`. The reason is the most useful token on the screen and it already exists
in the store; a view that renders "failed" without it is discarding the answer.

Retries are spans on the same row, numbered. The row header carries the attempt count and,
when the declared form supplies one, the deadline the attempts are running against.

### Model lane — not recorded today

**No event carries a model call, and none carries model reasoning.** Measured: zero
occurrences of reasoning/thinking/thought across `core/protocols/src/`.

The reference UI that prompted this document shows a Model band. We cannot show one honestly
today from anything the engine stores. Two options, and they are not equivalent:

- **Add the events.** A `ModelCallRecorded` kind carrying route, profile, token counts,
  latency and outcome would make the lane real and keep it replayable. Reasoning text, if
  ever recorded, is content and belongs behind a **content slot with evidence and a seal**,
  not inline in an envelope — it is the highest-sensitivity payload the system would hold,
  and the sealing machinery for exactly that already exists.
- **Render the lane as unknown.** Honest, available immediately, and useless on its own.

Until those events exist the lane renders as unknown. It is never omitted: a missing lane
reads as "this execution made no model calls", which would be a lie.

### Tools lane — node granularity only

A tool node's outcome says it ran and how it ended (`ToolExitedNonZero`, `ToolTimedOut`,
`ToolDenied`, `ToolHostError`). It does **not** carry the program, the arguments, or the
individual calls inside one node. The lane therefore draws one span per tool **node**, and
its label says so. Per-call spans need the same treatment as the model lane.

### Agents

An agent is not a lane. An agent is an **attribution** already carried by every envelope:
`actor` (`id` + `type`). The trajectory colours and filters rows by actor, so "what did
agent X do" is a filter over the same timeline rather than a parallel structure that could
disagree with it.

Sub-executions (a `subgraph` node) nest: the row expands into that execution's own
trajectory, loaded from its own stream.

## The absence rule

**A lane, row or field with no recorded data renders as UNKNOWN — never as empty, never as
clean.**

This is the rule this milestone paid for repeatedly, and it is the same tri-state the
attention seam already ships (`Verdict::NeedsYou` / `CanSleep` / `Unknown`). An empty list
and an evaluated-and-clear list are indistinguishable on screen unless the view is built to
distinguish them. Every guard lost this milestone was lost exactly there.

Concretely:

- An unknown lane is drawn, greyed, and labelled with *why* it is unknown ("this engine
  version records no model-call events") — not hidden.
- A node in flight whose deadline was never declared shows **not evaluated**, not "on time".
  The operator declared nothing; the view must not invent calm.
- A history written before an event kind existed shows that kind's lane as unknown for that
  execution, not as absent behaviour.

## Reading a trajectory must not change it

The wake lease is written into the same stream the operator reads. A view that arms a lease
in order to watch moves `headSequence` without moving `lastEventAt`, and head movement then
stops implying progress — the surface poisons the signal it exists to show.

**Rule:** the trajectory is read-only. It never arms, never rings, never writes. To follow a
live execution it polls the read path, and it renders **content head** — the head ignoring
`WakeLease` and `WakeLeaseConsumed` — through the single shared definition rather than a
second copy of the rule.

## Row inspector

Selecting a row opens four tabs. The first three exist in the reference UI; the fourth is
what an event-sourced engine can offer and a log viewer cannot.

| Tab | Content |
|---|---|
| **Summary** | actor, timestamps, duration, outcome, reason — rendered in words |
| **Preview** | the payload, formatted, with content slots shown as references rather than inlined |
| **Raw** | the stored envelope, verbatim |
| **Chain** | `sequence`, `eventHash`, `previousHash`, and a verify action that re-derives the hash from the stored bytes |

Content slots are **never inlined** in Preview. They are shown as a reference plus
sensitivity, and opening one is a separate, audited action — a slot may hold secrets, and a
trajectory view is precisely the screen someone screenshots.

## Header

The header carries the monitor's verdict verbatim — the same `Verdict`, from the same seam,
in the same words. Two surfaces that can disagree about whether the operator must wake up is
the defect the product promises against (PRD §8); the trajectory computes no opinion of its
own.

Beside it: execution id, mode, content head, and declared-form status (declared /
undeclared). Undeclared is a legitimate state for any history written before
`ExecutionFormDeclared` shipped, and it is labelled, not hidden.

## What this view must never do

Each of these is a finding from a blind-judge run against the current surfaces, recorded
here so the front end does not re-introduce them:

1. **Fold a failure into another bucket.** A `retryable_failure` counted as `queued` makes
   `failed` read zero, showing a clean board while a node is failing.
2. **Show a stall without saying it is a stall.** A running span with no progress events past
   its declared deadline is a stall, and the view says so — it does not leave the reader to
   compare wall-clock timestamps by hand.
3. **Render jargon without severity.** A field such as `silenceUnevaluated` printed bare
   tells the reader nothing about whether it is bookkeeping or the reason work is wedged.
4. **Make the reader chase.** If answering "what happened" takes five interactions and still
   fails, the summary is not a summary.

## Open questions

- Does the model lane ship as new events next milestone, or does the view ship with the lane
  declared unknown? The second is honest and available now; the first is what makes the view
  worth opening.
- Reasoning text as sealed content: worth the storage and the erasure obligations
  (`EvidenceErasureRequested` would apply to it), or is a structured summary enough?
- Live-follow cadence, given the read path must not write.
