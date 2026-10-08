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
- Records the task steps it owns, one command each, signed as your own lane (from a GraphHelm
  checkout; `docs/process/DELIVERY.md` "Task records" has the token and the defaults):
  - on taking the issue: `python tools/task-record/task_record.py --lane <you> claimed --issue <N> --branch <branch>`
  - name the issue and the PR by the standard (`docs/process/DELIVERY.md` "Naming": issue
    `<Area>: <what changes>` ≤ 50, PR `type(area): <what changes>` ≤ 60, each body starting with
    `Summary: <one sentence>`); `claimed` and `pr_opened` read them from GitHub for the Team tab
  - after `gh pr create` **and after every push to the PR** (each fix head): `python tools/task-record/task_record.py --lane <you> pr_opened --issue <N> --pr <P> --head <sha> --reviewer <reviewer>` (records the review assignment in the same call)
  - only when the reviewer changes: `python tools/task-record/task_record.py --lane <you> review_assigned --issue <N> --pr <P> --head <sha> --reviewer <reviewer>`

  A verdict counts only on a head that has a `pr_opened` record; without it the Studio shows it
  as a verdict on an unrecorded head.

  The Runtime refuses a record whose lane is not the recorder (`GHCLI038_ACTOR_MISMATCH`).

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
