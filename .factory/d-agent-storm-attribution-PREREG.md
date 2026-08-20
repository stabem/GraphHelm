# Storm rate-movement attribution — PRE-REGISTERED before any run

Written by D, 2026-08-20, after the api_http watch fired. **I wrote #103's gate; this is my
change under suspicion and I am the one measuring it.** Everything below is sealed before a
single run.

## What the world already told us, before I measure anything

B's evidence: `the_storm_holds_under_eight_concurrent_agents` RED in both stages of both gate
runs (4/4), plus 1 FAIL / 1 PASS isolated on an idle machine — **5 fails in 6 observations**,
every panic the documented 10060 shape at `api_http.rs:464:34` (`stream.read_to_end`, the 5s
`SO_RCVTIMEO` expiring). The idle-isolated failure is the one that matters: **my storm study
characterized this test as reliable when idle, and an idle fail falsifies that characterization
regardless of what causes it.**

## The structural fact I established BEFORE designing the runs

This is not a coincidence of targets. **The storm drives the exact graph #80 is about.**

- `cli_start` (`api_http.rs:376`) loads `examples/graphs/manual-override-deploy.yaml` —
  `implementation -> deploy`, one `data` edge, no condition.
- `blocked_fixtures` (`:978`) is `{"implementation": "failure"}`, so `implementation` reaches
  `Blocked` at the no-progress bound and `deploy` is `Ready`-but-edge-gated.
- `storm_thread` (`:1492-1503`) hammers **pause / resume / signal** from 8 concurrent threads.

Pause holds `deploy` (`Paused`); resume force-records `Started`, and
`(Paused, Started) => Queued` puts it in the retry chain. **That is exactly the state #103's gate
changed.** Pre-#103 the ungated retry chain dispatched `deploy`; post-#103 it does not.

So #103 is not merely "in the dependency chain" — it is on the storm's hot path by construction.

## Sealed hypothesis and mechanism

**H1 (mine, and the one I expect):** the rate MOVED UP with #103, and the mechanism is
**execution longevity, not per-pass CPU**. Pre-#103, `deploy` dispatched after a resume and the
execution progressed toward a terminal state; once terminal, later storm rounds hit fast domain
refusals. Post-#103 `deploy` stays `Queued` forever, the execution never completes, and **every
round of every thread keeps doing full drive work** — more store opens per request, more
contention on the per-open exclusive lock (~30 ms of structural work each, measured in the storm
lane), longer requests, more 5 s read timeouts.

**H2 (the alternative I must not suppress):** the environment moved (disk, machine load) and #103
is coincidental. B's idle-isolated failure is weak evidence FOR H2 and against my own prior
characterization.

**Note H1 predicts a direction opposite to the naive "more work per pass" reading**, which would
have predicted a tiny CPU cost. I am registering longevity, not CPU, and if the rate moved for a
CPU reason my mechanism is wrong even if my direction is right.

## The cheap structural check, run FIRST

Before any rate statistics: **compare the storm execution's END STATE pre vs post.** If post-#103
the stream ends with `deploy: queued` and the execution non-terminal where pre-#103 it did not,
H1's mechanism is confirmed structurally with N=1 per arm and no statistics needed. A rate claim
still needs the paired runs; a MECHANISM claim does not.

## Paired rate design

- Two isolated git worktrees, one per arm — **PRE = `07b1243`, POST = `e0849e8`** — each with its
  own `CARGO_TARGET_DIR`, so neither the source tree nor the built binaries can cross-contaminate
  (the test bakes `CARGO_MANIFEST_DIR` and the CLI binary path at compile time).
- Build both FIRST, then **alternate arms run-by-run** in one session. Alternating rather than
  blocking is the whole point: the storm lane's central finding is that this test is
  disk-conditional, and two blocks measure disk drift as if it were code.
- **N = 10 per arm, 20 runs.** Justification, and its limit stated: at N=10, an observed 0/10
  leaves a ~26% upper bound at 95% — so **this design can establish a MOVEMENT but cannot
  establish a ZERO**. If POST fails 5+/10 and PRE fails 0/10, that is a movement no reasonable
  bound reconciles. If both arms land mid-range, N=10 will not separate them and I will say the
  design was underpowered rather than reading a difference into it.
- Per run: `free_gb`, concurrent cargo/rustc count, wall time, PASS/FAIL, and on FAIL the panic
  site — `--nocapture` so every failure carries its line.

## Pre-registered split

| Arm | Predicted | |
|---|---|---|
| POST (`e0849e8`, with my gate) | **6-10 failures / 10** | matching B's observed 5/6 |
| PRE (`07b1243`, without it) | **0-2 failures / 10** | the pre-#103 baseline |

## Falsifiers, stated so I cannot move them afterwards

- **H1 IS FALSIFIED if the two arms' failure counts are within 2 of each other.** Then #103 did
  not move the rate, the environment owns it, and my characterization updates.
- **H1's MECHANISM is falsified independently of its direction** if the structural check shows the
  pre-#103 stream ALSO ends non-terminal with `deploy` unrun. Then longevity is not the
  difference, and a rate movement (if any) needs another mechanism — I do not get to keep the
  direction and swap the reason.
- **The whole design is void if the two arms' `free_gb` differ by more than ~2 GB across the
  session**, because that is the confound this lane spent a day failing to untangle. I will report
  the disk figures per run and void rather than reinterpret.

## What I will NOT do

- I will not report a POST-arm improvement as exoneration if PRE also fails at a similar rate;
  that is H2, and H2 means the red has an environmental owner, not that #103 is innocent of
  everything.
- I will not claim a zero from any arm at this N.
- I will not adjust the split above after seeing results. If it is wrong, it is wrong in writing.
