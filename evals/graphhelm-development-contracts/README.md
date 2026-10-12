# Evals for the development-contracts skills

These cases live outside the extension package on purpose: the package inventory is strict and
refuses any top-level entry it does not know (`GHEX012_INVENTORY`), so an `evals/` directory inside
the shipped package would make it fail validation. The eval runner reads cases from the plugin
directory, so run it against a scratch copy. From the repository root:

```sh
tmp="$(mktemp -d)"
cp -r extensions/builtin/graphhelm-development-contracts "$tmp/plugin"
cp -r evals/graphhelm-development-contracts "$tmp/plugin/evals"
claude plugin eval "$tmp/plugin" --trust-plugin
```

Each case is `prompt.md` plus `graders/criteria.md` (an LLM judge). The runner adds a no-plugin
baseline arm, so the delta column shows what the skill itself changes.

`judge-qualification.json` version 2 is a public, synthetic review packet for the current
`keel-observer-missing` and `test-audit-junk` prompts and rubrics. Each rubric has a proposed
correct, plausible-wrong, and boundary response. The original observer examples and their
proposed labels are unchanged. All labels are pending human review; they are not judge results
or secret holdout data. Reviewers must approve the labels before using them to qualify any
judge. Keep skill invocation, answer correctness, and completion/infrastructure status
separate; an incomplete response cannot count as a pass.

The packet's `contentSha256` binds its complete parsed content, including candidate transcripts
and rationales. Cite this digest with the source commit during human review; any content edit
creates a new review identity. Formatting-only changes do not alter the digest. It is an integrity
reference, not a signature, approval, secret holdout, or protection against someone changing both
the content and digest.

Each `-unprompted` case keeps its prompted case's scenario and settings but removes the skill
instruction, then requires transcript evidence that the expected skill was invoked. It measures
skill selection separately from behavior after an explicit request; the offline pairing lint
checks the twins without running a model.

`no-skill-arithmetic` is one negative selection case: Skill remains available, but the request
only needs arithmetic. This is corpus preparation, not a measured selection result. Before any
result is accepted, the external runner must preserve the grader distinction between a completed
answer with no Skill invocation and an INCOMPLETE run caused by a missing or truncated transcript,
tool failure, or infrastructure failure.

`no-skill-audit-grammar` is a lexical near-miss negative: the request contains "test audit"
but asks only for a grammar correction. Skill stays available, and the grader requires a
completed, meaning-preserving correction without a Skill invocation. The isolated eval must
expose only the intended development-contracts skills; a host editing skill would change the
case's premise. The external runner must keep INCOMPLETE separate from non-selection. These
prepared labels need human review before a judge can be qualified; no selection result is
claimed here.

`audit-before-planning` is a competing-intent selection case. The request starts with planning
but asks for an assessment of two existing tests. Run it with the isolated development-contracts
skill set, including planning and auditing skills. Its grader checks invocation and completed
assessment separately; another relevant skill invocation alone is not a failure. Missing or broken
execution is INCOMPLETE. This is a prepared label, not a measured selection result. Human review
of responses is still needed before judge qualification; false passes, false failures, repeat
disagreement, and actual selection rates remain unmeasured.

First run (2026-10-05, 3 runs per arm). This is one noisy sample: a review re-run of
`keel-scope` scored 0.67 with the skill and 1.00 without it, so treat any single-run number as
+/- one run. Use `--runs` to raise runs per arm before reading a delta as real.

| Case | With | Without | Delta |
|---|---|---|---|
| keel-scope | 1.00 | 1.00 | 0.00 |
| keel-green-on-parent | 1.00 | 1.00 | 0.00 |
| keel-observer-missing | 1.00 | 1.00 | 0.00 |
| test-audit-junk | 0.67 | 0.67 | 0.00 |
| test-audit-deletion | 1.00 | 0.33 | +0.67 |

A zero delta means the base model already behaves this way on that prompt (or a host-level
instruction file carries the same rule), so the case guards against regression rather than
showing the skill's lift. Only `test-audit-deletion` currently shows lift. Harder keel cases are
the next step.
