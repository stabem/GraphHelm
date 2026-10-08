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
5. Run the planner: `graphhelm keel plan --paths <p>... --promise "<text>"` (MCP `keel_plan`).
   **Not implemented yet (Phase B).** Until it lands, write the document by hand with
   `decidedBy: "rules"` and record it with `graphhelm execution signal --signal <file>` (or the MCP
   `signal` tool) on the team execution, as an `operator_note` whose description is the JSON
   document. Say in the PR body that the plan was written by hand.
6. Put the promise, paths and proof command in the PR body as the Keel card.

## Completion

Done when one plan record exists for the task (or the hand-written card, outside an execution)
and it names paths, promise, proof and skills. A plan with no proof kind is not done.
