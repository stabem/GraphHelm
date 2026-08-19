# Milestone 09 — Arming the Alarm

**The user sentence:** *"Arming the alarm is the moment the system and I negotiate what counts
as needing me — and nothing that was not negotiated there gets promised."*

M08 made the answer honest: three values, time on the surface, a declared bound carried end to
end, and a socket to declare it mid-run. The blind judge's standing objection survived nine
runs and changed kind on the way out — it stopped claiming the mechanism fails and became an
argument about a DEFAULT: *why must an operator configure a per-node bound before the screen
will answer?* M09 answers that, and the answer moves the question off the graph entirely.

## How the shape was found, including the two attacks that changed it

Five isolated cognitive frames were run over the nine recorded seeds. The convergence and both
corrections are recorded here because the corrections are the reason the plan is not wrong:

**The first shape fused seeds 3 and 4** — "a queued run wakes nobody" and "the doorbell cannot
ring for silence" — into one mechanism. **The reviewer refused it with a line number**
(`core/execution/src/attention.rs:332`): silence is filtered to `NodeState::Running`, and
`advances_without_the_operator` counts `Queued` as advancing. Two independent places treat
"in the queue" as "moving on its own".

His risk was concrete and decides the plan: build ONE mechanism, and the expiry contract works,
the wedged `queued` node never produces an expiry because it is never evaluated, **and the test
passes** — because it would be written with a `Running` node, which is where the mechanism
lives. Green guard, the judge's case still dead, the difference invisible. That is this
project's own rule pointed at its planning: the question *which states count* and the question
*when absence becomes an event* live at different heights.

**The second shape anchored the negotiation to "an unsupervised run".** The reviewer refused
that too, with another line (`core/governor/src/inflight.rs:114`): `supervised` decides whether
a MUTATION REQUIRES APPROVAL. It says nothing about a human watching. And the product's own
premise finishes the argument — executions are LONG, so an operator "present" for a four-hour
run is not present. Anchoring a sleep contract to a flag about approval is the wrong grain
again, one level up in the plan itself.

**The anchor that survived is the one the system already has: the wake lease.** Arming a lease
is literally *"I am leaving and I want to be woken"*. It is not a proxy and not an inference —
it is the declared act, and it is already an event in the store.

## The three decisions, in an order where none can be derived from another

### A. Which states have judgeable silence — POLICY, and it comes first

Today: `Running` only, by a filter nobody can trace to a decision. It becomes a list **with a
reason per exclusion**, because a list with reasons is what prevents the next instance while a
bare `== Running` is what produced this one.

* `Queued` **after a retryable failure** is not progress by any reading, and is the judge's own
  measured case.
* `Draft` is not silence — it is work that has not begun.
* `WaitingCapacity` has a known owner and a declared park-and-wait rule (M05) — excluded **as a
  limit of this milestone**, not forever, because a quota that never returns is also silence.
  Recorded as a limit so it does not become the next `== Running` nobody can trace.

**Binding on the test:** A's guard must NOT use a `Running` node. If it can only fail with one,
it is testing the mechanism rather than the policy, and B would satisfy it while the judge's
case stays dead.

### B. Absence becomes an event with a maturity — MECHANISM, and it comes second

The doorbell is defined over appends; quiet writes nothing; so nothing rings. The run holds a
dead-man's switch it must ACTIVELY SUPPRESS: expiry writes an event, and the doorbell rings.
Absence gains the shape of presence, and a settlement date.

The maturity belongs to **the lease**, not to the graph — which is where A and B meet without
anyone forcing them together.

**Who appends the expiry — the reviewer's third attack, and the plan was silent on it.** Only
three candidates exist and two are dead on inspection: the execution itself cannot write its own
death certificate (the case worth catching is the wedged one), and a reader computing expiry on
the fly rings nothing, because the doorbell fires on APPEND. That leaves a writer in `serve` —
and the sweep that exists today is **not** it. Measured at `apps/cli/src/commands/serve/mod.rs:803`:
`wake::sweep` is `tokio::spawn`ed after a mutation route's durable append. It is
append-triggered, so in a quiet stream it never runs, which is the exact condition expiry is
for. B therefore needs a **periodic tick**, and it may not be driven by reads — a read that
moves the clock is the M08 read-side defect returning with a new face.

**Declared limit, in the same breath as the promise:** expiry requires a running `serve`. A
dead-man's switch whose watchman can also die must state that, so it sits beside *no lease
armed, nobody wakes you* rather than hiding behind it.

**Expiry is a THIRD category, not content.** `is_content` (`apps/cli/src/commands/execution/wake.rs:20`)
is a blocklist — everything that is not wake bookkeeping counts as content — so a new expiry
kind becomes "work happened" by default, and the operator reads progress where the truth is
that NOTHING happened. The reviewer's line holds: do not lie in the classification to reuse the
old ring condition. The doorbell gains a **second condition** — new content OR a matured lease
— and `is_content` keeps meaning what it says.

### C. The negotiation happens at arming — the answer to the judge's standing objection

Arming a lease stops returning a cursor and starts returning a **contract**: what will wake you,
what will not, and where each number came from.

* **Form:** a handshake at arming time — decided at 23:00, which is the only hour anyone thinks
  clearly about 03:00.
* **Content:** numbers derived from that node's own history, with provenance said in the same
  breath (*"300s, median of this node's last 12 runs"*). The product is event-sourced; using its
  own record is what it is for.
* **Floor:** where there is no history, it **refuses to arm IN SILENCE** — the arm demands the
  number from the operator, right there in the handshake. No invented default, ever; but *never
  invent* and *always refuse* are not the same rule, and the first draft of this plan treated
  them as one. The reviewer measured the cost: no history is not a rare edge, it is **day one**,
  and it returns every time the graph grows a node — landing precisely when someone is trying to
  go to sleep. The cure was already in C: the handshake is where the operator speaks.

**Why the refusal is not adoption friction:** the threshold belongs to the SLEEPER, not the
graph. A run someone is watching needs no threshold at all, and `unknown` is cheap to a person
looking at the screen. The stranger following the quickstart never arms a lease, so never meets
the floor. The friction lands only where someone is asking to be trusted while they sleep.

**Declared and deliberate:** an operator who simply closes the laptop without arming anything
gets nothing, and that is correct — *no lease armed, nobody wakes you, by design*. Guessing at
presence is the same class of invention as guessing at a budget, and this milestone refuses
both.

## Also in scope, because they are one class each

**Counters become projections.** `acceptedMutations` reading 0 after an accepted mutation, and
arming a lease inflating the head sequence, are the same defect: numbers living OUTSIDE the
log. As projections, "read" and "change" are different verbs by construction rather than by
discipline.

**`graphhelm doctor`.** Two journals committed under `docs/acceptance/` fail integrity
verification and failed at the parent commit. They are cited as proof. This is not debt to pay
quietly — it is the product's thesis applied to its own repository, and the only artifact a
stranger can evaluate without trusting us.

## Not in scope, declared rather than omitted

**A second blind judge.** The first one is inside the loop now — we consult, measure, audit and
answer him. The reviewer's correction stands: the property that matters is not being ANOTHER,
it is being incapable of being optimised by us, and the read-audit already has that property
because served bytes have no opinion. Before hiring a second, **measure what the first stopped
catching by being inside** — that is answerable from the nine recorded runs and costs nothing.

## Closing condition, stated as a PROHIBITION

The milestone closes when the named findings are withdrawn by transcript — and additionally:

**No green run may contain a skipped, retried or quarantined test on any path a seed touches.**
A flake is a failure with better manners, and seed 9 sits on the exact path decision B attacks,
so any "it passes" there is unfalsifiable until the flake is dead. This makes the flake blocking
by consequence rather than by decree.
