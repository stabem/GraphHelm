# Blueprint — a load-confounded red must be VOID, not RED

D Agent, 2026-08-20. **Design only; no code.** Base named: `origin/main` @ **`d0b3f04`** — every count
and citation below re-derived at that ref with `git grep`/`git show`, never by `cd` into a shared
checkout. No cargo run for this document.

Feeds: #152 (run-manifest), #153 (kill bar item 1, verdict agreement), #166 (the gate refusing to run
under a bad precondition — same shape, and the precedent this borrows).

---

## 0. The problem, stated as what a future reader cannot do

A red arrives. Three causes are possible: **(A)** the change under test, **(B)** contention from a
foreign build, **(C)** a known flake. Today the record cannot separate them, so the response is
"discard and re-run" — which costs a full gate and depends on **someone remembering to ask whether
the machine was busy.** That is a sweeper made of attention, which this factory has now caught three
times in one day.

It bites #153 directly: kill-bar item 1 asks for verdict agreement 10/10, and a disagreement caused
by neither implementation nor product would spend the bar's budget on noise.

## 1. Measurement first: the fragile surface is 7 files, not 1 — and shape 3 is the reason this is hard

Enumerated at `d0b3f04`, by construct rather than by guess:

| construct | occurrences in `*/tests/*.rs` | can a red come from it? |
|---|---|---|
| `set_read_timeout` / `set_write_timeout` / `recv_timeout` | 13 / 9 / 6 | **yes — hard I/O deadline** |
| `thread::sleep` / `tokio::time::sleep` | 29 / 1 | mostly slows, rarely fails |
| `Duration::from_secs` / `from_millis` | 92 / 34 | only when used as a deadline |

Files carrying a **hard I/O deadline** (7): `apps/cli/tests/{api_http,gate_http,runtime_http,wake_http,resume_atomicity,resume_project_default}.rs`,
`adapters/model-gateway/tests/byok_adapters.rs`.

**Three failure shapes, and only the first two are honest about themselves:**

1. **Poll deadline** — `read_token`/`wait_for_health` (`gate_http.rs:81,94`): `Instant::now() + 5s`
   with `assert!(now < deadline, "no token file …")`. Fails with a message that names the cause.
2. **Hard I/O deadline** — `api_http.rs:411-412` (`set_read_timeout(5s)`). Fails as an I/O error;
   the storm flake is exactly this.
3. **Semantic-timing assertion** — `gate_http.rs:481`: `assert_eq!(resume_reply["data"]["status"],
   "completed", "…completes FAST: the model route hangs forever…")`. Under contention this fails as
   **"wrong status"**. The red contains **nothing about time.**

**Shape 3 is the design driver, and it is not hypothetical:** F reported a failure today at
`gate_http.rs:487`, which is a status assertion, in a file whose deadlines sit at 81, 94, 114-115.
A reader of that red sees a status mismatch and has no path back to a clock.

**Consequence that kills the obvious design:** you cannot make these reds self-identifying by
*annotating the tests*. The failing assertion is downstream of the timing and frequently carries no
temporal content. Any scheme that marks "timeout-sensitive tests" and expects the red to point at the
mark is answering the wrong question.

## 2. Therefore: make contention EXCLUDABLE at the run level, not identifiable at the assertion level

The record cannot tell you *which* cause fired. It can tell you *which causes were impossible*.

**If the run was provably exclusive, B is excluded** and the red is A or C — which is a decidable
question (re-run on the same commit; A reproduces, C does not). That is the whole leverage.

### The claim that matters is CONTINUOUS, not sampled at launch

A launch-time check would have passed every incident this factory hit today: my worktree was deleted
**mid-run**; F's build began **after** another run started. **Exclusivity is a property of the whole
interval**, so the manifest must record it as one:

```
exclusivity: {
  sampled_every_seconds: <n>,
  samples: <count>,
  foreign_build_processes_max: <n>,   # cargo/rustc/link.exe not descended from this run
  exclusive: <bool>                    # true ONLY if every sample was 0
}
```

`exclusive: false` is **not** a warning to read later. See §3.

### Supporting fields, in descending order of how much they buy

- **Per-stage duration against that stage's own baseline** (median of prior manifests for the same
  stage at any commit). A stage at 3× its median is a flag *even when green* — it is the earliest
  signal that a run was contended.
- **The fragile set, DERIVED at run time, never hand-listed.** Record the grep pattern and the files
  it matched. A hand list rots the moment someone adds a `set_read_timeout`; a recorded pattern lets
  a future reader re-derive and see the set has changed.
- **Slot lock state at launch and at exit** — held by whom, and whether it changed mid-run. Today's
  incident had the lock held by one agent while another process was building.
- **Free disk at launch and at exit on the target-dir volume.** `LNK1180` cost a full run today and
  was diagnosable only from the log body.

**Deliberately NOT proposed:** CPU percentage. It is noisy, hard to attribute, and answers "was the
machine busy" when the question is "was anything else building". Process presence is the honest
proxy and it is cheap.

## 3. The mechanism, which is the part worth arguing about

**A red from a non-exclusive run must not be recorded as RED. It must be recorded as VOID.**

Not "red with a caveat" — a distinct verdict, because a caveat is read by a human who may not read
it, and this factory has now paid three times for records whose reader was attention.

| condition | verdict |
|---|---|
| exclusive, all stages pass | **GREEN** |
| exclusive, a stage fails | **RED** — a result; attributable to A or C |
| **not exclusive, a stage fails** | **VOID** — not a result; the run is discarded by rule |
| not exclusive, all stages pass | **GREEN**, with `exclusive: false` recorded — contention cannot manufacture a pass |

This is exactly the discipline I applied by hand today three times (disk-full → HARNESS-BROKE, not
red; worktree deleted mid-run → uninterpretable, not red) — and doing it by hand is precisely what
should not be required.

**Refusal at launch** (the #166 shape) covers the easy half: if foreign builds are alive when the
gate starts, refuse rather than produce a run nobody can use. **Refusal cannot cover mid-run
arrival**, and aborting a 15-minute run because someone else started a build is worse than finishing
and stamping VOID — the artefacts are still useful for everything except the verdict.

**The asymmetry is deliberate and load-bearing:** contention can turn a green into a red; it cannot
turn a red into a green. So a contended PASS is still a pass, and only a contended FAIL is void. A
design that voided both would be discarding good news for symmetry's sake.

## 4. Trap-fixture, constructed before the seal (house rule)

The fixture must be buildable or the design is wrong, and this one is:

**Arrangement.** On a held slot, start the gate; once it is past the first stage, deliberately launch
a foreign `cargo build` in another directory. **Expectation:** the manifest records
`exclusive: false` with `foreign_build_processes_max >= 1`, and if any stage fails the run is stamped
**VOID** rather than RED.

**Sabotage that must make it fail (the discriminator):** make the sampler read only at launch.
The fixture must then go **GREEN-and-exclusive**, wrongly, because the foreign build began after the
single sample. If that sabotage does *not* flip the outcome, the sampler is decoration and the whole
continuous-claim argument is unpaid.

**Positive control:** a run with no foreign build must record `exclusive: true`, or the sampler is
reporting contention that is not there and every future VOID is suspect.

## 5. What this does not establish

- **Nothing measured for the mechanism.** The counts in §1 are real greps at `d0b3f04`; every claim
  about how the manifest would behave is design, not observation.
- **It does not identify cause C.** Excluding B leaves A-or-C, decided by re-running on the same
  commit — this design makes that question *askable*, not answered.
- **The classification of the 7 files is by construct, not by proof.** That a file contains
  `set_read_timeout` does not prove any of its tests fail under contention; it proves the mechanism
  is present. The only *observed* instance is F's `gate_http.rs` red today, and I did not reproduce
  it.
- **Whether #152's manifest can carry these fields without schema churn** is not assessed here; that
  belongs to whoever implements it.
- **Baselines need a corpus.** Per-stage medians are worthless until enough manifests exist, and the
  first runs will have no baseline to compare against. That is a delay, not a defect, but it should
  not be discovered later.
