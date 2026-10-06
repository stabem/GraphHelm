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
hand `npm install --save-dev @playwright/test` then `npx playwright install chromium`. The CLI ships this script and writes it to
`<app dir>/.graphhelm/observers/` on install, so a GraphHelm checkout is not needed.
## Journey captures

```sh
python tools/playwright-observer/playwright_observe.py --project <app dir> --journey checkout \
  --events <events dir> --execution <id> --keyring <keyring> --key-id <key id>
```

With `--journey <id>` the observer reads `<app dir>/.graphhelm/journeys/<id>.json` and, after the
run, records `graphhelm journey capture` per step and `graphhelm journey walked` per consecutive
pair (`--graphhelm` or `GRAPHHELM_BIN` names the CLI). The four record flags are required.

- One Playwright test per journey step, titled exactly with the step id, with
  `use: { screenshot: 'on' }`. A test may attach its own PNG named after the step id
  (`testInfo.attach('<stepId>', { path, contentType: 'image/png' })`); it wins over the automatic shot.
- A failed or unshot step is not captured, and its arrows are not walked. The output lists it under
  `journey.missing`. The exit code still follows the Playwright verdict only.
- It needs the Runtime's events dir and keyring, because each capture is a sealed signal.

`--command` or `GRAPHHELM_PLAYWRIGHT_COMMAND` overrides the default
`npx --no-install playwright test --reporter=json`.
