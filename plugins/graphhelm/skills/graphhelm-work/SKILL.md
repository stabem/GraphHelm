---
name: graphhelm-work
description: Use when a GraphHelm task is handed to you by id, or the orchestrator says "use graphhelm-work". It asks GraphHelm for the next step, loads the skill it names, and loops until the task is done.
---

# GraphHelm work (entry)

This is the one entry skill. Do not work from memory of the flow.

## Loop

1. Read the task's state with the GraphHelm MCP `tasks` tool (or `status` with its tasks block when `tasks` is not connected) and find the task in the `tasks` array. Its `next` field is `{step, skill, actor, because}`. If the task has no `next`, the Runtime is older than this skill: report `OBSERVER_MISSING` for `next` and stop; do not infer the next step from memory.
2. If `step` is `done`, the task is finished (merged, or released): stop and report done.
3. If `actor` is null or `actor.id` is not you, stop and report to the orchestrator, with `because`; do not act for someone else. A null actor on a `review` or `merge` step means the author would be the one acting: another reviewer must be assigned or approve.
4. If `skill` is null, report `OBSERVER_MISSING` and stop. Never treat a missing skill as success.
5. Load the skill named in `skill`, do that step, and record it (claim, PR, verdict, merge) with the task record tools.
6. Go back to step 1.

## Stops

- A `review` step is taken by the blind reviewer only; the author never reviews own work.
- A `merge` step is taken by the approving reviewer only.
- A `diagnose` step goes to the owner; do not guess the fix.

For the whole flow in one place, read `graphhelm-guide`.

## If in doubt

| Situation | Skill |
|---|---|
| New task, no plan yet | task-plan |
| Writing code for a claimed task | implement |
| Proving a journey | journey-prove |
| Asked to review a PR | blind-review |
| Got a BLOCK on your head | implement (same branch) |
| Approved and you are the reviewer | merge |
| Unsure what the next step is | graphhelm-work (asks GraphHelm next) |
| Unsure about the whole flow | graphhelm-guide |
