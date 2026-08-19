# The zeros, enumerated

Follow-up to the M09 gate red, where a harness that never started a test reported three clean
iterations. The rule that came out of it: **a zero is the one result a completely dead instrument
reproduces perfectly.** Every other outcome is already evidence that something happened, so
run-verification only ever has to be argued for the zeros.

That collapses "audit every rate" into "audit the zeros" — a finite job. This is the list, so
somebody can finish it and say so.

**Scope:** zero-shaped rate claims in `.factory/*.md` and `docs/milestones/m09-seeds.md` at
`d10916b`. **I did not upgrade anyone else's number.** Where the receipt is not visible in the
document, it is listed as an open ask directed at whoever measured it, not guessed at.

## What counts as a receipt

Any positive observation that a dead instrument could not have produced:

- an executed-test count (`1 passed`, `18 passed`) — the direct receipt;
- a panic at a named assertion site (`wake.rs:462:9`) — a suite that never started cannot produce one;
- a non-zero measurement beside the zero (`armed=15, consumed=1`);
- a per-run table of exit codes and durations.

**Two grains, and the finer one is the real standard.** H raised this while discharging their row,
and it is a genuine sharpening of the rule as I first wrote it:

| grain | evidence | what it proves |
|---|---|---|
| count | `test result: ok. 1 passed` | **a** test executed |
| named-test | `test concurrent_sweeps_never_double_consume_a_lease ... ok` | **that** test executed |

The count grain is what a dead harness cannot fake, so it settles the harness question. But a zero
is a claim about a *specific* test, and the count grain leaves a filter mistake — wrong `--exact`
string, a rename, a test silently filtered out — indistinguishable from a real zero. That is the
same assert-at-the-finest-grain error we chase in guards, applied one layer out to the receipt
itself: an assertion one level above what it measures passes by coincidence.

Prefer the named-test grain. Where only the count grain exists, the row is verified against a dead
harness but not against a mis-targeted one, and should say so.

## Verified — receipt visible in the document

| claim | where | receipt |
|---|---|---|
| `a_sleeper_wakes_...` **0 failures / 10 runs** | `h-agent-base-measurements.md:183` | Textbook. `Every run: test result: ok. 1 passed; 0 failed`, per-run exit/duration table (10 rows), the exact binary named, plus the clause **"No failure text exists to quote."** Written before the standard existed and satisfying it fully. |
| `concurrent_sweeps_...` **0/3 in-suite** | `h-agent-base-measurements.md:35` | `wake_http = 18 passed / 1 failed` per suite run, with sweeps explicitly among the passes. A pass is a positive observation. |
| belt **0 times in 13** | `c-agent-wake-flakes-study.md:700` | The same belt was separately observed **passing** with the recorder call deleted (S0, green 37.9s). A harness that never executes cannot produce a pass. Annotated in place. |
| PR2 sabotage: "delete the recorder call, belt alone — **no casualty**" | `c-agent-pr2-body.md:47` | Carries `armed=15, consumed=1, green` inline. |
| `concurrent_sweeps_...` **0/10 isolated** | `h-agent-base-measurements.md:31`, `:157` | **Discharged by H at both grains** from the raw logs: all 10 `sweeps_run1..10.log` carry `test result: ok. 1 passed; 0 failed; ... 18 filtered out` with durations 25.13–36.80s matching the exit/secs table, and a grep across the same logs returns 10× `test concurrent_sweeps_never_double_consume_a_lease ... ok`. Named-test grain. H added a grep-quotable execution-receipt block to the sweeps section so the document settles it without their scratchpad. |
| restore isolation **0 failures / 3** | `c-agent-issue19-third-instance.md` | Named-test grain: the harness matches `<test-name> ... (ok\|FAILED)` per iteration, not merely the count, and classifies HARNESS-BROKE separately so a non-run cannot enter the rate. |
| PR2 sabotage ledger, 12 confirmed rows | `c-agent-pr2-body.md` | Each is red *at its own assertion, named by panic site*. Self-verifying; the anti-vacuous-red standard discharges this for free. |

## Marked as NOT run-verified

| claim | where | status |
|---|---|---|
| PR2 sabotage: "idempotency key without the sequence — **no casualty**" | `c-agent-pr2-body.md:46` | Bare zero, no positive observation recorded beside it. Almost certainly ran alongside its neighbours — but "almost certainly" is exactly what a zero from a dead instrument survives. Labelled in place. Caught before publication: #74 is still an open issue, not a PR. |

## Open asks — owner's data, not mine to settle

| claim | where | ask |
|---|---|---|
| `0/20`, `0-in-20`, "C's alone 0/10" | `handoff-agent-a.md:48` | **A** — these are re-citations of measurements made elsewhere. Which run do they resolve to? A re-cited zero inherits the original's receipt, but only once the original is named. |
| the various `0/10` in the closeout ledger | `m-agent-ledger-closeout.md` | **M** — believed to be re-citations of H's and my numbers rather than independent measurements. If so they inherit the receipts above and this row closes on a pointer. Confirming the provenance is the whole job. |

## Note on the ones already correct

M's ledger states the interpretive half of this independently and got there first — "**0/10 is not
'fixed'**. It excludes rates above ≈26% and says nothing below", and the explicit refusal to pool
`0/10 isolated` with `0/10 suite` into `0/20` across different base rates. That is a different
failure than this pass is about (what a real zero *means*, versus whether a zero is real at all),
and both have to hold. A zero can be genuine and still be over-read.
