---
name: implement
description: "Write the change a task-plan decided, under Keel: stay inside the card's paths, gather only the context the promise needs and cite it, add only what the promise needs, and keep a failing-first test only for invariants. Use after task-plan, before journey-prove."
---

# Implement

Step 2 of the per-task graph. The binding rules are the `keel` skill's; load it first. This skill
adds what to do around them.

## Emits and reads

- Reads the task's `keel.plan` record (or the card in the PR body): paths, promise, proof, skills.
- Records the Keel card as a `keel.card` signal (shape: `docs/keel/RECORDS.md`) when working in a
  GraphHelm execution; outside one, `.graphhelm/keel-card.json`.
- After `gh pr create` succeeds, records `task.pr_opened` (`pr`, `headSha`, `journeys`, `lane`).
  Record it as a signal whose `type` is the kind and whose `description` is one
  `graphhelm-task-event-v1` document, with your own actor (`GRAPHHELM_ACTOR=<lane>`): the
  Runtime refuses a lane that is not the recorder (`GHCLI038_ACTOR_MISMATCH`). Fields and
  doors: `docs/process/DELIVERY.md` "Task records".

## Method

1. Work in your own worktree (`graphhelm workspace claim --root <root> --lane <lane> --task <n>
   --repo <repo>` creates it and records the claim). Never edit another session's checkout.
2. Gather context on purpose: open what the card names, then search for the exact symbol or
   caller. Cite each fact you rely on by file and line; say what you did not read. Do not dump
   the tree into the context.
3. Follow `keel`: stay inside the paths, extend before adding, keep callers working.
4. Tests: when the plan's proof is `tests` or `both`, write the test first, see it fail on the
   parent, then fix. Run the `test-audit` gate on every new or changed test. When the proof is
   `journey`, do not add unit tests for what the journey observes; `journey-prove` (#381) proves it.
5. Run the tests the change reaches, plus the repository's lints for touched code. List each
   command and its result in the PR body (passed, failed, skipped, unobserved, separately).
6. Open the PR: summary, `Closes #N` (only issues you mean to close), the card, tests run, and
   the identity line `Session: <name> · Head: <sha8>` on every comment and commit body.

## Completion

Done when the PR is open at a committed head whose listed tests were run on that head. A claim
you did not observe ends with `OBSERVER_MISSING: <what>`.
