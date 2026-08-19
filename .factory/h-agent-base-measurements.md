# H Agent — M09 base measurements (W1 inheritance)

Measured: 2026-08-19, main checkout `F:\github\GraphHelm`.
Base: branch `issue-m09-arming-the-alarm` @ `53d212d` — UNTOUCHED, zero tracked
modifications before/during/after (untracked `.factory/` coordination docs only).
`stash@{0}` not popped. Toolchain `cargo +1.97.1` (active via pin). No fixes applied.

## Machine context

- Windows 11 Pro 10.0.26200, local NTFS checkout.
- No competing cargo/rustc/graphhelm processes before or during (verified via
  Win32_Process; pair serve on 41999 was down). All runs SERIAL — never two test
  processes at once. No linker errors, no disk anomalies.
- Build once: `cargo +1.97.1 test -p graphhelm-cli --test api_http --test wake_http --no-run`
  → `target\debug\deps\api_http-9d8a75daaace784c.exe`, `target\debug\deps\wake_http-5b7d49c47a83f4dd.exe`.
  All measurement runs invoked the test binaries directly (no cargo overhead in wall times).

## Commands

Isolated (per test, N=10):
`target\debug\deps\<binary>.exe --exact <test_name> --nocapture`
In-suite (per target, N=3, default parallelism):
`target\debug\deps\<binary>.exe --nocapture`

## Headline numbers

| Test | Isolated fail/N | In-suite fail/N | Distinct failure forms |
|---|---|---|---|
| the_storm_holds_under_eight_concurrent_agents | **4/10** | **2/3** | read-phase 10060 (always) + get_status 10060 (sometimes) |
| a_sleeper_wakes_on_a_peer_append_with_zero_requests_in_the_window | **9/10** | **3/3** | ONE form only (wake_http.rs:822:5) |
| concurrent_sweeps_never_double_consume_a_lease | **0/10** | **0/3** | none observed |

In-suite, the only failing test in each target was the flake under study:
api_http = 30 passed / 1 failed (storm) in failing runs; wake_http = 18 passed / 1
failed (sleeper) in all 3 runs. concurrent_sweeps passed inside every suite run.

**Measurement note, not mechanism:** sleeper at 12/13 total failures is not
behaving as a flake at this base — it is near-deterministically red, isolated and
in-suite, with a single stable assertion form. Only isolated run 7 passed.

## Per-run table (exit 0 = pass, 101 = fail; secs = wall clock per run)

### storm, isolated
| run | exit | secs |
|---|---|---|
| 1 | 0 | 23 |
| 2 | 0 | 31 |
| 3 | 101 | 22 |
| 4 | 0 | 29 |
| 5 | 0 | 24 |
| 6 | 101 | 21 |
| 7 | 101 | 24 |
| 8 | 0 | 23 |
| 9 | 0 | 24 |
| 10 | 101 | 26 |

### sleeper, isolated
| run | exit | secs |
|---|---|---|
| 1 | 101 | 3 |
| 2 | 101 | 2 |
| 3 | 101 | 1 |
| 4 | 101 | 2 |
| 5 | 101 | 1 |
| 6 | 101 | 2 |
| 7 | 0 | 1 |
| 8 | 101 | 2 |
| 9 | 101 | 1 |
| 10 | 101 | 2 |

### sweeps, isolated
| run | exit | secs |
|---|---|---|
| 1 | 0 | 28 |
| 2 | 0 | 26 |
| 3 | 0 | 25 |
| 4 | 0 | 30 |
| 5 | 0 | 31 |
| 6 | 0 | 27 |
| 7 | 0 | 33 |
| 8 | 0 | 32 |
| 9 | 0 | 34 |
| 10 | 0 | 37 |

### full suites
| suite | run | exit | secs | result line |
|---|---|---|---|---|
| api_http | 1 | 101 | 26 | FAILED. 30 passed; 1 failed (storm) |
| api_http | 2 | 0 | 29 | ok. 31 passed |
| api_http | 3 | 101 | 19 | FAILED. 30 passed; 1 failed (storm) |
| wake_http | 1 | 101 | 40 | FAILED. 18 passed; 1 failed (sleeper) |
| wake_http | 2 | 101 | 39 | FAILED. 18 passed; 1 failed (sleeper) |
| wake_http | 3 | 101 | 38 | FAILED. 18 passed; 1 failed (sleeper) |

Total measurement wall time ≈ 12.7 min (isolated: storm 247s, sleeper 17s,
sweeps 303s; suites: api 74s, wake 117s) + 15s build.

## Verbatim failure texts

### storm — phase attribution (panic file:line distribution across ALL failing runs)

Every failing run (isolated 3,6,7,10; suite 1,3) contains 1–5 of:

```
thread '<unnamed>' panicked at apps\cli\tests\api_http.rs:464:34:
called `Result::unwrap()` on an `Err` value: Os { code: 10060, kind: TimedOut, message: "A connection attempt failed because the connected party did not properly respond after a period of time, or established connection failed because connected host has failed to respond." }
```

Isolated run 3 and suite runs 1,3 additionally contain (1–2 occurrences):

```
thread '<unnamed>' panicked at apps\cli\tests\api_http.rs:221:33:
request to http://127.0.0.1:65301/v1/executions/exec-http-storm failed: A connection attempt failed because the connected party did not properly respond after a period of time, or established connection failed because connected host has failed to respond. (os error 10060)
```

(The port number varies per run; suite run 3 had 2× the :221 form.)

Every failing run ends with the join panic:

```
thread 'the_storm_holds_under_eight_concurrent_agents' panicked at apps\cli\tests\api_http.rs:1468:5:
a scoped thread panicked
```

Panic-line counts per failing run:
| run | :443 connect | :460-461 write | :464 read | :221 get_status | :1468 join |
|---|---|---|---|---|---|
| iso 3 | 0 | 0 | 2 | 1 | 1 |
| iso 6 | 0 | 0 | 5 | 0 | 1 |
| iso 7 | 0 | 0 | 1 | 0 | 1 |
| iso 10 | 0 | 0 | 1 | 0 | 1 |
| suite 1 | 0 | 0 | 4 | 1 | 1 |
| suite 3 | 0 | 0 | 3 | 2 | 1 |

**ZERO connect-phase (api_http.rs:443) and ZERO write-phase (:460-461) panics in
any failing run.** All observed 10060s are read-phase (:464) or inside get_status
(:221, phase-laundered). For D: this is the free phase attribution B's T2 predicted.

### sleeper — one deterministic form (all 12 failures, isolated and suite, identical modulo timestamp/sessionId)

```
thread 'a_sleeper_wakes_on_a_peer_append_with_zero_requests_in_the_window' panicked at apps\cli\tests\wake_http.rs:822:5:
assertion `left == right` failed: the lease burned on the ring: {"command":"execution.wake_lease","data":{"contentHead":15,"cursor":13,"head":15,"lastConsumed":null,"live":true,"maturesAt":"2026-08-19T12:23:50.859198500Z","rendezvousId":"rdv-choreo-a","sessionId":"27e3a378a884b808"},"diagnostics":[],"ok":true}
  left: Bool(true)
 right: false
note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace
```

Stable across every failure: `contentHead:15, cursor:13, head:15,
lastConsumed:null, live:true`, rendezvousId `rdv-choreo-a`, assertion at
wake_http.rs:822:5, `left: Bool(true)` vs `right: false`. No other form ever
appeared. (For A Agent's four-mechanism fork: this is the deciding text, quoted
exactly — the post-ring lease reads back `live:true` with `lastConsumed:null`.)

### sweeps — no failures at 53d212d

0/10 isolated, 0/3 in-suite. C Agent's registered prediction targets the PARENT
of 53d212d — untested here (base-only mandate). At 53d212d itself, in these
scopes, the double-consume did not reproduce in N=13.

**Execution receipt (added 2026-08-19 on C's zeros audit — proves the zero's
instrument was alive):** all 10 isolated logs (`sweeps_run1..10.log`) each carry
`test concurrent_sweeps_never_double_consume_a_lease ... ok` (10/10 by grep) and
`test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 18 filtered out`
per run (verbatim, durations 25.13s–36.80s matching the per-run table above).
The test EXECUTED and PASSED in every run; this 0/10 is run-verified, not a
dead-harness zero.

## ADDENDUM 2026-08-19 — double-duty sleeper run at aac0d67 (RAW, unscored)

Ordered by orchestrator after C's fix landed on the milestone branch. H reports
raw only; scoring belongs to M against the sealed ledger.

- Checkout: main `F:\github\GraphHelm`, branch `issue-m09-arming-the-alarm` @
  `aac0d67` ("fix(wake): the consume pins its sequence from the read that judged
  the lease" = 53d212d + exactly 1 commit, touching only
  `apps/cli/src/commands/serve/wake.rs`, +335/−17).
- Tree state: one tracked modification, `.factory/handoff-agent-b.md`
  (coordination beacon, cargo-invisible — same class the orchestrator ruled
  clean for the base run). No other tracked changes. `stash@{0}` untouched.
- Build: `cargo +1.97.1 test -p graphhelm-cli --test wake_http --no-run`
  → harness `target\debug\deps\wake_http-5b7d49c47a83f4dd.exe` (unchanged from
  base — fix is bin-only code; test file untouched at aac0d67), spawned bin
  `target\debug\graphhelm.exe` rebuilt fresh at 10:46:03 local (verified mtime;
  test resolves the bin via `assert_cmd::cargo::cargo_bin!("graphhelm")`,
  wake_http.rs:34).
- Invocation per run (serial, isolated, N=10):
  `target\debug\deps\wake_http-5b7d49c47a83f4dd.exe --exact a_sleeper_wakes_on_a_peer_append_with_zero_requests_in_the_window --nocapture`
- No competing cargo/rustc/test processes (verified via Win32_Process before run).

**Count: 0 failures / 10 runs.** Every run: `test result: ok. 1 passed; 0
failed; 0 ignored; 0 measured; 18 filtered out`. No failure text exists to quote.

| run | exit | secs |
|---|---|---|
| 1 | 0 | 2 |
| 2 | 0 | 1 |
| 3 | 0 | 1 |
| 4 | 0 | 1 |
| 5 | 0 | 1 |
| 6 | 0 | 2 |
| 7 | 0 | 1 |
| 8 | 0 | 2 |
| 9 | 0 | 1 |
| 10 | 0 | 2 |

Comparable base (same scope, isolated-only): 9/10 failures at 53d212d.
Logs: `sleeper_fix_run1..10.log` in the H session scratchpad (path below).

## ADDENDUM 2 — 2026-08-19 — wake_http FULL SUITE at aac0d67 (RAW, unscored)

Ordered by orchestrator (suite scope; M sealed verdict cells before this ran).
Same checkout/build as Addendum 1: `issue-m09-arming-the-alarm` @ aac0d67,
tracked mod only `.factory/handoff-agent-b.md`, spawned bin fresh at aac0d67.

Invocation per run (serial, N=10, default parallelism):
`target\debug\deps\wake_http-5b7d49c47a83f4dd.exe --nocapture`

**Count: 0 failing runs / 10; 0 failing tests / 190.** Every run:
`test result: ok. 19 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out`.
Grep across all 10 logs for `FAILED|panicked`: zero hits. No failure text exists.
Per-test pass/fail: all 19 test names passed in all 10 runs (sleeper included).

| run | exit | secs |
|---|---|---|
| 1 | 0 | 31 |
| 2 | 0 | 30 |
| 3 | 0 | 30 |
| 4 | 0 | 31 |
| 5 | 0 | 30 |
| 6 | 0 | 30 |
| 7 | 0 | 31 |
| 8 | 0 | 31 |
| 9 | 0 | 31 |
| 10 | 0 | 32 |

Comparable base (same scope): sleeper failed 3/3 in-suite at 53d212d.
Logs: `suite_wake_fix_run1..10.log` in the H session scratchpad (path below).

## ADDENDUM 3 — 2026-08-19 — storm RE-BASELINE at ef51193 (RAW, unscored; D's spec stage 1)

Ordered by orchestrator (storm phase, D's run spec section 0: the 53d212d figures
are HISTORY, not control — #74 changed projection.rs/integrity.rs in the
replay/open path between them).

- Checkout: main, `issue-m09-arming-the-alarm` @ `ef51193`, CLEAN tree (only the
  beacon `.factory/handoff-agent-b.md` modified, cargo-invisible). No probes
  applied at run time; `GRAPHHELM_OPEN_PROBE`/`GRAPHHELM_CLIENT_PROBE`/
  `GRAPHHELM_TEST_WAKE_PHASE3_DELAY_MS` all unset (verified).
- Invocation per run (serial, isolated, N=10, disk checked before each — 19G):
  `target\debug\deps\api_http-9d8a75daaace784c.exe --exact the_storm_holds_under_eight_concurrent_agents --nocapture`
  (spawned `graphhelm.exe` rebuilt at ef51193 15:03:30; harness identity carries —
  `apps/cli/tests/api_http.rs` untouched 53d212d..ef51193, package has no lib target).

**Count: 0 failures / 10 runs.** Receipts both grains: 10× `test
the_storm_holds_under_eight_concurrent_agents ... ok` (named grain), 10× `test
result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 30 filtered out`. Grep
`panicked` across all 10 logs: zero hits — no failure text exists.

| run | exit | secs | | run | exit | secs |
|---|---|---|---|---|---|---|
| 1 | 0 | 18 | | 6 | 0 | 17 |
| 2 | 0 | 17 | | 7 | 0 | 18 |
| 3 | 0 | 18 | | 8 | 0 | 18 |
| 4 | 0 | 17 | | 9 | 0 | 19 |
| 5 | 0 | 18 | | 10 | 0 | 17 |

**Bound, stated so the zero is not overread (D's phrasing request):** 0/10 puts a
95% upper bound of ~26% on the true rate; a residual 10–25% flake is fully
consistent with this result. This is strong evidence the rate DROPPED from the
53d212d-era 4/10 (a 40% rate yields 0-in-10 ~0.6% of the time) but NOT evidence
the flake is gone ("gone" needs ~60 clean runs). The two numbers also do not
share a controlled environment: between them landed #74 (store path), C's fix,
A's seam, AND a disk-state change (orchestrator freed c-agent/k-agent target
caches before this run; disk at 53d212d-run time was unmeasured), AND fleet load
(concurrent agent sessions differed between the two windows, unrecorded) — THREE
uncontrolled variables: code, disk, concurrent load. None separable from these
two measurements alone. D proposed a disk-confound control (53d212d, N=10,
today's disk), then WITHDREW it as a demand after costing (needs a 53d212d
rebuild; both execution shapes destructive against the fleet/disk floor) —
status: desirable if ever genuinely free, not required. The forward-looking
substitute costs nothing: the scheduled instrumented run records per-open
elapsed at a KNOWN disk state, and max/median open × opens-per-request vs the
5s budget says whether this machine is near the cliff or far from it (D's D1
arithmetic doing double duty). Run-hygiene rule adopted from this loss: every
future run table records free disk and a concurrent-cargo count next to exit
code and wall time.

Logs: `storm_rebase_run1..10.log` in the H session scratchpad (path below).

## Raw logs

Per-run logs + results.csv in H session scratchpad:
`C:\Users\gabri\AppData\Local\Temp\claude\F--github-GraphHelm--claude-worktrees-h-agent-cbfd67\07a04345-0dfa-4c42-80b0-650d859185c2\scratchpad\`
(`storm_runN.log`, `sleeper_runN.log`, `sweeps_runN.log`, `suite_api_runN.log`,
`suite_wake_runN.log`). Session-scoped — copy out anything needed long-term.
