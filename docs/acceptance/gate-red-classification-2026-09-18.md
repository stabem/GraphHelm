# The first real gate red classification, and what the excerpt was costing it (#1149)

`graphhelm gate classify-red` (#1140, shadow only) was run against a real RED gate log with a real
TypeSafe System One key, before and after the excerpt fix in #1149. **The two runs differ in the
excerpt and in nothing else this run could control** — same log, same known-flake list, same route,
same *requested* model — so the difference in the answer is attributable to the evidence the judge
was shown.

**One thing is NOT pinned and these rows cannot claim otherwise** (`eloquent-jones`, #1153):
`jev-latest` is a request *tag*, not a resolved version, and the runs are separated in time.
`JudgeReply` carries `pub model: String` — the provider says which model answered — and nothing
writes it into the record. So "same model" is an assumption about the provider's routing rather
than a measurement, and two rows taken hours apart may not be comparable. It costs nothing while
this is shadow. It is the difference between comparable and non-comparable rows in the confusion
table this is building toward, and it must be recorded before any table is used to move a
threshold.

## The subject

    log          D:\graphhelm-slot\runner\1128-20260917T005738.log   (UTF-16, 1,239,516 bytes)
    known flake  #886  eof_arriving_after_the_deadline_is_not_silently_accepted
                       20 ms deadline race in apps/cli/tests/api_http.rs; passes on a re-run
    route        judge -> typesafe, jev-latest, https://api.typesafe.ai
    real failure exactly one stage: workspace tests

## What the log actually contains

Counted on the file itself, decoded as UTF-16 (a byte grep over this file returns zero — the
runner's transcripts are UTF-16 with a BOM):

    [gate] FAILED: lines = 8
      [gate] FAILED: workspace tests (exit 101)                  <- the real one
      [gate] FAILED: background stdout evidence (exit 2)
      [gate] FAILED: background stderr evidence (exit 2)
      [gate] FAILED: background silent failure (exit 2)
      [gate] FAILED: stdout evidence (exit 101)
      [gate] FAILED: information stream evidence (exit 101)
      [gate] FAILED: stderr evidence (exit 101)
      [gate] FAILED: no output at all (exit 101)

    [gate] RED - failed stages: lines = 1
      [gate] RED - failed stages: workspace tests

Seven of the eight are `ci/gate-*.tests.ps1` cells that fail **on purpose**, to prove the gate
reports failures. They print the gate's own vocabulary into the gate's own log.

## The two runs

    BEFORE  (excerpt reads every printed line: eight failed stages)
      class known_flake   confidence 1.00   sameAs []      contradicted []
      acts  false         unresolved true
      judgeUsage {inputTokens: 1683, outputTokens: 78}

    AFTER   (excerpt takes the banner: one failed stage)
      class known_flake   confidence 1.00   sameAs [886]   contradicted []
      acts  true          unresolved false
      judgeUsage {inputTokens: 1654, outputTokens: 78}
      excerptDigest 4bf1c37b45222b88f222d32f42d4c5a63bc86c53c1b73a0b7db07e1a003b2140

**The class was right both times.** What moved is the `same_as` Noul: with eight unrelated stages
in the evidence the judge would not say the failure was #886, and #1140's silence rule correctly
refused to act on a classification that named no flake. With one stage it named #886 and acted.

## What this does and does not establish

**Does.** The excerpt was the cause of the hedge, not the threshold. The hypothesis in #1149 was
that "eight unrelated stages failed" does not read like one flaky test; the measurement agrees.
And #1140's silence guard behaved correctly on its first live case in **both** directions — it
refused to act on contaminated evidence and acted on clean evidence, without either outcome being
tuned for.

**Does not.** It cannot support a threshold change: `NOUL_YES_THRESHOLD`, `ACT_THRESHOLD` and their
siblings in `core/architect/src/judgment/policy.rs` remain placeholders, and the rule in
`docs/acceptance/architect-judgments-recipe.md` — that a table with fewer than ten rows does not
support a change — is not relaxed by three.

The classification remains **shadow**: it changes no verdict, selects no stage, counts no pass and
re-queues nothing.

## Rows 2 and 3 — two `environment_void` runs with known ground truth

The row above is a `known_flake` and a confusion table needs the other classes. Two came for free
the same afternoon: the gate's target drive (`E:`, 112 GB, five ~20 GB cargo targets) hit zero and
reddened two runs twenty-eight minutes apart. **Both are `environment_void` by construction — the
drive was measured full at each run's timestamp — and they are not alike.**

| # | log | shape | class | conf | acts | unresolved |
|---|---|---|---|---|---|---|
| 1 | `1128-...T005738` | one flaky test, clean excerpt | `known_flake` | 1.00 | yes | no |
| 2 | `1153-...T150510` | 59 of 71 red, `os error 112` in every tail | `environment_void` | 0.92 | yes | no |
| 3 | `1152-...T152525` | 4 of 72 red, `LNK1318 ... FILE_SYSTEM (3)` | `environment_void` | 0.63 | **no** | **yes** |

    row 2  excerptDigest 3f952c08...  usage {in 1871, out 77}
    row 3  excerptDigest f26ce50b...  usage {in 1673, out 77}

**Row 3 is the hard case and it was predicted to fail differently than it did.** That run's cause
never enters the excerpt: `LNK1318` is not a `[gate]` line, not a test, not a panic, and not the
*first* `error:` — it is the `= note:` two lines under `error: linking with link.exe failed`. So the
judge is shown four stages, three of them in `schema`, with the environmental evidence filtered out.
Filed on #1138 as a prediction that the judge would be misled into a code-shaped class.

**It was not.** It answered `environment_void` — the right class — at **0.63**, below
`ACT_THRESHOLD` (0.80), so `unresolved` is true and nothing acts. The prediction was wrong about the
failure mode and right about the evidence being thin: **the threshold caught what the excerpt lost.**
Recording the wrong half explicitly, because a prediction quietly restated as a success is how a
calibration table stops being evidence.

So the silence rule has now behaved correctly on every live case it has met: refused on contaminated
evidence (row 1 before the fix), acted on clean evidence (row 1 after), acted on abundant
environmental evidence (row 2), and refused on thin environmental evidence (row 3). None of those
four outcomes was tuned for; the thresholds were written before any of this was measured.

**What three rows still do not buy.** No negative row where the true class is `real_defect`, and
none where the judge is *wrong* — so nothing here measures a false positive, which is the direction
`ACT_THRESHOLD` exists to guard. Rows 2 and 3 also share one cause, so they are two observations of
one situation rather than two independent ones.

## Reproducing it

    $env:GRAPHHELM_GATEWAY_KEY = (Get-Content -Raw '<project>\.graphhelm\serve.key').Trim()
    graphhelm gate classify-red `
      --log '<a RED runner log>' `
      --known-flakes '<[{"issue":886,"test":"...","summary":"..."}]>' `
      --manifest '<project>\.graphhelm\manifest.json' --judge-route judge `
      --broker '<project>\.graphhelm\broker' --keyring '<project>\.graphhelm\keyring' `
      --key-id studio `
      --out '<a path that does not exist yet>.json'

The route, the key and the broker are the ones `graphhelm gateway setup --provider typesafe`
(#1139/#1141) provisions; the key is never an argument and never reaches the record.

Refs #1138 #1140 #1149 #886.
