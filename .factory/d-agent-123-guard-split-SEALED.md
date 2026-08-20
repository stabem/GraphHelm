# #123 — guard split, SEALED IN A FILE BEFORE THE RUN

D, 2026-08-20, branch `issue-123-resume-edge-narrowing` off `c357a5e`. **Committed before launching
anything**, per L's file-not-message rule: the ordering is durable rather than self-reported, which
is the guarantee my last sealed run could not offer.

## The two trap guards, written BEFORE the fix

Both must be CONSTRUCTABLE. A guard whose fixture cannot be built is telling me the shape is wrong
— that instrument has already killed two shapes tonight before either reached code, and if it
kills a third I will report that rather than bend a fixture to fit.

### 1. `repeated_pause_resume_does_not_churn_a_node_whose_edges_are_unmet`

**PREDICTED RED PRE-FIX**, failing in the slope assertion (`steps.iter().all(|step| *step <= 2)`).
Measured slope today is 4 events/round; the assertion allows ≤2. It must also fail the
`deploy == "paused"` assertion, since today `deploy` ends `queued`.

FALSIFIED IF it is green pre-fix — that would mean the churn I measured on the CLI path does not
reproduce through these helpers, and the guard is measuring something else.

### 2. `a_gated_nodes_release_is_the_owners_act_and_the_drivers_own_hops_are_not`

**PREDICTED RED PRE-FIX**, failing in the TIMING assertion (`release_at > implementation_succeeded`)
— today `resume` force-`Started`s `deploy` at decision time, before the drive has run
`implementation` to `Succeeded`, so the release precedes the success that justifies it.

**NOT PREDICTED TO FAIL ON EITHER ATTRIBUTION HALF PRE-FIX, and that is the vacuity risk I am
naming rather than discovering.** Today the release is appended by `resume` itself under
`owner_actor()`, so half one ("release is owner-attributed") is already TRUE — vacuously, because
the release happens in the wrong place at the wrong time. Half two ("ordinary hop is
system-attributed") is likewise already true. **So the attribution halves cannot fail today and
only the timing half carries the red.** They earn their keep AFTER the fix, when the release moves
into the driver and could trivially inherit the system actor.

### Sabotage, required before either guard is trusted (L's condition 3)

Pass the owner's actor to EVERY driver hop (the obvious wholesale implementation). Guard 2 must go
RED on half two. If it does not, the guard passes on a build where attribution is uniformly wrong,
which is the exact failure it exists to prevent.

**PREDICTED CASUALTIES of that sabotage: guard 2 only, on the `ordinary_hop.3 == "system-cli"`
assertion.** Any other casualty is unpredicted and gets reported as such.

## What must NOT move

- The flagship story (`the_operator_story_runs_end_to_end_and_replays_byte_identical`) must stay
  green **unmodified**. It is the shape that killed the previous two proposals; if the fix requires
  editing it, the fix is wrong.
- `approve_is_not_a_dead_end_once_the_condition_is_fixed` must stay green unmodified.
- The two #80 tests that assert `deploy` ends `queued` (`:958`, `:1115` region) WILL move to
  `paused`, deliberately and with reasoning stated — they are the same tests #80 moved, moving a
  second time. The #80 rewrite condition still binds: a story must keep asserting that work RUNS,
  not merely that a state was reached.

## Confirmation runs, rule sealed here rather than declared in a message

1. **Slope, N=1 per arm**, deterministic: with the fix, the POST slope must return to **2
   events/round**. If it does not, the fix does not remove the amplifier and nothing downstream
   matters.
2. **ABAB rate, N=10 per arm**, `07b1243` vs `POST+fix`. **MOVED-BACK if the arms differ by ≤1 of
   10; NOT RESTORED if they differ by ≥6; INDETERMINATE otherwise, declared as such.** Escalate to
   N=20 only on INDETERMINATE, when the number would have a consumer.
3. Counters reported PER RUN either way, not a bare rate. **No re-rolls**; any run outside the
   sealed window is recorded as "ran, outside the window" and not counted — as run 11 was.

## Standing limits carried in

- The rate rule is **conservative by construction, not exact** (L's own correction): thresholds
  chosen for cost, not from a formal error rate.
- **Event count does not predict failure WITHIN an arm** — verified from the raw file: POST
  failures ran 53–68 events, passes 57–69, ranges fully overlapping, the highest count PASSED and
  the lowest FAILED. The counters carry mechanism at arm level; the rate carries regression.
  Neither does the other's work, and no per-run "it failed because it appended more" claim exists
  at any altitude.
