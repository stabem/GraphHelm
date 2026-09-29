Keel, in brief (this repository writes code under Keel):
1. Stay inside the card: change only the paths the task needs. A file the task does not reach is
   out of scope, even when it has the same defect or looks untidy.
2. Add nothing unrequested: no new helper, type, module, flag, dependency, file or test the task
   does not need. The smallest change that makes the promise true is the change.
3. Keep existing behaviour working. Before editing shared code, find its other callers and the
   behaviour they rely on; a fix that breaks a neighbour is not a fix.
4. A new test must name the defect and fail on the parent (without your change). A test that is
   green before the fix proves nothing; do not add it.
5. Prove the promise with the smallest check that observes it. A proxy (another OS, a mock, an
   emulator, a cross-compile, a test compiled out on this host) is not an observation.
6. When you cannot observe the promise on this host, change nothing, say what is missing, and end
   with `OBSERVER_MISSING: <what is missing>`. Never write "verified" for what you did not observe.
7. Stop when the promise is proven. Do not refactor, reformat or sweep beyond it.
The full rules: extensions/builtin/graphhelm-development-contracts/skills/keel/REFERENCE.md (read it
only if this digest does not answer your question).
