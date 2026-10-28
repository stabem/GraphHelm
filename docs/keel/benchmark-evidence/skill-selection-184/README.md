# Skill selection re-measure (#184), 2026-10-28

`python tools/evals/run_selection.py --repeats 3 --timeout 600 --budget-usd 20` on a clean
checkout of `origin/main` `75a508f2`. Claude Code 2.1.311, model `sonnet`, subscription login
(first-party), provider override variables removed from the child environment. Hidden,
one run at a time. **39/39 completed, 0 infra errors.** CLI cost estimate $9.39 (an estimate
printed by the CLI, not a bill). The cases, runner and report are unchanged from main; nothing
was tuned before or after this run.

Files: `observations.json` (runner output, one row per run) and `report.json`
(`selection_report.py` over it).

| kind | correct trigger | miss | false trigger | correct abstain |
|---|---|---|---|---|
| prompted (15) | **15** | 0 | 0 | - |
| unprompted (15) | **2** | 13 | 0 | - |
| no-skill (6) | - | - | **0** | **6** |
| competing intent (3) | 0 | **3** | 0 | - |

Per case (run 1 / 2 / 3, skills invoked):

| case | runs |
|---|---|
| audit-before-planning | - / - / - |
| keel-green-on-parent | keel / keel / keel |
| keel-green-on-parent-unprompted | - / - / - |
| keel-observer-missing | keel / keel / keel |
| keel-observer-missing-unprompted | - / - / - |
| keel-scope | keel / keel / keel |
| keel-scope-unprompted | - / - / - |
| no-skill-arithmetic | - / - / - |
| no-skill-audit-grammar | - / - / - |
| test-audit-deletion | test-audit × 3 |
| test-audit-deletion-unprompted | - / - / - |
| test-audit-junk | test-audit × 3 |
| test-audit-junk-unprompted | test-audit / - / test-audit |

## Reading

- Earlier runs on the same suite: 1/15 unprompted (2026-10-18 baseline), 4/15 after the
  #1597 descriptions. Today, on current main: **2/15**. With N=3 per case these three numbers
  are within noise of each other; the description gain seen on 10-18 did not reproduce.
- Prompted selection is now 15/15 (was 13/15). No false triggers; no-skill cases abstain 6/6.
- Competing intent (`audit-before-planning`) is still 0/3.

## Limits

- One model, N=3, this machine's installed plugins.
- Measures the pick, not answer quality (completion is `unjudged`).
- The plugin eval cases added in #2198 are not measured: the runner and report read only
  `evals/graphhelm-development-contracts`.
- Judge qualification is still blocked: all candidates in `judge-qualification.json` are
  `pending` human labels, so judge false pass/fail and repeat disagreement are not measurable.
