# Gate under five minutes — measurement and implementation plan

> Status: PLANNED. Issue #1053 cites `docs/superpowers/plans/2026-09-12-gate-under-five-minutes.md`;
> that file is not in the tree at any commit reachable from `origin/main`, so this document replaces
> the citation rather than pretending to recover it. Refs #1053, #956, #175, #174, #839, #904, #943.

**Goal.** Bring a complete local gate from a measured median of **1644 s (27.4 min)** to the
300–600 s band, without the gate proving one thing less than it proves today.

Every number below carries its instrument. A number labelled *inferred* has not been measured on
this tree and must not be cited as if it had been.

---

## 1. The measurement

Population: the 370 run records in `.factory/gate-runs/*.json`; 351 of them since 2026-09-01.

| slice | n | median wall |
|---|---|---|
| all runs since 2026-09-01 | 351 | **1644 s** |
| GREEN runs > 60 s | 212 | 1664 s |
| RED runs > 60 s | 146 | **1734 s** |
| `cargoTargetDir` on `D:` (HDD) | 156 | **1866 s** |
| `cargoTargetDir` on `E:` (SSD) | 183 | **1427 s** |

RED is **39 %** of runs (146 / 370; 12 HARNESS-BROKE). On a RED run the first failing stage ends at
a median of **795 s**, and the run then continues for a further **940 s** after the answer is
already known, because every stage runs under `--no-fail-fast`.

### 1.1 The critical path

Gantt of run `f966887a` (PR #1153, GREEN, 1781 s), offsets in seconds from `runStartUtc`:

| offset | stage | s |
|---|---|---|
| 0 | contamination canary | 5 |
| 5 → 576 | build pass — `cargo test --workspace --all-features --no-run` (`buildPassSecs`, not a `stages[]` entry) | **571** |
| 576 | rustfmt | 4 |
| 580 | clippy | 44 |
| 624 → 1117 | workspace tests (both PostgreSQL matrices overlap here) | **493** |
| 1117 → 1506 | 55 `cli: <suite>` stages, strictly serial | **389** |
| 1506 → 1633 | schema catalog | 127 |
| 1633 → 1657 | schema baseline / conformance / locked metadata / whitespace / frozen guard | 24 |
| 1660 → 1774 | `apps/studio` (npm), serial at the end | 114 |
| 1774 | required-features coverage | 1 |
| background | `ci powershell suites`, 5 → 893 | 888 |

Compilation is ~35 % of the path; test execution (493 + 389) is ~50 %.

### 1.2 Hardware, and where the gate builds

AMD Ryzen 9 9950X (16C/32T), 61.6 GB RAM.

| drive | device | media | free | role |
|---|---|---|---|---|
| C: | Kingston SFYRD2000G | **NVMe SSD** | 160 GB | forbidden as a gate target by AGENTS.md |
| D: | WDC WD20PURZ | **HDD** (surveillance platter) | 309 GB | `ci/gate-runner.ps1:77` sends the HDD slot's target here |
| E: | Kingston SA400 | SATA SSD | 61 GB | the SSD slot's target |
| F: | Kingston SA400 | SATA SSD | 49 GB | repository and worktrees |

One target directory measured at **23.1 GB across 68 364 files**.

Sampled live with exactly two gates running (2026-09-19 22:00 local):

```
C:  % disk time    14.3   queue 0.7
D:  % disk time 1 005.0   queue 8.0   <- the HDD gate slot
E:  % disk time     3.8   queue 0.0   <- the SSD gate slot
F:  % disk time     0.1   queue 0.0
```

At that instant the HDD slot (PR #1168, scope SCOPED to 14 crates) had been running 42 min and was
entering the cli loop; the SSD slot (PR #1164, scope FULL — the widest possible run) had started
18.5 min later and was already at `schema catalog`. This is #839, still live, one year of gate time
later.

### 1.3 Scope: the selector escalates on three runs in four

Last 120 records by `runStartUtc`, reading the manifest's `scope.full`:

| | n | share |
|---|---|---|
| FULL | 89 | **74.2 %** |
| SCOPED | 31 | 25.8 % |

FULL reasons: `unmapped-path` 36 · `ci/` 26 · no selection given 14 · `schemas/` 8 · `Cargo.toml` 4 ·
`core/protocols/` 1.

**But widening the selector's reach is not the fix**, and this is the plan's least intuitive
finding. From `cargo metadata --no-deps` (26 members, no compilation): for 23 of the 26 members the
forward closure of the selection is 23–25 crates of 26. `graphhelm-cli`'s own closure is 23 of 26.
Pushed through a replica selector, **146 of the last 150 first-parent commits on `main` (97.3 %)
reach `graphhelm-cli`**. The workspace is a near-complete fan-in, so "run only the crates this
change can reach" cannot narrow much.

Measured confirmation rather than inference — run
`55a8c9df0667-20260919T233903.748Z-8c806bd5.json` is `SCOPED: 1 crate(s) selected, crates:[graphhelm-cli]`.
Its `workspace tests` stage (which does honour scope, `ci/gate.ps1:5159`) ran **354.8 s against the
FULL-run median of 370.2 s — a 4 % difference**. The run still totalled 1539 s, because the build
pass ignores scope entirely.

A `docs/`-only exemption is **not** soundly implementable: `docs/` is genuine build input here.
`apps/cli/src/commands/events/backup.rs:303` does
`include_bytes!(".../docs/acceptance/m05-run-2026-08-16/events/format.json")` at compile time, and
`core/events/tests/{held_file_contention,recoverable_layout}.rs`, `core/quality/tests/geometry.rs`
and `apps/cli/tests/development_sabotage.rs` read files under `docs/` at run time. 6 of the 21
docs-only commits in the sample (29 %) touched a docs subtree that Rust reads. Exempting the class
would skip the very suite that reads the edited file — the silent narrowing
`ci/select-scope.ps1`'s own header forbids.

`apps/studio/*` survives the same check (6 Rust hits, all literal strings in emitted command text)
and is the one sound addition to `$KnownNonBuildInput`.

### 1.4 The build is ~50 s. The other ~515 s is contention.

Measured after the plan was first written, and it reorders the rest of this document.
`cargo test --workspace --all-features --locked --no-run` — the gate's own build pass — run on an
**idle** machine against a **fresh** target directory on the NVMe, four arms, alternating:

```
291 crates Compiling · 267 binaries linked · 0 Fresh on every arm
52 s · 51 s (link.exe)      50 s · 48 s (rust-lld)
```

The recorded cold `buildPassSecs` median is **565.7 s** (n=68). The same work, alone on a fast disk,
is **~50 s**. The gap is not compilation: it is the mechanical disk (L2) and two gates competing for
CPU and the one shared `$CARGO_HOME/.package-cache` lock.

**What this changes.** Every build-side lever in this plan is a slice of ~50 s of real work, so their
ceilings are far lower than the inferred estimates suggested — L6's linker half collapsed from
~103 s to 3 s on exactly this measurement. The levers that matter are the ones that attack
**contention and waiting**: L2 (disk), L5 (scheduling), L7 (fail-fast), and L4's process pool.

**What it does not settle.** Whether a single, uncontended, warm gate is already inside the 300–600 s
band is untested — no such run exists in the population, because the runner has never given one a
warm target on a fast disk with nothing else running. The first gate on this branch is that run.

---

## 2. The levers, ranked by measured saving

Each was investigated and then put through three independent adversarial lenses (evidence,
coverage, feasibility). "Corrected" is the mean of the three lenses' own figures.

### L1 — The runner wipes the target directory before every run · ~500 s · measured

`ci/gate-runner.ps1:480-486` removes `$target` unconditionally, commented
`#943: a re-run must be cold`. Consequence, from the 71 records carrying `buildPassSecs`:

| | n | median `buildPassSecs` |
|---|---|---|
| `buildMode = cold` | 68 | **565.7 s** |
| `buildMode = warm` | 3 | 65.5 s (values: 3.3, 65.5, 875.9) |

`targetBuildState.reason` reads *"no target directory yet, so nothing is being reused"* on **67 of
71** runs. The existence proof that warm reuse is both correct and fast is already in the tree:
`b1e5cd65782e-20260919T192310.826Z-1e1e789c.json`, a **FULL (unscoped)** run, `buildPassSecs`
**65.459**, `artifactsProvenReuse` 324, `artifactsRebuilt` 1.

**The workaround has outlived its cause.** #943 (2026-09-07, `664e3b2c`) added the wipe because the
then *timestamp-based* staleness instrument reddened on a warm target. Its *content-based*
replacement landed nine days later — 2026-09-16 `ebd2b51c` "a reused test binary passes on content,
never on cargo's fingerprint" (#904/#1038) — and was extended 2026-09-19 by `0e346038` (#1007).
Independently, `Get-TargetBuildState` (`ci/gate.ps1:2753`) already answers "did the last build in
this directory END", and the gate already aborts before its first compile when the answer is
`interrupted` or `concurrent` (`ci/gate.ps1:4893-4929`, #455).

So the guard the wipe was standing in for now exists, twice, inside the gate.

**Honest limit:** n = 3 warm runs, and one of them (`1cbacdda2e43`) took 875.9 s with 134 artifacts
rebuilt. A partially-warm target can be slower than a cold one. The claim is *"a target whose
previous run finished can cut the build pass from ~566 s to ~65 s"*, not *"warm is always faster"*.

**Change.** Make the removal conditional on `Get-TargetBuildState` reporting `contaminated` or
`suspect`; preserve a `complete` target. Note that the runner cannot call into `ci/gate.ps1` — the
state reader lives there — so either the state file is read directly by the runner (it is a small
JSON marker, `.graphhelm-build-state.json`) or the removal is dropped entirely and the gate's own
#455 abort is left to decide. Update the `#943` comment block to record that #904/#1038 replaced
its cause, and update the AGENTS.md paragraph that documents the unconditional removal.

**Guard.** One cell in `ci/gate-runner.tests.ps1`: a fixture target holding
`.graphhelm-build-state.json` in each of the three states; assert removal for `contaminated` and
`interrupted`/`concurrent`, preservation for `complete`. Sabotage by flipping the preserved state to
`contaminated` and confirming the cell reddens — otherwise it only tests the happy branch.

**Validation costs no slot.** The gate's own receipt is the instrument: after merging, compare the
next two runner-driven manifests' `buildPassSecs` and `artifactsProvenReuse` against the 565.7 s /
0-proven baseline. An unprovable warm run already shows as `artifactsUnprovenReuse > 0` and
`instrumentSuspect: true`, and goes RED.

### L2 — Every second gate builds on a mechanical disk · ~440 s · measured, observational

`ci/gate-runner.ps1:77` sends the HDD slot to `D:\runner-targets\hdd`. Median totals: **1866 s on
D:, 1427 s on E:** (n = 156 / 183). Per-stage, the difference concentrates exactly where it should —
compile-bound stages roughly halve (`clippy` 52 → 25 s, `schema catalog` 116 → 61 s) while
execution-bound `workspace tests` barely moves (373 → 366 s).

This is observational, not a controlled A/B: the two populations are different PRs. The mechanism,
however, was sampled directly (§1.2: 1 005 % disk time, queue 8).

**Change.** Move the second gate slot off the platter. `C:` is a 2 TB NVMe at 14 % utilisation with
160 GB free and already hosts per-lane build targets (`b-targets`, `g-targets`, `b1053`). AGENTS.md
currently reads "never a gate target" for `C:`; that rule predates this measurement and is the
orchestrator's to change, in config and by PR, with the floor kept explicit (≥ 100 GB free, removed
by its creator). The alternative — leaving one slot on the HDD — is a standing 31 % tax on half of
all gates plus a machine-wide I/O stall.

**Interaction with L1:** these are not additive. L1 removes most of the 566 s build pass, which is
the part the disk was amplifying. Land L1 first, then re-measure L2 against the warm baseline.

### L3 — `cargo run` rebuilds the CLI under a different profile · 72 s · measured, 3/3 lenses survived

`Cargo.toml` declares `[profile.test] debug = 1`. The three schema stages
(`ci/gate.ps1:5241,5245,5249`) are `cargo run` invocations, which use the **dev** profile — a
different compiled unit from the test profile the `cli:` loop just built — so `cargo run`
recompiles 22 workspace crates to change one debuginfo level. `schema catalog` n = 358,
p10 51.7 s, median 73.2 s, p90 153.3 s; its two siblings, reusing the binary it just built, cost
0.7 s and 0.5 s. That asymmetry is the proof it is a build and not schema work.

**Change.** Add `--profile test` to those three lines. `--profile` exists on `cargo run` at the
pinned 1.97.1 (verified). It changes exactly one compiler flag for these commands
(`-C debuginfo=2` → `-C debuginfo=1`); feature resolution, `opt-level`, `debug-assertions`,
`overflow-checks` and `panic` are identical.

**Guard.** Parse the root `Cargo.toml` and fail unless `[profile.test]`'s key set is exactly
`{debug}` — this is what stops the saving from outliving its justification. Trap-guard it first: add
`opt-level = 1` under `[profile.test]`, prove the cell REDs and names the key; add a key under
`[profile.release]` as a uniqueness decoy and prove the cell stays GREEN.

**Do not** instead add `[profile.dev] debug = 1`. It would work, and it would silently degrade the
debuginfo every developer gets from a plain `cargo build` to solve a gate-only problem.

### L4 — cargo-nextest for the 882 s of test execution · ~357 s corrected (claimed 520 s) · inferred

`workspace tests` (493 s) runs ~30 test binaries one after another, each internally parallel; small
crates never fill 32 threads. The 55-suite `cli:` loop (389 s) re-runs the same 684 tests a second
time. A single global pool addresses both.

Three findings that reshape the proposal:

- **The isolation the loop is believed to add is not unique to it.** `cargo test --workspace`
  already runs each test *binary* as its own process. What the loop uniquely adds is a per-suite
  name and exit code in the manifest, and a different feature resolution.
- **What process-per-test would silently destroy, named.**
  `apps/cli/src/commands/serve/routes.rs:110` holds `static RUNS: Mutex<Vec<ThreadId>>` and `:3286`
  `static SERIAL: Mutex<()>`; `measured()` at `:3297` reads `runs().len()` before and slices
  `runs[before..]` after. That slice is only correct because `SERIAL` excludes concurrent appenders
  **from the same process**. Under nextest the tests still pass — *vacuously*. Same shape at
  `adapters/sealed-key-provider/src/lib.rs:106`. This must be resolved before nextest lands, not
  after.
- **Two of the four "slow" suites are not scheduling at all.** `cli: wake_http` (64 s) is six
  `#[cfg(windows)]` tests each waiting out `apps/cli/tests/wake_http.rs:3535`
  `RACING_WAKE_LEASE_SECONDS: u64 = 60`; measured, a full process-per-test run of that suite was
  62.13 s against 62.49 s today — nextest buys 0.36 s. `cli: amend_budget` + `cli: api_http`
  (73 s combined) are the first two suites alphabetically and carry the cold rebuild under the
  non-`--all-features` resolution.

**Sequenced.** (1) ~~Lower the 60 s lease~~ — **refused after reading the code; see L10.** (2) Measure the workspace block at 32 jobs on an idle box before committing to the
headline. (3) Pin nextest ≥ 0.9.85 (where `--no-tests=fail` is the default) in a versioned
`ci/tool-versions.json`, and make an absent or mismatched binary a **refusal**, never a fallback to
`cargo test`. (4) Write `.config/nextest.toml` with test groups — the six wake-lease tests and the
two pipe-namespace scans (`a_pipe_scan_that_misses_a_held_pipe_is_retried_not_believed`,
`an_old_fixture_pipe_cannot_satisfy_a_new_wake_wait_observer`, which enumerate the machine-global
`\\.\pipe\` namespace) capped, and the postgres `#[ignore]` set at `max-threads = 1`. (5) Fold the
JUnit report back into `$script:stageRecords` as one synthetic record per binary named exactly
`cli: <suite>`, so `.factory/MERGE-CHECKLIST.md` keeps reading the names it reads today — plus a
set-comparison against the suites discovered from `apps/cli/tests/*.rs`, so a suite present on disk
and absent from the XML is RED and a suite reporting `tests="0"` is RED.

### L5 — Stage scheduling · ~312 s corrected (claimed 500 s) · partly refuted

Nothing is skipped; only start instants move, so coverage is byte-identical. Ship as separate PRs:
the two PostgreSQL matrices moved to the build verdict (slack today); a declared
`$script:stageDependencies` **table as data, not a comment**, so a cell can read it; then `clippy`,
the cli loop and the schema stages converted to background children joined at their current call
sites.

The refutation that survives: `Start-BackgroundStage` is `Start-Process` on a separate OS process
and `Complete-BackgroundStage` returns only an exit code plus replayed text — there is no channel
for structured state. The cli loop appends to `$artifactManifest` as it goes, so moving *it* to the
background is not the free move the other stages are. ~302 s of the claimed 500 s depends on solving
that.

Required guard: a cell that walks the dependency table against a constructed stage-record set and
fails if any stage's `startedUtc` precedes its declared dependency's `endedUtc`, plus a sabotage
declaring a false edge (Studio depends on nothing) and asserting the checker reddens.

### L6 — `CARGO_INCREMENTAL=0` · 12 s · **measured**; `rust-lld` · 3 s · **refused**

Both halves were measured on this workspace after the plan was written, alternating arms with a cold
target each time, `cargo test --workspace --all-features --locked --no-run`, all arms on the same
NVMe (291 crates compiled, 267 binaries linked, 0 Fresh on every arm):

| setting | arm 1 | arm 2 | files left in the target |
|---|---|---|---|
| `CARGO_INCREMENTAL=1` | 77 s | 69 s | 30 403 (25 871 incremental) |
| `CARGO_INCREMENTAL=0` | 63 s | 58 s | **4 532** (0 incremental) |
| `link.exe` (default) | 52 s | 51 s | — |
| `rust-lld` | 50 s | 48 s | — |

**Incremental off ships**: ~12 s and **26 000 fewer files** per run, and the file count is the half
that matters most — a real target measures 23.1 GB across 68 364 files. It is set in `ci/gate.ps1`,
not in `.cargo/config.toml`, so an interactive edit loop keeps the setting it genuinely benefits
from. Guarded by `ci/gate-incremental.tests.ps1`.

**`rust-lld` is REFUSED, and the plan's ~103 s estimate for it was wrong.** It works at the pinned
1.97.1, it preserves backtraces (a panic in an lld-linked binary still named `src\main.rs:1:13`),
and it buys **~3 s** — about 0.2 % of a gate. That does not pay for a new failure mode in the one
instrument this project has, nor for a committed config that changes every developer's link step.
Recorded here so it is not re-derived. (`-C linker-features=+lld`, which reads like the modern
spelling, is unstable at 1.97.1 and refuses without `-Z unstable-options`.)

### L7 — Fail-fast on the 39 % of runs that are RED · ~940 s on those runs · measured

Today a red gate spends a median of 940 s after the answer is known. The fix is not to stop the
run — it is to stop *paying for stages whose result nobody will read*, while keeping the manifest
complete.

Set `$script:abortAfterStage` when a stage fails (default on; `-NoFailFast` restores today's
behaviour). The driver then appends, for every skipped stage,
`@{ name = $n; passed = $null; notRun = $true; notRunCause = ... }`. `passed = $null` is
load-bearing: `$false` fabricates a failure, `$true` fabricates a green, and **omitting the entry
makes "not run" indistinguishable from "does not exist in this build"**. Background children must be
killed and recorded, not orphaned — AGENTS.md's disk rules make an orphaned cargo a real hazard.

Every consumer of `stages[].passed` must be taught the new key in the **same** PR: `$null` is falsy
in PowerShell and `None` in Python, so a record shape that changes ahead of its readers is how a
green gets manufactured downstream.

### L8 — Parallel PowerShell suites · 0 s today · measured

`ci/run-ps-suites.ps1:105-129` runs 46 suites in a strictly serial `foreach`, median 457 s — but it
is started at t≈5 s as a background child and joined near the end, so **it is not on the critical
path today**. It becomes the binding floor the moment the path approaches 300–600 s. Rank it after
L1–L4, then land a 6-wide pool scheduled longest-first (`merge-proof.tests.ps1`, then
`gate-manifest-provenance.tests.ps1`).

The contract must survive exactly: 0 all passed / 1 a suite failed / 2 the harness could not vouch,
with 2 beating 1. A prototype built during this analysis returned **empty exit codes for all 46
suites** — an unreadable exit code must map to 2, never to 0. Cells: a suite forced to an unreadable
exit code must exit 2; a suite dropped from the ledger before the final set-comparison must exit 2
naming it; zero discovered must still exit 2.

### L9 — Jev (TypeSafe System One) · 0 s · REJECTED

**Jev must never decide which stages run.** `ci/select-scope.ps1`'s header states the doctrine:
*"a selector that guesses narrow is a selector that silently stops running the stage that would have
gone red, and the failure is invisible because the remaining stages pass."* A probabilistic skip is
exactly that failure, and the thresholds it would ride on
(`core/architect/src/judgment/policy.rs`, 0.80 / 0.35 / 0.65) are doc-commented
"VALUES TO BE MEASURED". All three review lenses agreed the rejection is correct.

The safe variant — asking Jev to *order* stages so a red is found sooner — changes no coverage, and
is still not worth building: it saves **0 s on a green run** (61 % of runs), and the deterministic
alternative is free. The failure-frequency table already exists in the 370 records:

> `workspace tests` 66 · PostgreSQL non-C collation 29 · PostgreSQL ignored matrix 23 ·
> `cli: api_http` 17 · `apps/studio` 15 · `schema baseline` 11 · `cli: runtime_http` 11 ·
> `ci powershell suites` 11 · `schema conformance` 11 · `cli: wake_http` 11

A deterministic reorder off that table beats a model on every axis — no network on the gate's
critical path, no API key, no uncalibrated threshold — and L7 (fail-fast) captures the same value
more directly. **Record the refusal as a named rejected alternative in `ci/select-scope.ps1`'s
header**, so the idea does not return each quarter.

Jev's shipped use — `graphhelm gate classify-red` (#1140, shadow mode) — is unaffected by this
rejection and remains the right home for typed judgment at the gate.

### L10 — Lower `RACING_WAKE_LEASE_SECONDS` · **refused after reading the code**

The investigation proposed lowering `apps/cli/tests/wake_http.rs:3535`
`RACING_WAKE_LEASE_SECONDS: u64 = 60` for ~55 s, on the reading that six `#[cfg(windows)]` tests
each wait it out. They do wait — but the constant is a **ceiling, not a chosen duration**, and three
things in the file say so:

1. `PIPE_STARTUP_HANG_CATCHER_SECONDS = 30` sits two lines above it, and the comment states the
   invariant outright: *"The lease must outlive this catcher so a failure here still diagnoses
   startup rather than an already-mature lease (#413)."* The lease has to cover sidecar startup on
   a loaded Windows box, which is what the 30 s catcher is sized for.
2. The same file already uses a short lease where a short lease is safe:
   `a_silent_deadline_reports_a_silent_receipt_not_just_silence` arms `Some(1)` — one second. So the
   60 s is not an unexamined default; it is the value the *racing* scenarios need.
3. `a_burned_but_unrung_lease_names_its_missed_ring_at_the_deadline` arms the lease, spawns the
   sidecar, waits for its pipe, arms a decoy and burns the lease — all of which must happen **before**
   the deadline, and then asserts `receiptReadAt == "deadline-once"`. Shorten the lease and the
   deadline can arrive during startup, which is #413's defect exactly.

A reduction that keeps a margin over the 30 s catcher (60 → ~40) buys ~20 s of a 1644 s gate while
walking back toward the failure the constant was widened to fix, on a file carrying six recorded
flake issues (#240, #413, #466, #955, #1129, #886). **Not shipped.**

The real fix is not a smaller number: it is to arm the short lease **after** `wait_for_pipe` rather
than before it, so startup stops living inside the lease's window. That changes what the test
exercises and needs its own issue and its own repetition evidence — 30 runs at the new value, idle
and under load, plus a sabotage setting the lease below the catcher to prove the invariant is
load-bearing.

---

## 3. Ordering, and the budget it buys

| # | change | saving | confidence | touches |
|---|---|---|---|---|
| 1 | L1 stop wiping the target | ~500 s | measured (existence proof, n=3) | `ci/gate-runner.ps1`, AGENTS.md |
| 2 | L3 `--profile test` on the three schema stages | 72 s | measured, 3/3 lenses | `ci/gate.ps1` (3 lines) |
| 3 | L2 second gate slot off the HDD | ~440 s, re-measure after L1 | measured, observational | `ci/gate-runner.ps1:77`, AGENTS.md |
| 4 | L6 `CARGO_INCREMENTAL=0` (`rust-lld` refused at 3 s) | **12 s**, and 26 000 fewer files | **measured** | `ci/gate.ps1` |
| ~~5~~ | ~~L4 step 1 — lower the 60 s wake lease~~ | **refused** — see L10 | the constant is a ceiling, not a wait | — |
| 6 | L7 fail-fast with `notRun` records | ~940 s on 39 % of runs | measured | `ci/gate.ps1` |
| 7 | L4 nextest | ~357 s | inferred | `ci/gate.ps1`, `.config/nextest.toml`, `ci/tool-versions.json` |
| 8 | L5 scheduling | ~312 s | partly refuted | `ci/gate.ps1` |
| 9 | L8 parallel PowerShell suites | 0 s now, floor later | measured | `ci/run-ps-suites.ps1` |
| — | L9 Jev | 0 s | rejected | `ci/select-scope.ps1` header only |

The savings are **not additive**: L1, L2, L4 and L6 all attack the same compile-and-link mass.
Realistic staging:

- **After 1–4 (configuration and one obsolete workaround, no change to what is proved):**
  the build pass falls from ~566 s toward ~65 s and the disk stops being the bottleneck.
  Expected band **800–900 s**. This is where the cheapest work ends.
- **After 5–8:** the 882 s of test execution is the remaining mass; nextest plus scheduling brings
  it to ~350–450 s and moves clippy and the schema stages off the path. Expected band
  **400–500 s** — inside the 300–600 s target.
- **Item 9** is what keeps it there once the path is short.

## 4. What this plan refuses to do

- **No probabilistic scope selection** (§L9).
- **No `docs/` exemption class** — `docs/` is compile-time and run-time input (§1.3).
- **No crate-scope widening as a performance lever** — 97.3 % of commits reach `graphhelm-cli` and a
  1-crate scope measured 4 % off `workspace tests` (§1.3). The one sound addition is
  `apps/studio/*` to `$KnownNonBuildInput`, which narrows exactly the PostgreSQL matrices.
- **No `--all-features` on the per-suite `cargo test`.** It is a **measured no-op**:
  `cargo tree --offline --locked -p graphhelm-cli --all-features -e features -i graphhelm-postgres-event-store`
  resolves `graphhelm-postgres-event-store` to `default` only, byte-identical to the same command
  without the flag, because `--all-features` activates features of the *selected* packages and
  `graphhelm-cli` declares no `[features]`. The duplicate resolution between
  `--workspace --all-features` (test-support on) and `-p graphhelm-cli` (test-support off) is real;
  this is simply not its remedy. L4 is.
- **No stage removed, no population narrowed, no absence allowed to read as a pass.** Every item
  above keeps its stage name in the manifest, and L7 introduces `notRun` precisely so that a skipped
  stage is recorded as skipped.
