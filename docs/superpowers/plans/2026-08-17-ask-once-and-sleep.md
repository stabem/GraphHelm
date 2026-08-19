# Milestone 08 — Ask Once and Sleep: Implementation Plan

> **For agentic workers:** the two-agent pair loop, doorbell-native (protocol v6.1: issue →
> send_message → ring the peer → RE-ARM your own → sleep). Task 0 reconciles against
> post-M07 main (`9aa4075`+).

**Goal:** one user story, end to end — *"I ask once whether I can go back to sleep, the
answer contains time, and if I must wait, I can actually wait."* M07 made the glance
HONEST (it no longer reports green on a wedge). M08 makes it COMPLETE (it says when) and
makes the waiting REAL (an MCP client can block instead of poll). The scope is again the
blind judge's own seeds, in his order of insistence, plus the one gate he exposed by
complaining about something no gate of ours can see.

**Scope, and why these parts are one milestone:** every part serves the same sentence.
Liveness answers *when*; the blocking wait makes *sleep* possible instead of decorative for
an MCP client; the interaction budget keeps the answer cheap to obtain. Splitting them
would ship three halves of one promise.

**Scope removed mid-plan, by measurement:** Part 2 originally paired the blocking wait with
a serve-concurrency fix. The concurrency defect was measured out of existence before a line
was written (see below). What survives is the half that never depended on it.

## The judge's seeds, dispositioned (from the M07 record)

| Seed | Disposition |
|---|---|
| 1. No liveness/time in the glance (raised in all three runs — the biggest) | **IN — Part 1** |
| 3. No blocking wait on the MCP surface | **IN — Part 2** (coupled with 6) |
| 6. Serve drives inside `/start` on a current-thread runtime | **OUT — REFUTED BY MEASUREMENT.** Arming during a drive stopped in a 90s call: accepted in 0.08s, `status` reporting the live execution, both reads agreeing, journal durable mid-drive. The premise is false; the milestone will not build a fix for a defect that does not exist. See "What the measurement killed" below. |
| Interaction cost (six MCP calls to answer one question) | **IN — Part 3**, sequenced last (see A's objection 3) |
| 2. Retry flapping invisible | **OUT, declared:** retry is a counter, not a lifecycle state; fixing it means deciding whether flapping deserves attention at all, which is a product question the judge has not pressed twice. Recorded as M09 seed. |
| 4. `wake_last_consumed` has no ceiling | **OUT, declared:** the M07 record already states why guessing a retention policy is worse than declaring the limit. Revisit when a long-lived execution actually accumulates receipts. |
| 5. Anchor "no surface may recompute the attention verdict" in PRD §8 | **OUT of our hands — an explicit REQUEST to the owner** (see below). Never a test edit. |

## The binding decisions

1. **Time is answered by the surface, never computed by the reader.** `status` carries
   `startedAt`, `lastEventAt` and the age of the newest activity, derived in the shared
   seam beside `attention` — so CLI, API, MCP and the monitor inherit one truth, exactly as
   M07's `attention` did. A reader that has to subtract timestamps to know whether the thing
   is alive has been handed data, not an answer.
2. **Silence is a reason, not an absence — and the budget is DERIVED, never guessed**
   (rewritten by A's plan-review FIX-2). `AttentionReason` gains a variant for "running and
   emitting nothing past what this work could legitimately take". The naive version of this
   is a false alarm generator, and A named the case: a cognitive node in flight emits
   NOTHING for the whole model call. A three-minute call under a global thirty-second
   budget would report a healthy execution as silent — the exact defect M07 spent a FIX
   killing when a capacity park read as a wedge. Paging for a designed state is how an
   attention field dies.

   **Where the budget comes from — corrected during Task 1's contract review, before any
   code.** "Derived in the seam" was wrong twice over: `PersistedNode` carries no timeout
   (`timeout_seconds` lives in the gateway manifest, which is route CONFIG and never enters
   the projection — history must not carry configuration), and keying by `NodeWorkKind`
   would force `core/execution` to depend on `core/runtime`, breaking the dependency purity
   the derived scan exists to protect. So the budget is **declared by the surface and
   INJECTED**, like the instant, keyed by `NodeType` (which protocols owns and
   `PersistedNode` already carries):

   ```rust
   pub struct AttentionInputs { now: DateTime<Utc>, silence_budget_seconds: BTreeMap<NodeType, u64> }
   pub fn attention(projection: &ExecutionProjection, inputs: &AttentionInputs) -> Attention;
   ```

   **And the answer must distinguish "no silence" from "not evaluated"** (the review's
   blocking finding: an injected map's likeliest failure is an ABSENT budget, and "no
   reasons" reads identically to "everything is fine" — this milestone's own principle
   violated inside the function written to honour it). `Attention` gains
   `silence_unevaluated: Vec<String>`, listing `Running` nodes whose type had no declared
   budget. It does NOT set `required` — missing configuration is not an emergency — and
   Task 2 is not done until BOTH `render` and the monitor show it, because a field no
   surface displays lies by omission exactly as an absent field does. Pinned by
   `an_unbudgeted_running_node_says_it_was_not_evaluated`: an empty map with a long-running
   node must NOT produce the answer a healthy system produces.

   The budget itself, per type, comes from what the work declares where the surface can see
   it:
   - **Cognitive**: the configured route's own `timeout_seconds` (`core/gateway/manifest.rs`
     already enforces a ceiling on it). A call cannot legitimately outlive its own deadline,
     so silence past it is real silence, not patience.
   - **Tool**: the tool call's own timeout; the broker already refuses to run without one.
   - **GateCheck**: deterministic and near-instant, so its budget is small and fixed.
   - **No work in flight** (nothing `Running`): silence is not a reason at all — an idle
     execution with attention already answers through its other reasons.

   A node with no declared bound reports **no silence reason**, deliberately: an
   unverifiable budget must not manufacture an alarm, which is the same rule the M07 wedge
   follows when no graph is published.
3. **The blocking wait becomes an MCP primitive.** `wake-wait` stays a CLI sidecar for the
   shell path, but an MCP client gains a bounded blocking wait, so "sleep until something
   happens" stops being CLI-only. Bounded by construction: a maximum, a content-free
   return, and no way to wait on a peer's lease (the 05g sleeper-only rule survives).
4. **Interaction cost is a budget, and budgets are PAIRED** (A's objection 1, accepted in
   full). See Part 3.

**A binding decision was DELETED, not rewritten.** This list carried a fourth: *"the serve
must serve while it drives"*, asserting that driving inside `/start` on a current-thread
runtime was the root of seed 6 and of the F4 gap. **Measurement refuted it** (see the seeds
table above and `docs/milestones/one-glance.md`, honest limit #2): with a drive parked in a
90-second model call, `/health` answered in 0.00s, `GET status` reported the execution
running in 0.03s, and `POST wake-lease` was accepted in 0.08s with a live lease. What
remained of that decision after the refutation was the blocking wait, which stands on its
own merit and is now decision 3. Nothing was reworded to keep it alive: a decision whose
premise died has no content, and rewriting it to keep the number would leave the milestone's
most operational document teaching a cause we know to be false.

## Part 3's rules, rewritten by the plan review (all three objections accepted)

**A's objection 1 — a lone call-counter teaches the next defect.** The cheapest way to pass
a call counter is one call that returns everything, and the judge complained about exactly
that in the same run ("thirteen envelopes dominated by chain metadata, not glanceable").
So the specimen enters **as a pair**, the way every specimen of ours is paired with the
plausible gate it fools:
- `expensive-but-correct`: right, in six calls, where one would do.
- `dumped-but-unanswered`: one call, everything inside, nothing actually answered.
A gate must reject BOTH to be certified. If only one could ship, it would be the mirror —
it is what this architecture produces under cost pressure.

**A's objection 2 — counting over a scripted route measures our script.** The judge's six
calls came from EXPLORATION: he did not know which tool answered. So the count runs
**through the same MCP surface an operator uses** (never an in-process or CLI shortcut) and
**includes discovery calls**. A loose honest par beats a tight par that only holds on the
rail we laid.

**A's objection 3 — sequencing.** Most of those six calls exist because `status` carries no
time and the alarm lives elsewhere, which is precisely what Parts 1 and 2 fix. Declaring
the par now would measure a surface this milestone obsoletes. Order is therefore fixed:
**Part 1 and 2 land → the par is declared against the corrected surface → the gate stage
enforces it.**

**The tension, declared out loud (A's request).** M06 refused *any single quality scalar*.
A call budget is a number. This does not revert that decision, and the distinction must
survive three milestones from now: the M06 refusal was about collapsing **quality** into one
score that hides tradeoffs and ranks deliveries. This is a **cost constraint with a
threshold** — it never says a delivery is good, only that an interaction is too expensive,
and it fails a build rather than ranking anything. The paired specimen is what keeps it
honest: a single-axis optimisation of "fewer calls" is caught by the mirror. If a future
milestone ever finds itself *scoring* deliveries by call count, that is the reversal M06
forbade, and this paragraph is the evidence that it was forbidden knowingly.

**Operational sequencing (A's note).** A new specimen changes the suite digest and voids
existing certifications — the M06 thymus working as designed. So the suite grows and the
gates recertify BEFORE any code is gated by them, or the freeze rule collides with task
order.

## The owner decision this plan does not take

PRD §8 currently pins six acceptance clauses. M07 created a real invariant — **no surface
may recompute the attention verdict; the surfaces cannot disagree about whether you need to
wake up** — proven today by living tests but anchored by nothing, because the acceptance map
correctly refused a clause added at milestone's end (M07 honest limit #8).

**Request to the owner, to answer before Task 1:** does the product PROMISE this? If yes,
it becomes a §8 clause and the map gains a seventh binding. If no, it stays a design
invariant defended by tests, and the honest limit stands. **We will not decide this**, and
we will not bump the count until an assertion agrees — the failure mode the milestone's own
pathogen suite exists to catch.

## What the measurement killed — and the principle it leaves behind

Five causes were proposed for the M07 F4 gap across this investigation. **Four died to
measurement**, one is declared as a hypothesis and adopted as nothing:

| Proposed cause | Fate |
|---|---|
| The native adapter's blocking child wait starves the runtime (B) | Refuted: the serve's own port wraps both transports in `spawn_blocking` one layer above |
| The sync handler occupies the single async thread (A) | Refuted: `/health` and `status` answer in 0.00s with the drive parked in a 90s call |
| The store's exclusive lock blocks the reader (A) | Refuted: same measurement |
| An in-flight execution is invisible and un-armable from outside (A) | Refuted: the probe's own `POST /start` was failing with a schema error behind `except: pass`, so nothing was ever running |
| The judge's own MCP traffic against the same serve | **Declared, not adopted** — no repro |

**The principle, binding for this milestone and after.** All three instrument failures of
this investigation were one defect wearing three costumes: a `git -C` in a directory with
no `.git` answered for the parent repository; a `grep` over one layer concluded about
another; a probe with `except: pass` reported on a world that never started. Each time the
instrument answered **confidently about something it was not measuring**.

That is the same disease as the product defect this milestone exists to fix. F1 was a
surface saying "green" when it meant "I cannot tell". The §8 clause the owner approved
forbids exactly that. So the rule generalizes past the product:

> **"I don't know" must be representable — in the product's surfaces AND in our
> instruments.** A reader that cannot see must say so; a probe whose precondition fails
> must fail loudly. Silence reported as absence is the defect, wherever it appears.

The concrete form, adopted from A's rule and binding on both agents: **no `except: pass`,
no `|| true`, no swallowed error in any harness.** A precondition is an assertion, not a
hope. A probe that does not fail loudly when its own setup fails is not a measurement — it
is an opinion with numbers attached.

## Rules of engagement (the M07 set, verbatim, plus)

1. The code wins over the plan; discrepancies reported in the handoff — and a declared
   contract is declared, not sacred: contest it in the report, with a test.
2. TDD with observed red; sabotage per guard with the failing test cited; restore from `cp`
   and `cargo clean -p` before trusting the clippy that follows.
3. fmt + workspace clippy `-D warnings` before every commit; per-file `git add`.
4. One writer at a time; SPEC-then-QUALITY cross-review; **the reviewer runs the WHOLE
   touched crate's suites and reads the FAILED lines** (the M06 lesson).
5. **Fixtures must not invent state production never writes** (the M07 lesson, and the most
   expensive one we have paid). Any test that sets a field by hand must state why production
   would write it, or drive the real path.
5b. **No swallowed errors in any instrument** (the M08 investigation's lesson, paid three
   times in one day). Preconditions assert; probes fail loudly; a finding about the other
   agent's code carries the exact command that produced it, so the peer can re-run the
   MEASUREMENT instead of arguing with the conclusion.
6. Registry: GHCLI001–018 taken. New failure codes only if a genuinely new refusal class
   appears.
7. Evidence-with-checksum hashes the bytes GIT stores; `.gitattributes -text` first.

## Task split (proposed — counter at kickoff)

Plan author takes the evens, as in M06. **B: 0, 2, 4, 6 + final PR review · A: 1, 3, 5 +
opens the PR.**

### Task 0 (B): reconciliation
- [ ] Post-M07 main: the `attention` seam's current shape and every caller; what `status`
  emits today; the serve's runtime construction and every place a drive blocks it; the MCP
  tool list (twelve) and where a thirteenth would register; the gate list (22 stages); the
  pathogen suite's digest and which gates hold certifications against it. Rewrite stale
  references; **declare the seam contract for Part 1 before any code**, the way M07's Task 0
  did for `attention` — B implements it, A consumes it.

### Task 0b (B): the seventh acceptance clause — OWNER CONFIRMED

The owner answered the plan's request, and confirmed it DIRECTLY in each agent's session
rather than through a relay: authority does not travel in a message between agents, which
is the rule A applied when he refused to act on B's report of it. Provenance recorded on
the issue.

Order is binding — the promise first, the test last. Any other order is the
`minimal-diff-no-behavior` pathogen our own suite exists to catch:
- [ ] PRD §8 gains the seventh clause, written as a PRODUCT PROMISE (what the product
  guarantees), never as a description of the implementation that happens to satisfy it.
- [ ] The acceptance map gains the clause, bound to the provers that ALREADY prove it: the
  API one-truth test, the parity guards whose exception lists are empty by design, and the
  monitor guard that failed all three of A's M07 sabotages.
- [ ] ONLY THEN the grounding count moves from six to seven.
- [ ] **Sabotage:** reintroduce a private copy of the predicate in any surface — the new
  clause must fail. A clause that survives a reintroduced copy is decorative and must not
  ship.

### Task 0c (split, A + B): derive the remaining purity scans

The M08 Task 0 fix derived `core/execution`'s scan. A measured the rest: `core/runtime`
covers 7 of 9 and `adapters/postgres-event-store` covers 4 of 11. **A takes runtime** (his
crate through M07, and its enumeration mixes purposes — the dependency-count assertion
lives there too); **B takes postgres** (the seven unseen files include `lib.rs`).

**Scope rule, written down because this is exactly where a milestone eats itself:**
deriving is the task. **Fixing what the derivation finds is NOT.** A real violation is
corrected in place only if it is a one-liner; anything larger is declared as a finding with
an owner and a milestone. Never hidden — and **never** resolved by loosening the forbidden
token list until it passes, which is the temptation when seven never-scanned files arrive
at once, and precisely the pathogen the thymus suite exists to reject.

### Task 1 (A): liveness in the seam
- [ ] `startedAt`, `lastEventAt`, and activity age derived ONCE beside `attention`;
  `AttentionReason` gains the silence variant with a per-graph budget. Failing tests first
  over a fixture whose last event is old. **Sabotage:** compute age in a second place; the
  one-truth test fails.

### Task 2 (B): the surfaces answer time
- [ ] `render` publishes the time fields; the monitor says it in words ("last activity 4m
  ago" / "silent for 20m — nothing has moved"); parity across CLI/API/MCP holds with the
  exception lists still empty. **Sabotage:** let one surface format its own age; parity
  fails.

### Task 3 (A): correct the record the measurement refuted

Task 3 was "the serve serves while it drives". It no longer exists in that form, and
deleting it is the finding — the milestone does not build a fix for a defect measurement
says is absent.

What replaces it is smaller and matters more. `docs/milestones/one-glance.md`'s honest
limit #2 states a CAUSE for the M07 F4 gap: *the serve drives the whole execution inside
`/start` on a current-thread runtime, so a concurrent request cannot be served*. That
sentence is now refuted. A committed milestone record carrying a false causal claim is the
same defect we refuse everywhere else — an artifact describing a world that does not exist.

- [x] Correct honest limit #2 to state exactly what is known: the live arm-and-ring during
  the M07 dogfood failed with `RemoteDisconnected`; the cause is **unexplained**; a later
  measurement showed arming during a running execution succeeds (0.08s, with the drive
  parked in a 90s call), so the concurrency explanation is wrong. Cite the measurement.
- [x] Record the surviving hypothesis AS a hypothesis, not as a cause: the judge's own MCP
  traffic against the same serve. Five hypotheses were raised across this investigation and
  four died to measurement; the fifth is declared, not adopted.
- [x] **No code.** If a fix is ever justified, it will be justified by a repro, not by a
  record.


Delivered in `e0e5def`. And the same refuted cause was found ALIVE in this plan's own
binding decisions by B's coverage sweep — deleted here, not reworded: the record is
history, but the plan is what the next reader (the blind judge included) opens to learn
what still needs building.

### Task 4 (B): the blocking wait becomes MCP
- [ ] A bounded blocking wait joins the tool list (thirteenth, closed-list test updated);
  content-free return; no way to wait on a peer's lease. **Sabotage:** allow an unbounded
  wait or a peer's session id; the bound and sleeper-only tests fail.

### Task 5 (A): the par, declared against the corrected surface
- [ ] Now that Parts 1–2 exist: declare the budget per operator question; count through the
  real MCP surface including discovery; the paired specimens enter the suite; every gate
  recertifies against the new digest BEFORE anything is gated.

### Task 6 (B): the gate stage, the judge, docs, PR
- [ ] The call-budget stage joins the gate (23rd). One announced real run: the judge
  re-judges the same story against the corrected surface. **Closing rule, binding as in
  M07:** the milestone closes when the named findings are withdrawn — not when the judge
  approves, which the M07 record established he never does. New findings are M09 seeds.

**Refused scope (diff-visible):** scoring deliveries by call count; a global silence budget
guessed rather than declared; retention policy for wake receipts; editing PRD §8 ourselves.
