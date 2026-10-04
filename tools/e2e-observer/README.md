# e2e browser observer

`e2e_observe.py` runs the open-source [`e2e`](https://github.com/tester-army/e2e) runner (npm
package `e2e`, Apache-2.0) in a project and turns its result into proof lines for a `keel.proof`
signal ([`docs/keel/RECORDS.md`](../../docs/keel/RECORDS.md)).

```sh
python tools/e2e-observer/e2e_observe.py --project <app dir>
```

| Script exit | `verdict` | When |
|---|---|---|
| 0 | `passed` | e2e exited 0 and wrote `.e2e/report.json` |
| 1 | `failed` | e2e exited 1 (a test failed) and wrote the report |
| 2 | `observer_missing` | e2e not installed, timed out, exited 2/3/4/130, or wrote no report |

Only a written report with exit 0 or 1 is an observation of the journey. A config, credential,
engine or model-provider failure is not a red journey; it is `OBSERVER_MISSING`.

The evidence lists the command and its exit code, the report's sha256, and one line per result
(`status: title path`) from `run.results`.

Setup in the app under test: `npm i -D e2e`, then `npx e2e-web install chromium`. The runner needs a
model key for agent steps (see the e2e docs); deterministic steps run without one. `--command` or
`GRAPHHELM_E2E_COMMAND` overrides the default `npx --no-install e2e run --reporter list,json,junit`.

This is not yet the JPD bundle's `graphhelm-browser-user-journey` observer: that catalog entry is
part of a digest-bound package and stays `unsupported_by_bundle` until an activation adds it.
