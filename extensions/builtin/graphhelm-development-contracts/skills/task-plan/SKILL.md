---
name: task-plan
description: "First step of a task after its issue: decide the paths, promise, proof kind (journey, tests, both, none), review count and skills for the change, and record them as one keel.plan record. Use before writing any code for an issue."
---

# Task plan

Step 1 of the per-task graph (issue, **plan**, implement, prove, review, merge). The plan decides
how much process the change needs. It never writes code.

## Emits and reads

- Emits one `keel.plan` record: a signal whose description is a `graphhelm-task-plan-v1`
  document (`docs/specs/2026-10-07-journey-first-keel-design.md` §6). Fields: `schema`, `taskId`,
  `revision`, `paths`, `classes`, `journeys`, `proof` (`journey` | `tests` | `both` | `none`),
  `reviews` (0, 1 or 2), `skills`, `tools`, `delegation`, `decidedBy`.
- Reads: the issue, `.graphhelm/journeys/` (which screens the paths touch), the repository's
  `AGENTS.md` and `docs/process/DELIVERY.md` §2 (the card rows).

## Method

1. Read the issue. Write the **promise** as one user-visible sentence, and the **paths** the change
   needs (files or directories, no globs). Start from the promise and search on purpose; do not
   list the tree.
2. Pick the card row (DELIVERY.md §2): docs/config/one-line/test-only, bounded code change, new
   public surface, or persistence/permissions/compatibility/security/external effects.
3. Pick the proof:
   - a path is in some step's `screen.scopePaths` → `journey` (name the contract ids in
     `journeys`); a touched screen with no journey → add `journey-map` to `skills`;
   - an invariant the user cannot see (schema, digest, concurrency, security) → `tests`
     (add `test-audit` to `skills`);
   - both kinds of promise → `both`; docs only → `none`.
4. Reviews: 1 for every class today (owner order). `keel` is always in `skills` for code.
5. Run the shipped planner: `graphhelm keel plan --task <task-id> --paths <p>... --promise "<text>"`
   (or the MCP `keel_plan` tool). It prints a `graphhelm-task-plan-v1` plan. When recording is
   configured, pass `--events`, `--execution`, `--keyring` and `--key-id` to also append that plan
   as a `keel.plan` signal to the execution. This planner output and signal are distinct from the
   team's `task.planned` lifecycle record: use `tools/task-record/task_record.py` as documented in
   `docs/process/DELIVERY.md` to record that step. If no Runtime-backed task record is available,
   keep the plan as a card in the PR body; do not describe a hand-written card as a persisted
   record.
6. Put the promise, paths and proof command in the PR body as the Keel card.

## Design critic (`critic: design`)

`graphhelm keel plan` sets `critic` to `design` for a change that needs a full card, and `none`
otherwise. When it says `design`, the design is graded before any implementation:

1. Write the design: the promise, the paths in scope, the approach and how it will be proved.
2. Run the critic as a **blind subagent**. Its brief carries the promise, the card and the design,
   and asks for one grade from 0 to 10 with its reasons. It never carries the author's reasoning,
   a finding to look for or an expected grade; a critic given any of those is not blind.
3. Record the round: `python tools/task-record/task_record.py --lane <you> critic_verdict
   --issue <N> --round <R> --score <S> --design-ref <path> --reason "<why>"`. The verdict follows
   from the score and the round: `pass` at 8 or more, `revise` while a round is left, `exhausted`
   on round 3.
4. On `revise`, change the design for the reasons given and grade again as the next round. On
   `exhausted`, stop and hand the task to a person. Running out of rounds is never a pass.

Implementation starts only after a `pass` record.

When a graph is synthesized for the task, hand the plan to the graph builder so it decides:
`graphhelm graph synthesize --plan <plan.json>` (the `keel plan` output as printed; MCP
`synthesize` takes it as `plan`). A plan that says `design` puts the design step and the critic
step in front of the work, with the plan's own bounds, and the reply's `critic` says the plan
decided. Pass `--critic` only to overrule the plan, and say why in the PR body.

## Completion

Done when one plan record exists for the task (or the hand-written card, outside an execution)
and it names paths, promise, proof and skills. A plan with no proof kind is not done.
