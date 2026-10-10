# Studio click-to-visible measurements

This is a measuring tool, not a performance gate. It changes no Studio settings,
sets no pass/fail threshold and compares no builds. Use only a disposable Studio
started by `tools/studio-journey-fixture/fixture.sh`; never a live project.

## Run

Install the Studio dependencies in this checkout with
`npm ci --prefer-offline --no-audit --no-fund` from `apps/studio`.
The observer resolves `@playwright/test` from `apps/studio/package.json`, or from
the project named by `GRAPHHELM_JOURNEY_TOOLCHAIN_PROJECT`. Install Playwright and
its Chromium in that observer project first. No new repository dependency is added.

In Git Bash, from the repository root (choose unused isolated ports and a fresh
directory owned by your lane):

```bash
fixture=D:/gh/cx/codex-15/perf-fixture-185
trap 'bash tools/studio-journey-fixture/fixture.sh down "$fixture"' EXIT
bash tools/studio-journey-fixture/fixture.sh up "$fixture" 18815 15815
node tools/studio-perf/measure.mjs --base http://127.0.0.1:15815 --runs 5 --out D:/gh/cx/codex-15/perf-185.json
```

On the shared machine, run that script through the workspace build slot:

```powershell
graphhelm workspace slot --root D:\gh --lane codex-15 --jobs 6 --label studio-perf-185 -- "C:\Program Files\Git\bin\bash.exe" D:/gh/cx/codex-15/measure-185.sh
```

Never use ports 5183, 5196 or 8793, including for the fixture Runtime. The tool
refuses those Studio ports and non-local URLs, but cannot verify which Runtime
a separately launched Studio proxies to. The fixture launcher provides isolation.
Always run `down`, including after failure. No events are sent by the measurement
tool; the launcher seeds only its disposable store.

## What is measured

One headless Chromium browser runs sequentially, with a fresh browser context per
run and a fixed 1440 by 900 viewport. `--runs` must be an integer from 1 to 20.
For this slice, run one five-run batch, not a load test. These are fresh contexts
against one dev server, not five cold server starts; later runs may use warm server caches.

1. Navigation to the fixture until the `Run actions` button is visible.
2. Clicking the fixture's `Deploy · ready` step until region `Node deploy` is visible.
3. Closing the node window, then clicking `Chat` until textbox `Message` is visible.

Clicks are dispatched inside the page, immediately after reading `performance.now()`.
Visibility uses `checkVisibility()` with opacity and CSS visibility checks, polled
once per animation frame, then finishes on the following animation frame. This
observes DOM visibility, not physical pixels on a display. Setup waits and closing
the node window are outside the click measurements. Load starts with a performance
clock read in the previous document immediately before `location.assign()` in the
same page task. `performance.timeOrigin` bridges the document clocks. The load
observer is installed before navigation.

Each observation has a 10-second page-side cap and reports `timeout`, never a
numeric substitute. This cap is cooperative: a blocked browser main thread can
delay its timer; a late frame is still rejected. Navigation also has a Playwright
10-second timeout. Missing setup controls cause an error instead of fabricated
measurements. A load timeout skips both clicks; a node timeout skips Chat for that run.

The JSON contains individual observations, completed/timeout/skipped counts and
median, nearest-rank p90 (`ceil(0.9 * N)`) and maximum in milliseconds. Median
averages the two middle values for even counts. With any incomplete observation,
that interaction's aggregate is `null`, so partial samples cannot look like a
complete N-run result. CPU count, available parallelism, Node/Chromium versions,
OS and viewport accompany the report. stdout prints one summary line.

## Pure statistics proof

```powershell
node --test tools/studio-perf/stats.test.mjs
```

The tests cover numeric ordering, odd/even medians, nearest-rank p90 on ten values,
maximum, a single value, unchanged inputs and refusal of empty/non-finite input.
They require only Node, no browser, Runtime or network; normally under one second.
