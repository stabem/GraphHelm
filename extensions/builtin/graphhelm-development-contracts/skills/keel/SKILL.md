---
name: keel
description: "Work under the Keel development model: load a contract card instead of the tree, spend a declared write surface, prove a promise against a named defect, and read the watchdog's verdict as a routing state. Use before writing code in any project that adopted GraphHelm."
---

# Keel, in brief

This digest is what loads by default; it carries every binding rule. Use only as much Keel as the
change needs: a docs or one-line change needs nothing beyond the task record.

1. Stay inside the card and search on purpose: open what it names first, then search for the
   specific symbol or caller. Change only the paths the task needs; a file the task does not reach
   is out of scope, even when it has the same defect or looks untidy.
2. Add nothing unrequested: no new helper, type, module, flag, dependency, file or test the task
   does not need. Extend an existing body or reuse a proven symbol before adding a type; a new
   interface needs a real caller (producer and consumer may land together).
3. Keep existing behaviour working. Before editing shared code, find its other callers and the
   behaviour they rely on; a fix that breaks a neighbour is not a fix.
4. A new test must name the defect and fail on the parent (without your change). A test that is
   green before the fix proves nothing; do not add it. A new or changed test states its cost (run
   time, what it needs) and passes the `test-audit` skill's four-question gate.
5. Prove the promise with the smallest check that observes it. A proxy (another OS, a mock, an
   emulator, a cross-compile, a test compiled out on this host) is not an observation.
   Report passed, failed, skipped and unobserved separately; a later green never erases an earlier red.
6. When you cannot observe the promise on this host, change nothing, say what is missing, and end
   with `OBSERVER_MISSING: <what is missing>`. Never write "verified" for what you did not observe.
7. Stop when the promise is proven. Do not refactor, reformat or sweep beyond it.
8. Removing a test needs the `test-audit` skill's deletion record naming the observer that still
   covers its obligation. No record, no removal.

Record the card and the proof where the hooks and the Studio can see them: inside a GraphHelm
execution, as `keel.card` and `keel.proof` signals threaded by `replyTo`; outside one, the card at
`.graphhelm/keel-card.json`. Shape: `docs/keel/RECORDS.md` in the GraphHelm repository.

When lanes coordinate through an execution, the step that writes an identity line also records its
`task.*` signal (`task.claimed`, `task.pr_opened`, `task.review_assigned`, `task.review_verdict`,
`task.merged`), with your own actor (`GRAPHHELM_ACTOR`) as `source.id`. The document shape is
`schemas/task-event.schema.json` in this package; the steps are in `docs/process/DELIVERY.md`,
"Task records". The Runtime refuses a task record signed by another actor.

When the change touches a screen of a journey (a path listed in a step's `screen.scopePaths` in
`.graphhelm/journeys/<contractId>.json`), name the journey in the card (`journeys: [contractId]`,
or a `Journeys:` line in the PR body). With `--events`, `--execution`, `--keyring` and `--key-id`,
`graphhelm keel check` then warns `keel.journey.no_fresh_capture` for each touched screen with no
capture at the head. The warning is advisory and never blocks. Answer it by capturing: a `before`
capture at the base and an `after` capture at the head, linked in the PR body (`docs/process/DELIVERY.md`
in the GraphHelm repository, "Before/after captures for screens"); the Playwright observer's
`--journey <contractId>` mode records the head captures in one run. A screen left uncaptured is
reported as unobserved, not as passed.

The full rules (the card and its bounds, surface counting, the verdict and its causes, the index,
where to spend time): `REFERENCE.md` beside this file. Read it only when this digest does not
answer your question.
