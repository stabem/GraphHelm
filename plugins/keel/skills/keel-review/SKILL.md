---
name: keel-review
description: "Use when reviewing someone else's change under Keel as its single blind reviewer: what to read, what to run and how to give the verdict."
---

How to use this skill:

- First run: if the project has no `KEEL.md` at its root, install Keel into it with
  `python <this plugin>/setup/keel_setup.py <project root>` (the plugin folder is two levels above
  this skill's folder). It copies `KEEL.md` and the `keel/` step files into the project, offline. Run
  again later, it replaces a Keel file the project never edited (one equal to a past bundled version)
  with the bundled copy and says `updated <name>`; a file the project changed, or one newer than this
  plugin knows, is kept. It adds a marked pointer line to `CLAUDE.md` and `AGENTS.md`. It also sets
  `advisorModel` in the project's `.claude/settings.json` and keeps any value already there; pass
  `--no-advisor` to skip that write, or `--advisor <model>` to choose the model.
- Read the project's own `KEEL.md` at its root when one exists, and the `keel/` step files it names.
  Before that step has run, read https://github.com/stabem/GraphHelm/blob/main/KEEL.md and the step
  files under https://github.com/stabem/GraphHelm/blob/main/keel/. The text below is the bundled copy.
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

Step 3: one reviewer per pull request. Read [`KEEL.md`](https://github.com/stabem/GraphHelm/blob/main/KEEL.md) first.

## This file rules, on the strongest model

This file (the `keel-review` skill) is the review rule. The GraphHelm `blind-review` skill is the
same review plus GraphHelm's `task.review_verdict` and review-activity records and a saved-body
step per head; where the two differ, this file wins. The review runs on the
strongest model (`KEEL.md`, "Model roles"): the `keel` plugin warns, never blocks, when a review
skill loads in a session on a middle or cheap model. A session so warned hands the review to
`keel:keel-reviewer` or tells the owner.

## Blind

You did not write the change, and your brief carries only the PR number, the identity line to
sign with, where the rules live, the build directory, where the parent's run logs are (see below)
and questions that apply to every review (correctness, security, fail-closed behaviour, test
quality). If you helped write it, or your brief carries the author's reasoning, a finding to look
for, a summary of the change or an expected verdict, you are not blind: say so and do not review.

## Who runs what (#2421)

When a blind subagent reviews, the session that launches it runs the long work first, on the
pinned head: any required build and proportional test runs, each to its end, with every log saved
to a file. That long work includes every run the steps below ask of the reviewer: the reached
receipt check and any risk-selected tests, Keel check or journey replay. In this split,
"run" in those steps means the parent runs it and the subagent reads and judges its log. Only then does it launch the subagent. The subagent reads the diff, those logs and the
rules, judges the logs itself, and runs only short checks: the break-once, and single tests or
probes that finish within 5 minutes. It never waits on or polls a command for more than 5
minutes; a check that needs longer goes back to the parent or is reported as not observed. A
subagent waiting on a long run rewrites its expired cache on every gap (#2421 measured 66% of the
review subagents' cache writes there).

## Do

1. **Read the earlier comments** and answer any open block on this head.
2. **Reuse the author's exact-head receipt.** Check its head, scope, commands and outcomes.
   Do not rerun valid author checks or rebuild the same head merely for review. Run only one or
   two tests justified by the risk by default. If the diff touches persistence, permissions,
   compatibility or security, the reviewer reruns the reached scope: the tests those paths reach,
   not the whole plan. Missing, stale or failed evidence requires scoped repair,
   never an automatic whole-plan run. Name reused and rerun commands with their results and
   populations. DELIVERY.md §3 defines proportional scope and the nightly full-main run.
   **Await the checkout before any proof.** A checkout, fetch or reset that returns a live or
   background session is not finished: wait for it to exit, then read `git rev-parse HEAD` and an
   empty `git status --short`, and pass that full sha to the runner as `--head`. A run started
   while the head is still moving ends `incomplete` ("HEAD changed while running") and is not
   evidence; rerun it with a new `--output` (#2030).
   **Run the Keel check with the card.** Save the pull request body to a file and pass it as
   `--card` to `graphhelm keel check`; without it the check cannot measure scope. A body whose
   paths are derived from the diff ("the N files in the diff"), or an over-budget warning with no
   body line saying who decided not to split and why, is a finding. A body the check cannot parse
   (input error) is a BLOCK that asks only for the body to be fixed; the head does not change. The
   reviewer never writes or restates the card.
   **A project whose card is a JSON file** passes the JSON card as `--card` and the saved body as
   `--body`: `graphhelm keel check --diff <base>..<head> --card <card>.json --body <body>.md`. A
   blocking `keel.card.body_unread` means the body rules (identity line, skills line, declared
   skills) did not run, and the check does not pass: rerun with `--body` before approving.
3. **List only the values in `KEEL.md` that do not hold** on the diff, plus every value the
   card's risk row names, whether it holds or not. A value marked `to build` is checked by reading.
4. **Check every exception** the change adds: is the reason true, and does it expire? Refuse one
   that excuses a value instead of a constraint.
5. **Measure any promised gain live**, before and after, under the same condition, enough runs to
   see the noise. A single count is not a measurement.
6. **Check the advisor line.** The PR body carries exactly one `Advisor:` line in one of the three
   forms `KEEL.md` defines (Model roles). A missing or malformed line is a finding. Your own
   verdict comment carries your line too. Check the `Skills:` line next to it: the PR body carries
   one line `Skills: <the skills loaded, in order>`, and the author's step skill is among them. A
   missing line is a finding. Your own verdict comment carries your `Skills:` line too, listing the
   skills loaded up to that comment; compare each `Skills:` line with the Skill calls made up to
   that point.
   **Identity line.** `keel.card.identity_line_missing` from the Keel check means the body has no
   filled identity line (the form the brief gave), absent or still a template. Read through
   `--body` it blocks, and the reviewer asks the author to add the line before approving. For a
   markdown card given without `--body` it is a warning, because a card file that is not a body
   cannot carry one.
   **Skills line.** A warning `keel.card.skills_line_missing` means the body has no filled
   `Skills:` line (or `Skills usadas` heading with a name under it). Same reading: for a pull request
   body it is a finding, and the reviewer clears it before approving.
7. **Try to break the promise** a user would see, not only the tests.
   **Reach of each new rule.** For every rule, check or branch the change adds inside a loop,
   dispatcher or match, name the inputs it must see and every earlier exit (`continue`, `return`,
   `break`, an earlier match arm or filter) that can skip some of them. Name one input the rule
   never sees, or show that none exists. A skipped input the promise covers is a block.
8. **Remove your own review checkout** when you are done with `graphhelm workspace remove <dir>`
   (#1663). It unlinks every junction without following it before anything is deleted. Never
   `rm -rf`, `Remove-Item -Recurse` or `git worktree remove --force` a tree that may hold a
   junction: they follow `apps/studio/node_modules` into the owner checkout and delete it.

## What the screen says (`keel.semantic`)

For every screen the diff touches, and every screen that shows a value the diff changes, answer
each line from what you read on the rendered screen, not from what the code intends. Write a line
you could not observe as unobserved, with what is missing.

1. **Declared unit is the used unit.** A field labelled "unit price", "total" or "per m²" is
   stored and summed the way its label says; the visible label and the accessible name say the
   same thing.
2. **Same value, same text.** The same amount shows the same string, cents included, on every
   screen, bar label, receipt and message. One formatter, not one per page.
3. **Visibility follows state.** Name each state the subject can be in and what each screen shows
   for it: a finished item leaves "upcoming" lists; a record the user entered stays visible in every
   later state.
4. **Honest label.** No adjective the data does not carry (with two prices, the dearer one is not
   "top of the range"); a capped bar still says the real percentage.
5. **Exact copy at the boundaries.** Relative dates and plurals are read at 0, 1, 2, 6, 7 and 14,
   under a fixed clock.
6. **The rule before the failure.** A limit, minimum or required field is said before submit, and
   the failure names the rule, never a generic "could not save".

## Verdict

`APPROVE`, `APPROVE-WITH-RISK` (name each risk and what is still unobserved) or `BLOCK` (name the
rule id or the broken promise, and what would clear it). Start with the identity line. The verdict
is the comment text.

## What comes next

| After this | Next skill | Who acts |
|---|---|---|
| APPROVE or APPROVE-WITH-RISK on the head | keel-merge | the approving reviewer merges; the author never does |
| BLOCK on the head | keel-exec on the same branch, then a new review on the new head | author |
| Journey flow waits for approval | keel-journey-qa | journey QA agent |

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
