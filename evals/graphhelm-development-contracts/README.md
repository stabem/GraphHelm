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
