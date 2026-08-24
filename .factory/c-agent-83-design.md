> **PROVENANCE: this document became the implementation of #83.**
> Provenance is the strong relation and the one grep cannot see: the derived artefact carries the
> content and never the source's name, so only the author can record it. Established against
> `gh issue list --state all` and the PRs, not from memory.

# #83 — resume atomicity against drive()'s setup failure

**Base: main `0f4e7fe`.** Every file:line is re-derived against that commit, not carried from the
M09 branch where I first read the path (`cf031aa`). Verified per file with `git diff --quiet` and a
fresh `git show 0f4e7fe:… | grep -n`; byte-identical, numbers coincide. Right-by-luck and
right-by-check are different states and only one is reviewable.

**v2** — folds B's frozen review (freeze `a72214ac…7e41f`, derived blind; design v1 `bffa81df…`).
Deltas and their provenance are recorded at the end rather than silently absorbed.

## The invariant — THREE classes, not one

v1 stated a single invariant: `ok:false` ⟹ no `ExecutionResumed`. **That is wrong**, and B's M4
caught it. Post-fix, a genuine mid-drive failure correctly answers 500 with `ExecutionResumed`
durably committed — the resume really did happen and then the work failed. The v1 guard would fire
on correct behaviour: a false-red generator, live the first time anyone writes a mid-drive test.

| class | trigger | store | response must |
|---|---|---|---|
| **(a)** | refusal (preconditions) or **setup** failure | **no `ExecutionResumed`**, hold intact | say refused, hold stands |
| **(b)** | mid-drive failure (`drive_to_quiescence_async`) after healthy setup | `ExecutionResumed` durable, work attempted | **say so — distinguishably from (a)** |
| **(c)** | crash between decision and drive | decision stands | triage recovers on next resume |

**Class (a)+(b) must be distinguishable in the response.** Today both answer
`GHCLI016_DRIVER_FAILURE`. A single code covering both is the issue's own two-facts split reborn one
layer up — and it is the identical shape as #81's `InvalidRestore`, where an elapsed timeout and a
corrupt archive arrive as one value with opposite operator responses. Same defect, different
subsystem, found twice in one day. **"The call failed" must carry which failure.**

Operator-facing: *"the call failed"* and *"your hold still holds"* are one fact in class (a) and
must be **different** facts in class (b) — and the operator must be able to tell which they got.

## What a refused resume DOES legitimately write

`resume.rs:176-196` appends `Interrupted` per `recovery_plan` node **before** `resume_preconditions`
runs, deliberately — the comment states it: "each gets `Interrupted -> Blocked` before anything
else, so `resume_preconditions` below can see — and refuse — any that are still untriaged."

So a refused resume **does** append. Any claim shaped *"a refused resume leaves the store
untouched"* is false and must not appear in the commit message. The assertable invariant is
specifically about `ExecutionResumed` and the hold, and the fix must keep triage inside
`execute_prepared`, pre-decision. (B's M3.)

## The defect, located

`routes.rs:677` commits; `routes.rs:685` then runs setup that can fail. `drive` (`routes.rs:809`)
has four failure points, all after the commit, all answering `GHCLI016`:

| # | step | line | pure? |
|---|---|---|---|
| 1 | `build_sealer(state.sealing…)` | `815` | **pure** — validates `GRAPHHELM_EVENTS_KEY` is 64 hex |
| 2 | `ServeModelPort::build(wiring).await` | `823` | **NOT pure on `DirectApi`** — takes a credential lease |
| 3 | project resolution (`"project"` or cwd) | `826-835` | **pure** |
| 4 | `ServeToolPort::build(wiring, &project)` | `836` | **pure** — `WorkspaceConfig::validated` + `ToolHost::new` |

Step 1 is the issue's observation 2 (missing sealing key).

**No fifth failure point** — walked `:836` to the drive call at `0f4e7fe` and independently by B:
`ToolLease` literal, `FixtureExecutor::new`, `watch::channel`, cancel-map insert, `suite_digest` are
all infallible. The next fallible thing IS `drive_to_quiescence_async` — which is class (b) **by
definition and must not be rolled back**. That is why the (a)/(b) response split is load-bearing
rather than cosmetic.

The hazard was already written down and left unnamed: `drive`'s doc comment at `routes.rs:798` opens
"Runs the async drive for a `PreparedDrive` the decision half (`execute_prepared`) **already
committed**." That sentence belongs in the fix commit — the ordering was documented; what was
missing is that the second half can refuse after the first is durable.

## Scope: the issue and the fix boundary are different things

v1 posed this as resume-only vs widen-the-issue. **Both were wrong** — B's frozen answer is a third
framing and it is better:

- **Issue scope stays `resume`.** That is the operator story and the primary guard.
- **The fix boundary is the shared route/`drive()` shape.** The hoist lands once and fixes
  `start` (`routes.rs:277`→`286`, identical ordering) for free. Scoping the *diff* to resume would
  mean artificially not-sharing code.
- **Therefore `start` gets its own thin guard** — same arrangement, `ExecutionStarted` grain. A
  route fixed for free but left unguarded is #81's lesson in new clothes: the defect belongs to the
  shape, and an unguarded site is where it grows back.

Severity still differs and the commit should say so: `resume` drops an **operator's hold**; `start`
leaves a started-but-not-running execution. Incident case vs consistency defect.

## Fix shapes

**(c) Split the fallible from the effectful — CHOSEN.** Hoist steps 1, 3, 4 (pure) before the
commit; handle step 2's lease explicitly. B adopted this over their own frozen pick after my lease
finding.

**Step 2, the lease** (B's Q2 framework):
- **Validation** failures (credential_ref unresolvable, route unknown) are pure checks → hoist
  unconditionally.
- **Acquisition** failures (broker busy/exhausted) are the crux. Leaving acquisition post-commit
  recreates the defect for exactly that class → acquisition goes **pre-commit with
  release-on-refusal**, via a drop guard so no refusal path can leak it.
- The brief lease-on-a-refused-call is **accepted and named**, not hidden. If the broker exposes
  lease state, guard "a refused resume leaves no live lease"; if it does not, the acceptance is a
  **reading** and says so.
- `Transport::NativeRuntime` is pure, so this only bites the direct-api configuration.

**(b) Transactional rollback — REFUSED BY BOTH, INDEPENDENTLY.** Same verdict from a blind freeze
and from my own read, on four distinct grounds. Recording all four, because convergence on a verdict
via different reasons is stronger evidence than either reason twice:

1. The store is append-only and hash-chained; compensation means the journal records a resume that
   then un-happened.
2. The hold's meaning during the gap becomes a new, unanswered question.
3. *(B)* A compensation is a **system** act un-doing an **owner** act, inverting the split
   `resume.rs:246-249` documents explicitly — "the resume decision and its `Started` redispatches
   are the owner's acts; the drive that follows … stays under the system actor, so the log can tell
   sovereignty from machinery."
4. *(B)* A crash between commit and compensation leaves exactly today's half-state, so (b) buys the
   invariant only **probabilistically**.

**(a) Hoist all four** — superseded by (c); it ships leases on refused calls with no lifetime story.

## What this fix does NOT make atomic

`resume.rs:230-243` redispatches the held nodes in a **per-node append loop**, not one batch. A
crash mid-loop leaves `ExecutionResumed` committed with some nodes still `Paused`. **Pre-existing
and out of scope here**, but it means the commit must not claim "the decision is atomic". The claim
is scoped to: *a setup failure cannot commit the decision.* (B's M5. Batching the appends is a
separate issue; naming it rather than silently inheriting the overclaim.)

## The red

A resume whose **drive-setup fails**, asserting the hold **survives**:

- Arrange: execution paused with a held node.
- Inject: `build_sealer` failure — the issue's own observation 2, a pure env-var read, so the first
  red needs no failpoint machinery.
- Assert, finest grain, **on the journal not the projection** (status is a projection and can agree
  for the wrong reason):
  1. no `ExecutionResumed` for that call;
  2. the held node still `Paused`;
  3. **a second resume is ACCEPTED — no `not_paused`.** This is the judge's exact recorded symptom,
     asserted as an absence. v1's red never asked it, and it is the operator-visible half of the
     invariant. (B's S1.)
- Expected today: **FAILS** — `ExecutionResumed` is present. The red must land on *that* assertion,
  named by panic site. A red upstream (request helper, fixture) proves the arrangement broke, not
  that the guard sees.

## Sabotages owed — one per blade, each red at its own assertion, named by panic site

1. Re-order back to commit-then-setup → guard falls.
2. Assert on rendered status instead of the journal → must still fall (proves the journal assertion
   is the blade, not decoration riding on the projection). *Mine; not in B's freeze.*
3. Setup failure from something other than the sealer → must still fall (guard is about ordering,
   not one injectable).
4. **Idempotency (B's S4).** A resume with key K failing at setup commits nothing, so the **same key
   K retried must execute FRESH** — not replay, not conflict. This is the operator's actual retry
   loop; it becomes correct silently under the fix and would regress silently without a guard.
   Sabotage: restore commit-before-setup → same-key retry answers `not_paused`/conflict → falls.
5. **Triage preserved (B's S5).** A refused resume with a `recovery_plan` node still appends
   `Interrupted` → falls if the fix moves triage out of its pre-decision position.
6. Positive control: setup that **succeeds** still resumes normally and lands `ExecutionResumed` —
   without it the guard passes trivially if resume stops working entirely.
7. `start`'s thin guard, same arrangement at `ExecutionStarted` grain.

Predicted casualties sealed before any run, with an UNINFORMATIVE cell written in advance.

## Review deltas folded (provenance)

B's five sealed guesses at what v1 would miss: **4 hit, 1 wrong.**

- **M1 wrong** — v1 *did* find the `start` route, with a severity split B's freeze lacked.
- **M2 hit** → sabotage 4 (idempotent retry).
- **M3 hit (partial)** → the triage section; v1's invariant was compatible, its prose would not have been.
- **M4 hit** → the three-class invariant. The most consequential single correction: v1's guard would
  have fired on correct behaviour.
- **M5 hit** → the scoped atomicity claim.
- **Q1** → issue-scope ≠ fix-boundary, plus `start`'s own guard.
- **Q2** → the lease validation/acquisition split.
- **Q3** → confirmed no fifth failure point, which is *why* (a)/(b) must be distinguishable.

Where v1 beat the freeze: **the lease is a side effect, not a cost** — B had `ServeModelPort::build`
classified as "one wasted port build", which would have shipped leases on refused calls with no
lifetime story. Also the `:798` doc-comment archaeology.

B's freeze file stands unedited as the record of what was derived blind.
