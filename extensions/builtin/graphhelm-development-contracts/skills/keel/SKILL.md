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

The full rules (the card and its bounds, surface counting, the verdict and its causes, the index,
where to spend time): `REFERENCE.md` beside this file. Read it only when this digest does not
answer your question.
