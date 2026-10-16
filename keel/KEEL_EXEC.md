# KEEL_EXEC

Step 2, from an accepted card to a proven change. Read `KEEL.md` and the card first.

## Write

- Read the symbol, not the file: search for the name, then open the window around it. Open a
  whole file only when the task is the file.
- Commit structure and behaviour apart: first the moves, renames and extractions with the tests
  unchanged, then the behaviour change.
- Stay inside the card's paths. A path outside it blocks; if the card was wrong, change the card
  first and say so in the pull request.
- Reuse what the plan found. Add only what the promise needs: no speculative option, layer or
  abstraction, and no private function that only forwards to another.
- Apply every value in `KEEL.md` as you write: no duplicate, no dead code, inward dependencies,
  searchable names, no panic or swallowed error, a deadline on every external call, atomic writes,
  idempotent writes, no new slow shape, no complexity increase the card did not justify.
- Keep existing behaviour working. Before you edit shared code, find its other callers and what
  they rely on. A fix that breaks a neighbour is not a fix.
- A change that removes, replaces or redirects a production caller deletes the displaced path
  (its declarations, exclusive helpers, types, styles, dependencies and tests) in the same change
  and writes `Retirement:` in the pull request: each candidate removed, retained (name its live
  consumer or contract) or unobserved (name the gap). A zero-hit search or green tests are leads,
  not deletion proof.
- Stop when the promise is proven. Do not refactor, reformat or sweep beyond it, even when a
  neighbouring file has the same defect: name it in the pull request instead.

## Prove

1. **Choose the cheapest observer that proves the promise**: unit, integration or end to end.
   Prefer the observer closest to the user-visible promise when it is affordable:
   - a screen with an approved journey flow: `graphhelm journey replay` with the browser observer
     installed (`graphhelm setup --install-observer playwright`); a project that keeps its own Playwright
     tests: `tools/playwright-observer/playwright_observe.py --journey <contractId>`;
   - the CLI and the Runtime: the real binary against a real Runtime on a scratch store, asserting
     the reply and what was recorded;
   - hooks: the real hook process in a scratch folder, asserting what it recorded.
   Run it red on the parent and green on the change.
2. **Every new test passes the break-it-once sabotage (step 5) and the `test-audit` skill's four
   questions**, whenever it was written.
3. **The artifact.** Every end-to-end proof ends in a receipt recorded in GraphHelm, the one the
   Studio Proof tab shows: what ran, on which commit, the result, and the command that repeats it.
4. **The failure list.** A change in the last row of the `KEEL_PLAN.md` table writes down every way
   it can fail and proves each one.
5. **Break it once.** Before you call it done, break the change on purpose in one place the
   promise depends on, and show that a proof turns red. Restore it and say what you broke.
6. **Run the whole reached plan.** `tools/reached-tests/run_reached.py` on main plus the change,
   every step, never a hand-picked subset. Cite each step and its result in the pull request.
7. **Report what you saw, not what you hoped.** Passed, failed, skipped and unobserved are four
   different lines. A proxy (another OS, a mock, an emulator, a test compiled out on this host) is
   not an observation. Mock only the clock, randomness and I/O at a named boundary.
8. **No observer, no claim.** When the promise cannot be observed on this host, change nothing
   more, say what is missing and end with `OBSERVER_MISSING: <what is missing>`. Never write
   "verified" for what you did not observe.

## Hand off

Open the pull request with the card, the receipts, the broken-on-purpose result, the reached-plan
steps, every exception you wrote with its reason, and what context the card was missing.

When a rule fires, record its cause (the card, the tool, the task or the agent) before anyone
tightens anything.
