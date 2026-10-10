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
| Connect | Connect click | Graph's truthful empty journey state visible; selected run is `demo`; Graph selected |
| Switch tabs | Each tab click | Graph â†’ Lanes: planner on Agent board; Lanes â†’ Proof: selected Proof and truthful empty journey state |
| Open Journeys | Journeys button click | First draft journey row visible |
| Return to Graph | Graph tab click from Journey canvas | Truthful empty journey state |
| Open journey | Chat journey row click | Its two steps and matching detail heading; different detail before click |
| Open Chat | Chat toggle click from Graph | Everyone tab visible beside Graph; `aria-pressed=true` |
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
another browser. The Proof tab has no certified journeys in this fixture: only
its selected state and truthful empty state are observed, not a populated proof
table. Details are measured; replay, screenshots, approvals and agent replies
are **not** measured. All other UI requests reach the real fixture Runtime.

## Baseline: 2026-10-09 (Sao Paulo)

Measured at 01:04 UTC on October 10 (22:04 on October 9 in Sao Paulo) on Windows,
Chromium 145.0.7632.6, Playwright 1.58.0 and installed GraphHelm 0.1.1. The clean measurement source was
`1ef4fe58a16f0f3eef44cf1142242cdd66de9f57`; subsequent changes only add these results.
All ten passes completed in one slot-held run, including the chat reload checks.
No other work from this lane ran during collection.

This replaces the earlier Team/Journey navigation baseline; the routes differ, so
these values are not evidence of a speedup. The unchanged parent observer at
`1dd84fcfea487911c729dc5871dc25ee012172f9` failed on run 1 at Connect because
it waited for the retired Team control. That failed attempt wrote no report.
The updated observer below is the single completed baseline collection.

| Interaction | Median (ms) | p95 (ms) |
|---|---:|---:|
| Connect to first usable paint | 236.45 | 251.90 |
| Graph to Lanes | 60.95 | 63.00 |
| Lanes to Proof | 61.20 | 63.00 |
| Proof to Journeys | 129.85 | 146.10 |
| Open journey details | 79.05 | 95.60 |
| Journey canvas to Graph | 78.05 | 79.10 |
| Open Chat beside Graph | 60.45 | 62.50 |
| Send chat to visible message | 282.60 | 295.50 |

[Raw JSON](studio-speed-2026-10-09.json) retains all ten samples, tool versions,
source revision and dirty-tree status. Median uses the middle pair; p95 uses
nearest rank, which is the maximum with only ten samples. These are local
development observations, with low tail confidence and machine-load sensitivity.
They do not establish a production bottleneck or authorize tuning. Network,
Runtime, parse and render decomposition, stalled reads, large histories and
production browser/display latency remain unobserved.
