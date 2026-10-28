# drael-v1 do-less three-arm battery — 2026-10-26

**Status.** Instrument pilot run with `drael/drael-v1` as the executor model. It produces the first
score-vs-cost table for the public starter set on this executor and repairs/isolates three failure
modes in the measurement instruments. It does **not** yet establish a methodology win: the
Keel-armed baseline sits inside the noise band of the un-armed baseline, and the headline cost gap
rests on one suspected mis-graded task. Read §What this run does and does not establish before
quoting numbers.

**Follow-up.** A second battery on the same executor and task set, this time against external
spec-driven methodologies (Spec Kit `s`, OpenSpec `o`), is reported in
[`CROSS-METHOD.md`](CROSS-METHOD.md) (data in [`cross-method/`](cross-method/)). Headline: the Keel
surfaces stay cost-neutral-to-cheaper and quality-neutral-to-better against both chains, while the
multi-stage chains are the expensive side (≈9–11× $/proven after instrument corrections).

## 1. Claim under test

Per [BENCHMARK_PROTOCOL.md](../../BENCHMARK_PROTOCOL.md): *"equal or better delivered-code quality
at lower total cost per proven delivery."* Arms compared on identical frozen tasks:

| Arm | Surface given to the agent |
|---|---|
| `a` | Task prompt + card only (baseline) |
| `k` | Baseline + pinned Keel surfaces (skill REFERENCE.md + AGENTS.md Keel section + `policies/keel.yaml`) |
| `d` | Baseline + short digest (`tools/token-bench/doless/surfaces/keel-digest.md`) |

Prompt style: `explicit`. Scoring: hidden oracle + regression observer + Keel check + do-less
surface columns. A run is **PASS** only when every blocking column is clean *and* the oracle and
regression observers pass. **INCOMPLETE** runs count toward cost, not toward score. Cost of a
failed run is never reported as a saving.

## 2. Frozen controls (design)

- **Executor:** `opencode` CLI v2.1.4, sha256 `b12b0613e9b4a491e5b12d90a5e9bbae6d261fe9688c82bf73125a036963d399`;
  invoked as `opencode run --auto -m drael/drael-v1 --format json`. Requested and reported model:
  `drael/drael-v1`. Runtime checker: `graphhelm.exe` sha256
  `14b1c732d95bf1e1f8b528266ad58c4019e803e3ae18db9829386cd573f117df`.
- **Tasks:** the 7 frozen battery tasks in `tools/token-bench/doless/tasks.json` (doless v2),
  qualified green pre-battery on the same tree. Parent SHAs per task are recorded per run in
  [`runs.json`](runs.json); the battery checkouts are frozen, so commits that landed on `main`
  afterwards do not affect the results.
- **Surfaces:** arm-`k` digests — REFERENCE.md
  `sha256:a1ad951f…876986100`, AGENTS.md Keel section `sha256:1c78f2c9…41e50a`,
  `policies/keel.yaml` `sha256:9671bf52…3eb68e6c` (identical on every arm-`k` row and identical to
  current repo state); arm-`d` digest `sha256:5076244f…4833e885`. Full set:
  [`control-digests.json`](control-digests.json).
- **Plan:** 30 runs, single Windows host, sequential, counterbalanced arm order (`a,k,k,a` rotated);
  window `2026-10-26T13:49:20Z → 16:12:36Z` (~2 h 24 min wall); timeout cap 1500 s per run.
- **Ledger:** token usage read from the OpenCode session database (`session_v2`, recursive over
  `parent_id` so subagents are included). `tools/token-bench/doless/results.jsonl` stays
  untracked by design; this directory is the committable evidence record.
- **Cost labeling:** Drael sessions are subscription-metered (observed billed $0.00). All USD
  figures here are list-price **estimates** at $0.50/M input+cache-read and $1.50/M
  output+reasoning+cache-write, labeled `costUsdEstimated`. The canonical ledger carries no USD
  for Drael rows, which is why `doless.py table` prints `-` in its USD column for them (§13).
- **Plan caveats (read before quoting):**
  1. Arm `a` ran **once** on `docs-config-risk` (its first slot was consumed by the pre-battery
     smoke run and is excluded from the ledger).
  2. Arm `d` ran **only on the 3 hardest tasks** (`docs-config-risk`, `py-mcp-meta`,
     `rs-signal-id-retry`) — selection-biased by construction; its aggregate numbers are not
     comparable to `a`/`k` on that account.
  3. One row (`docs-config-risk` arm-`k` run 1) was **reconstructed after the fact** from the
     battery log plus the session DB after the log driver dropped line 0 on a UTF-8 BOM parse miss
     (defect #1, §9). It is flagged in `runs.json` and is **not** in `results.jsonl`, so canonical
     commands see 29 Drael rows while this report analyzes 30.

## 3. Headline results

Computed over all 30 runs (reconstructed row included). Score excludes INCOMPLETE; cost includes
it. CI: Wilson 95% over scored runs.

| Arm | Runs | P/F/I | Score (95% CI) | Tokens total | Tokens/run (in+cache / out+reason) | Est. USD | Est. $/run | Est. $/proven | Wall (mean / median / max, s) | Timeouts |
|---|---|---|---|---|---|---|---|---|---|---|
| `a` | 13 | 8/4/1 | 8/12 = 67% [39%, 86%] | 18,881,880 | 1,441,633 / 10,819 | 9.5816 | 0.7370 | 1.1977 | 280.6 / 134.2 / 1345.3 | 0 |
| `k` | 14 | 9/5/0 | 9/14 = 64% [39%, 84%] | 15,488,355 | 1,096,183 / 10,128 | 7.8860 | 0.5633 | 0.8762 | 260.0 / 156.3 / 953.1 | 0 |
| `d` | 3 | 0/2/1 | 0/2 = 0% [0%, 66%] | 6,097,442 | 2,021,769 / 10,712 | 3.0809 | 1.0270 | n/a (no proven) | 350.9 / 122.2 / 855.8 | 0 |

**Pareto reading (protocol rules).** With list-price estimates: `a` and `k` are **both on the
frontier** — `a` is dearer but scores higher; `k` is cheaper but scores lower; neither dominates.
`d` is **dominated** (lower score and higher cost than either). On the canonical ledger basis
(no billed receipts for subscription runs) all three Drael arms print as `unplaced` (§13). The
confidence intervals for `a` and `k` overlap heavily, so the ranking between them is **not
settled** at n=13/14; per protocol this reads as *parity within noise*, not as a win for either.

## 4. Per-task verdict map (Drael)

Verdicts per arm/run in score order; tokens and est. USD summed over the arm's runs on that task.

| Task (split) | Arm `a` | Arm `k` | Arm `d` |
|---|---|---|---|
| docs-config-risk (train) | FAIL — 330,897 tok, $0.171 | FAIL, PASS — 378,682 tok, $0.195 | FAIL — 251,068 tok, $0.128 |
| docs-doc-comments (test) | PASS, PASS — 956,271 tok, $0.490 | PASS, PASS — 911,761 tok, $0.474 | — |
| om-win-arm64 (test) | PASS, PASS — 757,701 tok, $0.395 | PASS, PASS — 2,299,390 tok, $1.175 | — |
| py-actor-session-header (train) | PASS, PASS — 1,159,699 tok, $0.590 | PASS, PASS — 1,157,749 tok, $0.593 | — |
| py-mcp-meta (train) | FAIL, FAIL — 1,250,921 tok, $0.634 | FAIL, FAIL — 1,067,411 tok, $0.545 | FAIL — 882,943 tok, $0.448 |
| py-sessionend-cap (test) | PASS, PASS — 2,359,184 tok, $1.203 | PASS, PASS — 1,846,914 tok, $0.942 | — |
| rs-signal-id-retry (train) | INCOMPLETE, FAIL — 12,067,207 tok, $6.097 | FAIL, FAIL — 7,826,448 tok, $3.962 | INCOMPLETE — 4,963,431 tok, $2.505 |

Every PASS run is a genuine oracle+regression+surface pass. Every FAIL/INCOMPLETE carries its
machine reason in `runs.json`; the three recurring failure modes are isolated in §5.

## 5. Failure modes (three)

### 5.1 `py-mcp-meta` — the "added one test" habit (all arms, both models)

All 5 Drael runs (and 5 of 6 historical Codex runs) made the **correct core fix** (the oracle
passes in every case: `_meta` accepted in both `plugins/graphhelm/hooks/task_handoff.py` and
`plugins/graphhelm-codex-hooks/hooks/task_handoff.py`) and then added exactly **one test the card
did not request**, landing `unrequestedNewTests=1` and blocking PASS. This is a cross-model agent
habit, not a Keel effect: arm `a` fails it 2/2, arm `k` 2/2, arm `d` 1/1. The blind reviewer read
the added test as a legitimate regression test (§6, disagreement D1) — the do-less column exists
precisely to charge the habit, so the scorer stands. This is the natural hillclimb target (§12).

### 5.2 `rs-signal-id-retry` — suspected mis-grader (constant across models)

The oracle (`oracles/rs_signal_id_retry.py`) requires a **single line** containing both
`signalId` and `assert`. The idiomatic multi-line `assert_eq!(…)` fix fails the check: 5/5 Drael
runs and 6/6 historical Codex runs are `oracle_fail`. Per protocol, a task failing every run in
every arm is *suspected ambiguous/mis-graded*, and the evidence supports that label. On top of the
mis-grader, this task produced the battery's only substantive defects:

- **`k` run 1:** regression observer failed (rustfmt on `api_http.rs`) and the blind reviewer found
  the diff is **test-only** — `serve/mod.rs` was never changed, so the retry `signalId` the new
  assertions demand is never emitted and the test is red on the delivered tree (reviewer R11, C3
  false). The one materially defective delivery of the battery, correctly marked FAIL.
- **`a` run 1 and `d` run 1:** fixes introduced `panic!`/`expect()`, which made `keel check`
  refuse with *"refused for a rule the do-less columns do not score: `keel.rel.no_panic`"* →
  INCOMPLETE. This is an **instrument gap** (defect #2, §9), not a scoring outcome; note the same
  run, `a` run 1, also drew the blind reviewer's *bloated* verdict (a `RetryProof` struct rippling
  through four functions).

### 5.3 `docs-config-risk` — placement-sensitive oracle

The oracle checks **table rows only** (lines starting `|`) for the phrase
`behavio[u]?ral evidence` in the required cells, plus specific config-row checks. Arm `k` run 2 put
the phrase inside the expanded table cell → PASS; `a` run 2, `arm d` run 1, and reconstructed `k`
run 1 put the same phrase in adjacent prose → FAIL. The three near-identical diffs are
**indistinguishable to blind review** (R06/R07/R08 all score clean on every claim; disagreement
D2). Historical Codex was mixed on this task (a 2/4, k 2/3), so it is placement-fragile rather than
constant — but the cell-only search is defect #4 (§9).

## 6. Blind review

Method: 14 anonymized exhibits ([`blind-pack/`](blind-pack/)) reviewed by an isolated subagent
session (`ses_e86b60980ffeZ2M3kvuMtxrfL2`) holding the repo read-only, seeing no arm labels, run
numbers, or scorer verdicts, against the protocol's 8-claim rubric. Full verbatim table and
findings: [`blind-review.md`](blind-review.md).

Summary:

- **13 of 14 exhibits true on all eight claims.** The single `false` is **R11** (= `rs-signal`
  arm-`k` run 1, C3): test-only diff, producing code absent, assertion red — the one materially
  defective delivery, already FAIL-scored (agreement between independent signals).
- **No critical finding touches any of the nine PASS runs**; no proven delivery is revoked.
- **Caveat noted:** on R02 the written header key is `X-GraphHelm-…` while the assertion checks
  `X-Graphhelm-…` (consistent with the pre-existing sibling assertion, likely suite convention,
  unresolvable from the diff).
- **Review-vs-scorer disagreements recorded, not averaged:** the mcp added test reads as a real
  regression test (scorer stands — the column charges the habit); the dcr PASS and the two dcr
  FAILs are invisible to review (only the hidden oracle separates them).

## 7. Token-economy diagnostics

**The output side is at parity; the entire difference between arms is context consumed.**
Output+reasoning averages 10.8K / 10.1K / 10.7K tokens per run for `a` / `k` / `d` — within ~7%
across arms. The headline token delta lives in input+cache-read: 1.442M (`a`) vs 1.096M (`k`) per
run (−23.8%), i.e. estimated $0.737 vs $0.563 per run and $1.198 vs $0.876 per proven delivery
(−26.8%). Caveats per protocol: subscription estimate, overlapping score intervals, and different
task mixes (`k` ran the extra `docs-config-risk` second run; `a`'s mix includes the 4.26M-token
INCOMPLETE `rs` run).

**Sensitivity — excluding `rs-signal-id-retry`** (the task with the suspected mis-grader):

| Arm | Scored | Pass | Rate | Est. cost total | Est. $/proven |
|---|---|---|---|---|---|
| `a` | 11 | 8 | 72.7% | 3.4841 | 0.4355 |
| `k` | 12 | 9 | 75.0% | 3.9238 | 0.4360 |

Once the fragile task is out, the per-proven dollar gap **closes to zero** (k is a tenth of a cent
*higher*), while quality stays parity-with-k-nodding-ahead. The headline "$ saving from Keel" is
currently a one-task artifact concentrated exactly where the grader is suspected broken.

**Balanced comparison (the five tasks where both arms ran 2×):**

| Task | Arm `a` tokens | Arm `k` tokens | Δ (k vs a) |
|---|---|---|---|
| docs-doc-comments | 956,271 | 911,761 | −4.7% |
| py-actor-session-header | 1,159,699 | 1,157,749 | −0.2% |
| py-mcp-meta | 1,250,921 | 1,067,411 | −14.7% |
| py-sessionend-cap | 2,359,184 | 1,846,914 | −21.7% |
| om-win-arm64 | 757,701 | 2,299,390 | **+203%** |
| **sum** | **6,483,776** | **7,283,225** | **+12.3%** |
| **sum, excl. om-win-arm64** | 5,726,075 | 4,983,835 | **−13.0%** |

On directly comparable tasks, `k` is cheaper on four of five and more expensive on
`om-win-arm64`, where it spent **3.0× more** context (both runs PASS; +1.54M tokens). That single
outlier flips the summed balance from −13.0% to +12.3%. Until the `om` spike is understood
(defect #5), the honest statement is: **the direction of the token effect on comparable tasks is
not settled; it is a coin weighted by one unexplained outlier.** Do not quote the −23.8%/−26.8%
headline figures without this section.

**Never-counted-as-savings (per protocol):** `py-mcp-meta` arm totals (all FAIL — `k`'s lower
spending there is not a saving), the 2 INCOMPLETE runs (cost counted, score not), and arm `d`'s
low dollars (it proved nothing).

## 8. Reliability

- **30/30 runs reached a terminal verdict.** Zero infrastructure losses, zero agent errors
  (the historical Codex ledger had 1 `agent_error`; Drael had none).
- **0/30 timeouts** against the 1500 s cap; observed max 1345.3 s (the bloated `rs` arm-`a` run).
- **Token ledger complete:** every run has full usage from the session DB, subagent subtrees
  included. One row reconstructed post-hoc and flagged (§2 caveat 3).
- **INCOMPLETE rate 2/30** (a 1/13, k 0/14, d 1/3), both traced to the *same instrument gap* — the
  `keel.rel.no_panic` refusal (defect #2) — not to model behavior. If that rule were mapped, both
  runs would be scored FAIL (their oracles also failed).
- **Determinism of the freeze:** surface digests identical across all arm-`k` rows; prompt style
  constant; checkouts pinned to per-task parent SHAs; repo HEAD moved during/after the battery
  (commits through `#2116`) without touching frozen checkouts.

## 9. Instrument defects found (actionable, ordered)

1. **UTF-8 BOM in `results.jsonl`.** A leading BOM (i) broke `doless.py table` (strict `utf-8`
   reader) and (ii) made the driver's own log parse drop line 0, forcing the `docs-config-risk`
   `k1` reconstruction. Fix: write without BOM / read with `utf-8-sig`; both sides, in
   `tools/token-bench/doless.py` and the driver.
2. **Unmapped `keel check` refusal → spurious INCOMPLETE.** *"keel check refused for a rule the
   do-less columns do not score: `keel.rel.no_panic`"* hit 2/30 runs. Either map the rule to a
   scoring column or declare it non-blocking with a structured reason.
3. **`rs-signal-id-retry` oracle fragility.** Single-line co-occurrence of `signalId` + `assert`
   rejects idiomatic multi-line assertions; 11/11 oracle_fail across two models. Repair to
   multi-line-aware assertion detection or retire the task.
4. **`docs-config-risk` oracle placement.** Phrase search restricted to table rows makes prose
   placement of an otherwise correct edit a hard FAIL that blind review cannot even see. Broaden
   the search window (adjacent paragraph) or state the placement requirement in the card so agents
   can satisfy it intentionally.
5. **`om-win-arm64` arm-`k` context spike.** 3.0× the arm-`a` tokens on a "hold the line" task,
   both runs passing. Unexplained; watch item for the next round (candidate mechanism: the larger
   surface changes exploration budget on a task that asks for almost no change).

## 10. Cross-model context (historical Codex rows, verdicts only)

`results.jsonl` also carries 37 historical Codex rows (2026-10-12, doless runner digest
`e5445c28…743d2`, model gpt-6-sol). Different runner generation, **no token ledger** — they inform
verdict patterns only and are excluded from every token/cost figure above. From the canonical
pareto (§13): `codex:a` 10/18 = 56% [34%, 75%] and `codex:k` 10/18 = 56% [34%, 75%] — both on the
frontier, again parity. The failure **signatures reproduce on the second model**: `py-mcp-meta`
`unrequestedNewTests` (a 2/3 F, k 3/3 F), `rs-signal-id-retry` 6/6 `oracle_fail` (both arms),
`docs-config-risk` mixed (a 2/4, k 2/3). What Drael adds to the pattern: the `keel.rel.no_panic`
INCOMPLETE class (2 instances) and the cleanest blind-review profile yet (one materially defective
delivery in 30 runs).

## 11. What this run does and does not establish

**Establishes**

- The do-less instrument works end-to-end on a **third executor** (OpenCode host + Drael model)
  with a complete usage ledger, counterbalanced arms, and frozen controls — the battery's machinery
  is portable beyond the original runner/model pair.
- First **score-vs-cost table** for the public starter set on this executor, including the first
  arm-`d` (digest) data points: 0/3 proven on the hardest subset.
- Baselines for hillclimbing: 67% / 64% pass rates — **below the protocol's 80–95% hillclimb
  band**, so the correct next move is repairing the instruments (§9), not climbing.
- Three isolated failure modes with root causes: one cross-model agent habit (5.1), one suspected
  mis-grader (5.2), one placement-sensitive oracle (5.3).
- Cross-model reproducibility of the do-less verdict patterns (§10).

**Does not establish**

- **Quality improvement from Keel.** 64% vs 67% with overlapping CIs is parity within noise; the
  only task separating the arms on quality is the placement-sensitive one (5.3), and the battery's
  one materially defective delivery sat in arm `k`.
- **A realized cost win.** The −23.8%/−26.8% headline depends on `rs-signal-id-retry` (suspected
  mis-grader); on the five balanced tasks the sign flips depending on whether the unexplained
  `om-win-arm64` k-spike counts (§7). Cost figures are list-price estimates, not receipts.
- **Arm `d`'s standing.** n=3, hardest-tasks-only selection, zero proven; dominated this round,
  but not measured to settle that.

## 12. Next steps (ordered)

1. Fix defects #1–#4 (BOM emit/parse, `no_panic` mapping, `rs` oracle, `dcr` oracle) — small
   changes in `tools/token-bench/` and `oracles/`, each with the tests it reaches, per repo rules.
2. Re-run the same 30-run spec after the fixes to get a noise band before any hillclimb decision
   (protocol: two passes per task per arm).
3. Hillclimb round 1 against the **`py-mcp-meta` test-adding habit** (card/prompt wording around
   `expected.newTests = 0`) — the one failure mode that is real, frequent, and surface-addressable.
4. Run arm `d` across **all seven tasks** if the digest stays in contention, removing the
   selection bias.
5. Observe the `om-win-arm64` arm-`k` context spike; if it reproduces, instrument the surface's
   effect on exploration on minimal-change tasks.

## 13. Artefacts and reproduction

In this directory:

| File | Content |
|---|---|
| [`runs.json`](runs.json) | All 30 runs: order, timestamps, task/arm/run, verdicts, machine reasons, surface columns, tokens, est. cost, wall time, session ids, parent/diff SHAs; the reconstructed row is flagged by note |
| [`control-digests.json`](control-digests.json) | Executor/CLI/checker digests, per-arm surface digests, evaluator script hashes, ledger SHA-256 (pre/post BOM strip), repo HEAD at report time |
| [`diffs/`](diffs/) | The 30 delivery diffs, named `{NN}-{task}-{arm}{run}-{verdict}.diff` |
| [`blind-pack/`](blind-pack/INDEX.md) | INDEX + 14 anonymized exhibits as handed to the blind reviewer ([R01](blind-pack/R01.md), [R02](blind-pack/R02.md), [R03](blind-pack/R03.md), [R04](blind-pack/R04.md), [R05](blind-pack/R05.md), [R06](blind-pack/R06.md), [R07](blind-pack/R07.md), [R08](blind-pack/R08.md), [R09](blind-pack/R09.md), [R10](blind-pack/R10.md), [R11](blind-pack/R11.md), [R12](blind-pack/R12.md), [R13](blind-pack/R13.md), [R14](blind-pack/R14.md)) |
| [`blind-review.md`](blind-review.md) | Verbatim blind-review table, findings, post-hoc exhibit→run mapping, disagreements |
| [`CROSS-METHOD.md`](CROSS-METHOD.md) | Cross-methodology battery vs Spec Kit (`s`) and OpenSpec (`o`), same executor/tasks/scorer (report) |
| [`cross-method/`](cross-method/) | Cross battery data: `rows.json` (28 rows, per-stage sessions included), `stats-cross.md`, `diffs/` (28 delivery diffs) |

Canonical ledger: `tools/token-bench/doless/results.jsonl` (untracked by design) — 66 rows at
battery-1 report time (37 Codex + 29 Drael; the reconstructed `k1` row lives only in `runs.json`).
SHA-256 after the BOM strip at that time: `dc688da1…97e8b51c` (pre-strip `0aeb5d47…eb0e6344a`,
166,311 B). The cross-method battery (run afterwards) appended 30 Drael rows (28 scored runs + 2
excluded smoke rows); post-cross ledger SHA-256 `1a0addd7…fd29bb97` (315,609 B) is recorded in
[`control-digests.json`](control-digests.json) and [CROSS-METHOD.md](CROSS-METHOD.md) §12.

Official outputs reproduced at report time (`python tools\token-bench\doless.py …`):

```text
$ python tools\token-bench\doless.py table
task | split | arm | runs | do-less pass/scored | USD total | reasons (count)
docs-config-risk | train | codex:a@sha256:e5445c282107b1b2e9f8103172ed8852fd1372a6fe1552b94c677da7813743d2 | 4 | 2/4 | 0.000 | oracle_fail x2
docs-config-risk | train | codex:k@sha256:e5445c282107b1b2e9f8103172ed8852fd1372a6fe1552b94c677da7813743d2 | 3 | 2/3 | 0.000 | oracle_fail x1
docs-config-risk | train | drael:a | 1 | 0/1 | - | oracle_fail x1
docs-config-risk | train | drael:d | 1 | 0/1 | - | oracle_fail x1
docs-config-risk | train | drael:k | 1 | 1/1 | - | 
docs-doc-comments | test | codex:a@sha256:e5445c282107b1b2e9f8103172ed8852fd1372a6fe1552b94c677da7813743d2 | 3 | 3/3 | 0.000 | 
docs-doc-comments | test | codex:k@sha256:e5445c282107b1b2e9f8103172ed8852fd1372a6fe1552b94c677da7813743d2 | 3 | 3/3 | 0.000 | 
docs-doc-comments | test | drael:a | 2 | 2/2 | - | 
docs-doc-comments | test | drael:k | 2 | 2/2 | - | 
om-win-arm64 | test | drael:a | 2 | 2/2 | - | 
om-win-arm64 | test | drael:k | 2 | 2/2 | - | 
py-actor-session-header | train | codex:a@sha256:e5445c282107b1b2e9f8103172ed8852fd1372a6fe1552b94c677da7813743d2 | 3 | 2/2 | 0.000 | agent_error x1, oracle_fail x1
py-actor-session-header | train | codex:k@sha256:e5445c282107b1b2e9f8103172ed8852fd1372a6fe1552b94c677da7813743d2 | 3 | 3/3 | 0.000 | 
py-actor-session-header | train | drael:a | 2 | 2/2 | - | 
py-actor-session-header | train | drael:k | 2 | 2/2 | - | 
py-mcp-meta | train | codex:a@sha256:e5445c282107b1b2e9f8103172ed8852fd1372a6fe1552b94c677da7813743d2 | 3 | 1/3 | 0.000 | unrequestedNewTests x2
py-mcp-meta | train | codex:k@sha256:e5445c282107b1b2e9f8103172ed8852fd1372a6fe1552b94c677da7813743d2 | 3 | 0/3 | 0.000 | unrequestedNewTests x3
py-mcp-meta | train | drael:a | 2 | 0/2 | - | unrequestedNewTests x2
py-mcp-meta | train | drael:d | 1 | 0/1 | - | unrequestedNewTests x1
py-mcp-meta | train | drael:k | 2 | 0/2 | - | unrequestedNewTests x2
py-sessionend-cap | test | codex:a@sha256:e5445c282107b1b2e9f8103172ed8852fd1372a6fe1552b94c677da7813743d2 | 3 | 2/3 | 0.000 | unrequestedNewTests x1
py-sessionend-cap | test | codex:k@sha256:e5445c282107b1b2e9f8103172ed8852fd1372a6fe1552b94c677da7813743d2 | 3 | 2/3 | 0.000 | unrequestedNewTests x1
py-sessionend-cap | test | drael:a | 2 | 2/2 | - | 
py-sessionend-cap | test | drael:k | 2 | 2/2 | - | 
rs-signal-id-retry | train | codex:a@sha256:e5445c282107b1b2e9f8103172ed8852fd1372a6fe1552b94c677da7813743d2 | 3 | 0/3 | 0.000 | oracle_fail x3
rs-signal-id-retry | train | codex:k@sha256:e5445c282107b1b2e9f8103172ed8852fd1372a6fe1552b94c677da7813743d2 | 3 | 0/3 | 0.000 | oracle_fail x3
rs-signal-id-retry | train | drael:a | 2 | 0/1 | - | keel_check_error x1, oracle_fail x2
rs-signal-id-retry | train | drael:d | 1 | 0/0 | - | keel_check_error x1, oracle_fail x1
rs-signal-id-retry | train | drael:k | 2 | 0/2 | - | oracle_fail x2, regression_fail x1
```

```text
$ python tools\token-bench\doless.py pareto
arm | runs | scored | INCOMPLETE | pass rate [95% CI] | USD mean/run | USD median/run | USD per pass | frontier | Codex tokens by requested model (input/cached/output)
codex:a@sha256:e5445c282107b1b2e9f8103172ed8852fd1372a6fe1552b94c677da7813743d2 | 19 | 18 | 1 | 10/18 = 56% [34%, 75%] | 0.000 | 0.000 | 0.000 | YES | gpt-6-sol: 9924776/9220480/53335
codex:k@sha256:e5445c282107b1b2e9f8103172ed8852fd1372a6fe1552b94c677da7813743d2 | 18 | 18 | 0 | 10/18 = 56% [34%, 75%] | 0.000 | 0.000 | 0.000 | YES | gpt-6-sol: 10796033/10022272/61077
drael:a | 13 | 12 | 1 | 8/12 = 67% [39%, 86%] | - (13 without cost) | - | - | unplaced | 
drael:d | 3 | 2 | 1 | 0/2 = 0% [0%, 66%] | - (3 without cost) | - | - | unplaced | 
drael:k | 13 | 13 | 0 | 9/13 = 69% [42%, 87%] | - (13 without cost) | - | - | unplaced |
```

(Reading note: the canonical `drael:k` row shows 13 runs / 9-13 because the reconstructed
`docs-config-risk` `k1` FAIL row is not in `results.jsonl`; including it gives the 9/14 figure in
§3. Canonical USD is absent for all Drael rows by design — subscription billing.)

Repo HEAD at report time: `8469dc1894bc20645eaea953195d031c1bead4a5` (`main`). Battery checkouts
were frozen at their per-task parent SHAs throughout.

Post-report verification (2026-10-27, #2141): headlines recomputed independently and unchanged;
see `CROSS-METHOD.md` §13 for the notes it adds (smoke rows in `doless.py table`, LF pins, token
sampling policy, `keel.rel.no_panic`, forward-only oracle fixes).
