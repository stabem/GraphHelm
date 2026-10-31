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
- When replacing a user-visible surface, remove or explicitly retire the old surface in the same
  PR, or name the follow-up issue that will retire it. Include its entry points in `Retirement:`;
  keeping both surfaces reachable without a named live purpose or retirement issue is not a plan.
- Stop when the promise is proven. Do not refactor, reformat or sweep beyond it, even when a
  neighbouring file has the same defect: name it in the pull request instead.

## Prove

1. **Choose the cheapest observer that proves the promise**: integration or end to end, or a unit
   test under step 2.
   Prefer the observer closest to the user-visible promise when it is affordable:
   - a screen with an approved journey flow: `graphhelm journey replay` with the browser observer
     installed (`graphhelm setup --install-observer playwright`); a project that keeps its own Playwright
     tests: `tools/playwright-observer/playwright_observe.py --journey <contractId>`;
   - the CLI and the Runtime: the real binary against a real Runtime on a scratch store, asserting
     the reply and what was recorded;
   - hooks: the real hook process in a scratch folder, asserting what it recorded.
   Run it red on the parent and green on the change.
2. **A standalone unit test is a regression test.** Add one only when someone finds a bug: the
   smallest test that reproduces it, red before the fix and green after. The exceptions are data
   formats and digests, security (paths, links, keys, permissions) and pure algorithms, where an
   end-to-end run cannot reach every case. Every new test, whenever it was written, passes the
   break-it-once sabotage (step 5) and the `test-audit` skill's four questions.
   **Property + counterexample** is the proof kind for an obligation that reads "for every input X,
   Y holds": canonicalization, idempotency, monotonicity, round-trips, replay. Example tests and
   journeys check chosen inputs; a property checks the rule over generated ones.
   - *Evidence:* the property written as a statement (for example "hashing a canonicalized graph
     twice gives the same hash"); a generator with bounded size and a bounded case count, so the run
     is deterministic and cheap, with the failing seeds kept in the committed regression file the
     runner writes; and, when the generator found a failure, the shrunk counterexample that drove
     the fix. Pin that counterexample as a plain example test or regression file entry in the same
     change, and cite it in the pull request. A later green run never erases it, and a retry after
     it names it as the evidence delta.
   - *Example:* `core/graph/tests/canonical_hash.rs`, where a `proptest!` block checks that any UI
     coordinates leave the semantic hash unchanged.
   - *Not for:* a promise a user sees on a screen (use the journey), a single known bug (the
     smallest regression test is enough), code with effects outside the process, or a property you
     can only state by repeating the implementation. No SMT or formal solver is part of Keel; raise
     one only for a concrete obligation a generator cannot reach. Without a stated property, report
     `OBSERVER_MISSING`.
3. **The artifact.** Every end-to-end proof ends in a receipt recorded in GraphHelm, the one the
   Studio Proof tab shows: what ran, on which commit, the result, and the command that repeats it.
4. **The failure list.** A change in the last row of the `KEEL_PLAN.md` table writes down every way
   it can fail and proves each one.
5. **Break it once.** Before you call it done, break the change on purpose in one place the
   promise depends on, and show that a proof turns red. Restore it and say what you broke.
6. **Run the whole reached plan.** `tools/reached-tests/run_reached.py` on main plus the change,
   every step, never a hand-picked subset. Cite each step and its result in the pull request, and
   leave the JSON report as the receipt the reviewer reuses (`KEEL_REVIEW.md` step 2). Two steps of
   that plan are the ones authors miss, so run them even when you check by hand: every changed `.py`
   reaches `python -m unittest tools/reached-tests/test_no_console_window.py` (a spawn passes
   `creationflags=NO_WINDOW`, AGENTS.md "No console windows"), and every new test runs by the
   runner's own command from the repository root, `python -m unittest <path>`, not `discover -s` or
   pytest; a test that imports from its own folder copies the `sys.path.insert` the folder's other
   tests use.
7. **Assert what the user reads** (`keel.proof.reads_screen`, `keel.test.intent`). A journey
   over a screen checks the rendered text the promise is about: the amount with its cents, the
   exact date phrase under a fixed clock, the plural, the label, and the effect on the other
   screens that show the same subject. Take every expected string from the intent, never from
   the code's current output. A known open defect stays in the proof as an expected failure
   naming its issue, so its fix turns the proof red until the marker goes.
8. **Report what you saw, not what you hoped.** Passed, failed, skipped and unobserved are four
   different lines. A proxy (another OS, a mock, an emulator, a test compiled out on this host) is
   not an observation. Mock only the clock, randomness and I/O at a named boundary.
9. **A step that should run itself and does not gets its own issue.** A manual workaround, a stale
   record, a check no test caught or a tool that failed: search for an open issue or PR first, then
   open one at once saying what should have happened automatically, what happened, the evidence and
   the shape of the fix. Review findings stay comments on the PR.
10. **No observer, no claim.** When the promise cannot be observed on this host, change nothing
   more, say what is missing and end with `OBSERVER_MISSING: <what is missing>`. Never write
   "verified" for what you did not observe.

## Hand off

**Diff hygiene (#2251).** Before asking for review, the author runs the diff-hygiene check on the
branch head and quotes its status line in the PR body:
`python tools/diff-hygiene/diff_hygiene.py --card <file holding the PR body>`. It compares
`git diff origin/main...HEAD` with the Keel card and fails when:
1. Line endings: `git diff --numstat` differs from `git diff --numstat --ignore-cr-at-eol` for any file.
2. Stale base: a path the branch changes was also changed on origin/main after the merge-base
   (`git diff --name-only $(git merge-base origin/main HEAD) origin/main` ∩ the branch's paths).
   Fix: rebase and rerun. A merge-base behind origin/main with no overlapping path passes. An
   overlap means rebase and rerun, even when it is not a revert.
   Limit: rule 2 compares against commits after the merge-base, so it does not see a branch that
   undoes the merge-base commit itself (for example a `git reset --soft` onto an older commit).
   Rule 3 catches that only when the undone path is outside the card; inside the card it passes.
   Rebase; never rebuild a branch with `reset --soft`.
3. Scope: a changed path is outside the card's `paths` plus a short committed allow-list file
   (`tools/diff-hygiene/allow-paths.txt`: lockfiles).
   A summary-only card (docs/config/one-line) has no paths, so check 3 does not apply.

When origin/main cannot be fetched or the card cannot be read, the check reports `unobserved` with
the reason, never `passed`. It is not a merge gate: the reviewer reads the status line and may BLOCK
when it is missing or red.

Open the pull request with the card, the receipts, the broken-on-purpose result, the reached-plan
steps, every exception you wrote with its reason, and what context the card was missing.

When a rule fires, record its cause (the card, the tool, the task or the agent) before anyone
tightens anything.
