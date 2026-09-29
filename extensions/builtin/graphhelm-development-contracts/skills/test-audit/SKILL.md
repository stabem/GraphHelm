---
name: test-audit
description: "Decide whether a test earns its cost under Keel: gate every new or changed test with four questions, audit an existing suite for tests that prove nothing, and prune one subsystem at a time with a deletion record per removed test. Use before adding a test, when a suite feels slow or noisy, or when asked to prune tests."
---

# Test audit

Adapted for Keel from OpenClaw's `test-audit` skill
(<https://github.com/openclaw/openclaw/blob/main/.agents/skills/test-audit/SKILL.md>) and the test
guidance in OpenClaw's `AGENTS.md`. The three-mode shape and the junk list are theirs; the
vocabulary (criterion, defect, observer, obligation, deletion record) is Keel's.

## What this skill is, and what it is not

A test is a proof instrument (Keel Law 3): it exists to kill a named defect for one criterion.
A test that cannot fail for a credible reason, or fails only when the test itself changes, costs
run time, review time and read tokens and buys no evidence. This skill decides which tests earn
their cost. It enforces nothing and edits no rules file; the reviewer reads its output.

Three modes. Pick the one the task needs.

## Mode A — Authoring gate (every new or changed test)

Before writing or changing a test, answer four questions in the PR body, one line each:

1. **Contract.** What observable behavior, invariant or independent contract does it protect?
   Name the criterion id when a card exists.
2. **Regression.** What credible change to production code makes it fail? Name the defect, not
   "if the code breaks".
3. **Gap.** Why does existing coverage not already fail on that defect? Name the nearest existing
   observer and why it misses.
4. **Seams.** Does it need a production seam (an export, a hook, a flag, a constructor parameter)
   that no real caller uses? If yes, that seam is surface spent for the test alone; prefer driving
   the real entry point.

If question 2 or 3 has no honest answer, do not write the test. Also state its **cost**: roughly how
long it runs and what it needs (a process, a network port, a platform). A test that only mirrors
the implementation of a reversible, low-impact change is skipped; the change's own observer, or
the reviewer reading the diff, is the proof.

Keep a test when it guards a public API, protocol or wire shape, config or migration, storage,
security or permission boundary, platform behavior, or a default a user relies on, and a credible
regression there would ship silently. A source-inspection test stays only when it is the cheapest
independent guard against a user-visible change.

## Mode B — Audit (read a suite, list what proves nothing)

Read the tests of one area and list every test that matches the junk list, with its location and
the pattern:

- **No assertion.** It runs code and asserts nothing, or only that it did not panic where panicking
  is not the contract.
- **Self-comparison.** It asserts the subject's output equals the subject's output, or a value the
  test built by calling the code under test.
- **Copied fixture.** The expected value was pasted from a run of the current code, so it pins
  today's bytes, not the contract.
- **Exact source grep.** It asserts that source text contains a string, where no user-visible
  behavior depends on that spelling.
- **Duplicate invocation.** Another test already drives the same path with the same inputs and
  the same assertions.
- **Test-only export keeper.** It exists only to keep an export alive that nothing but tests import.
- **Dead code only tests call.** The production function under test has no non-test caller; the
  test and the function are one deletion.
- **Mock implements the behavior.** The mock computes the answer the test then asserts; the
  subject only forwards it.
- **Negative control that passes for the wrong reason.** A "must refuse" test that passes because
  setup failed, the path was never reached, or the error came from somewhere else. Check that the
  refusal it sees is the refusal it names.

The audit's output is a list, not a diff. Deleting is Mode C.

## Mode C — Campaign (prune one subsystem at a time)

Prune one subsystem per pull request; never sweep the whole tree. Each removed test gets a
**deletion record** in the PR body:

| Field | What it says |
|---|---|
| Test | Exact path and name. |
| Detects | The failures it can catch today, or "none" with the junk-list pattern. |
| Production callers | Non-test callers of the code it drives (search, do not guess). |
| Covering observer | The observer that still covers its obligation, by path and name. A test with no obligation worth covering still names the observer that covers the contract it touched; if none exists, the test stays. |
| History | Why it was added (commit or PR), when that is findable. |
| Unlocks | Production code or seams that can go with it. |
| Risk | What could now ship unnoticed, and why that is acceptable. |
| Validation | The command that runs the covering observer, and its result. |

A test may be removed only when its record names the observer that still covers its obligation
(Keel Law 3). No record, no removal. A count reduction alone is not a quality win.

Procedure: do not edit while tests run; run the covering observers after the removal and name the
command and result; read the diff's stat so nothing outside the subsystem moved; hand off to one
review as `docs/process/DELIVERY.md` says.

## Hands off to

`keel` for the card and the proof, `code-contract` when a record reveals a missing criterion, and
`memory-curator` when a pattern here recurs.
