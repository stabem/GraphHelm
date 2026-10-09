# Studio interaction baseline

This opt-in observer measures the current Studio, without changing rendering,
polling, Runtime code or production telemetry. It serves the bounded measurement
slice of issue #185, not its broader stalled-read/large-history investigation.

## Run

Requires the installed `graphhelm`, Git Bash on Windows (Bash elsewhere), Node/npm,
Python and Python Playwright with Chromium. Install observer dependencies locally:
`python -m pip install playwright==1.58.0` and `python -m playwright install chromium`.
In `apps/studio`, run `npm ci --prefer-offline --no-audit --no-fund`.
From the repository root:

```powershell
python -m unittest tools/studio-speed/test_measure.py
python tools/studio-speed/measure.py --fixture-dir C:/gh/codex-4/speed-fixture-new --output C:/gh/codex-4/speed-new.json
```

Both output and fixture paths must be new. Ports 8897 and 5284 must be free;
override with `--runtime-port` and `--studio-port`. The script starts and stops
`tools/studio-journey-fixture/fixture.sh` using the installed binary, with no Cargo
build. The fixture directory is retained locally, including its private token and
logs; do not commit it. Only the JSON report is shareable. No prompts, credentials,
identities, response bodies, screenshots or traces are exported.

On the shared team machine, put the measurement command in a lane-owned Bash script
and run it through the workspace slot, even though it has no Rust build:

```powershell
graphhelm workspace slot --root D:\gh --lane codex-4 --jobs 6 --label studio-speed-185 -- "C:\Program Files\Git\bin\bash.exe" C:/gh/codex-4/speed-185.sh
```

## What each number observes

One headless Chromium runs ten fresh browser contexts **sequentially**, at
1440 x 1000. Each uses the same fixture Runtime and Vite dev server. There is no
untimed warmup: the first sample includes cold server work reached by that action;
later samples benefit from warm server caches. Initial page navigation and typing
the fixture token happen before the Connect measurement. This is not a bundled
production build or a cold navigation benchmark.

| Action | Start | Visible end / correctness check |
|---|---|---|
| Connect | Connect click | Graph's Team control visible; selected run is `demo` |
| Switch tabs | Each tab click | Team: planner; Journey: first journey row; Graph: truthful empty journey state |
| Open journey | Chat journey row click | Its two steps and matching detail heading; different detail before click |
| Send chat | Send click, after filling Message | Unique message in Everyone; composer clears; reload rereads it from Runtime |

Times use the browser's monotonic clock, from the actual click event to a visible
DOM result plus two animation frames (a paint opportunity, not physical display
verification). Locator observation and frame scheduling add overhead. Independent
content assertions can fail regardless of speed; a failure exits nonzero and writes
no report. There are no millisecond pass/fail thresholds. Each tab direction has its
own distribution instead of mixing unlike transitions.

The dataset is one manual-override fixture run, one planner question, and the
repository's draft Studio flows. Each pass adds one synthetic chat record, so
history grows by ten records. Journey preview requests alone receive a 404 stub,
using Studio's existing older-Runtime fallback: opening a journey otherwise launches
another browser. Details are measured; replay, screenshots, approvals and agent
replies are **not** measured. All other UI requests reach the real fixture Runtime.

## Baseline: 2026-10-09

Pending measurement. The accompanying JSON will retain all ten samples, tool
versions, source revision and dirty-tree status. Median uses the middle pair;
p95 uses nearest rank, which is the maximum with only ten samples. These are local
development observations, with low tail confidence and machine-load sensitivity.
They do not establish a production bottleneck or authorize tuning. Network,
Runtime, parse and render decomposition, stalled reads, large histories and
production browser/display latency remain unobserved.
