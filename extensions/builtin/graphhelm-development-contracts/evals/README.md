# Evals for the development-contracts skills

Run from the repository root:

```sh
claude plugin eval extensions/builtin/graphhelm-development-contracts --trust-plugin
```

Each case is `prompt.md` plus `graders/criteria.md` (an LLM judge). The runner adds a no-plugin
baseline arm, so the delta column shows what the skill itself changes.

First run (2026-10-05, 3 runs per arm):

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
