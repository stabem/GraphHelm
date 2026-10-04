# Playwright browser observer

`playwright_observe.py` runs `npx playwright test` in a project and turns its JSON report into
proof lines for a `keel.proof` signal ([`docs/keel/RECORDS.md`](../../docs/keel/RECORDS.md)). It is
the default browser journey observer: deterministic, no model key, no AI cost. The `e2e` adapter
([`../e2e-observer`](../e2e-observer/README.md)) is the opt-in alternative for agent-driven steps.

```sh
python tools/playwright-observer/playwright_observe.py --project <app dir>
```

| Script exit | `verdict` | When |
|---|---|---|
| 0 | `passed` | playwright exited 0 and its report counts tests that ran and none unexpected |
| 1 | `failed` | playwright exited 1 and its report counts an unexpected result |
| 2 | `observer_missing` | runner not on PATH, timed out, any other exit, or the report is missing, broken, empty or disagrees with the exit |

The report is written to `.graphhelm/playwright-report.json` (via `PLAYWRIGHT_JSON_OUTPUT_NAME`)
and deleted before each run, so an earlier report never stands in. The evidence lists the command
and its exit code, the report's sha256, and one line per test (`status: title path`).

Setup: `graphhelm setup --project <app dir> --home <home> --install-observer playwright`, or by
hand `npm install --save-dev @playwright/test` then `npx playwright install chromium`.
`--command` or `GRAPHHELM_PLAYWRIGHT_COMMAND` overrides the default
`npx --no-install playwright test --reporter=json`.
