---
name: keel-review
description: "Use when reviewing someone else's change under Keel as its single blind reviewer: what to read, what to run and how to give the verdict."
---

How to use this skill:

- If the project has its own `KEEL.md` at its root, follow that file and the `keel/` files it names.
  The text below is the bundled copy, for projects that have none.
- In the text below, `keel/KEEL_PLAN.md`, `keel/KEEL_EXEC.md`, `keel/KEEL_REVIEW.md` and
  `keel/KEEL_MERGE.md` are this plugin's `keel-plan`, `keel-exec`, `keel-review` and `keel-merge`
  skills, and `KEEL.md` is its `keel` skill. `docs/keel/KEEL_SPEC.md` exists only in the GraphHelm
  repository.
- If the `graphhelm` CLI is on PATH, run `graphhelm keel check --diff <base>..<head> --card <card>`
  for the rules marked as measured. If it is not, the reviewer checks those rules by reading and says
  so in the review.
- Nothing here needs a GraphHelm Runtime, MCP server, task records or journeys. Where the text names
  them, they apply only inside GraphHelm: elsewhere, prove the promise with the project's own test
  runner.

<!-- keel source: keel/KEEL_REVIEW.md (generated; edit the source, then run tools/keel-plugin/sync_keel_plugin.py) -->

# KEEL_REVIEW

Step 3: one reviewer per pull request. Read [`KEEL.md`](../KEEL.md) first.

## Blind

You did not write the change, and your brief carries only the PR number, the identity line to
sign with, where the rules live, the build directory and questions that apply to every review
(correctness, security, fail-closed behaviour, test quality). If you helped write it, or your
brief carries the author's reasoning, a finding to look for, a summary of the change or an
expected verdict, you are not blind: say so and do not review.

## Do

1. **Read the earlier comments** and answer any open block on this head.
2. **Reuse the author's receipt, rerun what proves.** The author leaves the `run_reached.py` JSON
   report as a file path in the PR body with its summary line; it names the head (`headBefore`)
   and the base main sha (`baseSha`). Reuse it when its head equals the PR head and nothing in it
   is failed or pending. Then run the end-to-end proof of the promise, any cell you doubt, and fmt
   and clippy on the touched crates. Rerun the whole reached plan on main plus the head only when
   the receipt is missing, stale, red or pending, or when the diff touches persistence,
   permissions, security, compatibility or the plan selector itself. Name what you reused and what
   you reran, each command and its result, and the size of each population (for example, how many
   test files ran).
3. **List only the values in `KEEL.md` that do not hold** on the diff, plus every value the
   card's risk row names, whether it holds or not. A value marked `to build` is checked by reading.
4. **Check every exception** the change adds: is the reason true, and does it expire? Refuse one
   that excuses a value instead of a constraint.
5. **Measure any promised gain live**, before and after, under the same condition, enough runs to
   see the noise. A single count is not a measurement.
6. **Try to break the promise** a user would see, not only the tests.

## Verdict

`APPROVE`, `APPROVE-WITH-RISK` (name each risk and what is still unobserved) or `BLOCK` (name the
rule id or the broken promise, and what would clear it). Start with the identity line. The verdict
is the comment text.
