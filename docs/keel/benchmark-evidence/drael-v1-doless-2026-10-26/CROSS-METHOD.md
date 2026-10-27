# Cross-method comparison — single-shot vs Spec Kit vs OpenSpec (drael-v1) — 2026-10-26→27

**Status.** Second battery on the same executor, tasks, and scorer as the [three-arm battery](README.md)
(arms `a`/`k`/`d`, 2026-10-26). It adds arms `s` (Spec Kit, 4-stage) and `o` (OpenSpec v1.15.0, 2-stage)
so the protocol claim can be read across *methodology*, not just surface. Headline: the multi-stage
spec-driven chains cost **≈9–11× the dollars per proven delivery** of the single-shot arms once the
suspected mis-graders are excluded, score **no higher** (43%/38% vs 67%/64%), and reproduce every
task-intrinsic failure mode the single-shot arms make — while adding one failure mode of their own
(`om-win-arm64` artifact pollution). The chains are good documentation producers; under a headless
agent executor they are poor cost engineers. Read §10 before quoting numbers.

## 1. Claim under test

Same claim as the parent battery, per [BENCHMARK_PROTOCOL.md](../../BENCHMARK_PROTOCOL.md):
*"equal or better delivered-code quality at lower total cost per proven delivery."* Exactly one
variable moves between these arms: the methodology surface handed to the agent. Executor, model,
frozen tasks, scorer, and timeout policy are identical to the parent battery.

## 2. Arms and frozen controls

| Arm | Methodology | Shape |
|---|---|---|
| `a` / `k` / `d` | — (parent battery) | single shot (+ Keel surfaces / digest) |
| `s` | Spec Kit ([github/spec-kit](https://github.com/github/spec-kit), MIT) | 4 sequential fresh sessions: `specify → plan → tasks → implement` |
| `o` | OpenSpec v1.15.0 ([Fission-AI/OpenSpec](https://github.com/Fission-AI/OpenSpec), Node CLI) | 2 sequential fresh sessions: `opsx-propose → opsx-apply`; the agent runs the real `openspec` CLI in the checkout |

Common controls (identical to the parent battery unless stated):

- **Executor/model:** `opencode` CLI v2.1.4 + `drael/drael-v1`, `opencode run --auto --format json`.
- **Tasks:** the same 7 frozen battery tasks (`tools/token-bench/doless/tasks.json`, doless v2),
  same per-task parent SHAs. Window `2026-10-26T19:45:44Z → 2026-10-27T06:53:52Z`, 28 runs
  (7 tasks × 2 arms × 2 runs), plus 2 excluded smoke rows (run 0).
- **Scorer:** hidden oracle + regression observer + `graphhelm keel check` + do-less surface
  columns; verdict logic `doless.do_less_verdict`. CI: Wilson 95% over scored runs.
- **Execution model:** each stage is a **fresh session** (no context carries between stages); what
  carries is the checkout — i.e. the artifacts the previous stages wrote. Stage answers are
  concatenated with `--- next stage ---` separators into the scored answer; token usage is the sum
  of each stage's session subtree; wall time is the sum of stage walls. Timeout cap **2400 s per
  stage** (parent battery: 1500 s per run).
- **Surfaces:** 11 pinned command/template files, SHA-256 in
  [`control-digests.json`](control-digests.json) (`crossMethod.surfaceFileSha256`); files committed
  under `tools/token-bench/doless/surfaces/spec-kit/` and `.../openspec/`, the OpenSpec scaffold
  (from `openspec init --tools opencode`) under `.../openspec-init/`. Every ledger row repeats its
  own pins and per-stage prompt digests (`surfaceDigests`, `promptDigest`).
- **Code/drivers committed for reproduction:** `tools/token-bench/driver_cross.py` (+ the
  parent-battery `driver.py` it builds on) and `PLAN_CROSS.json`; digests in `control-digests.json`.

Documented fidelity adaptations (each changes the arm from its interactive default):

1. **Headless.** No interactive questions allowed; the prelude instructs the agent to make
   reasonable assumptions and record them in the artifact. Both chains assume a human at the
   keyboard by default; none of the 56 stages waited for input.
2. **Constitution stage omitted (arm `s` only).** `/speckit.constitution` is one-time project setup,
   not per-feature work; the checkout's root `AGENTS.md` substitutes as the governance reference
   (stated in the prelude). What ran is therefore the per-feature 4-stage chain, not 5.
3. **Missing `.specify/scripts/` (arm `s`).** This checkout has no Spec Kit scripts; the
   `setup-plan` / `setup-tasks` / `check-prerequisites` JSON outputs are given inline in the stage
   prelude, which says so. `FEATURE_DIR` is fixed to `specs/001-bench` (no sequential-number scan).
4. **Commands delivered expanded.** Slash commands arrive as their generated markdown with
   `$ARGUMENTS` substituted (the first stage gets the task text; later stages get the change name /
   nothing) — exactly how a harness expands them.
5. **Artifact exclusion in blocking columns.** Methodology artifact directories are excluded from
   the do-less *blocking* columns (`unrequestedNewFiles`, and `keel check` OUTSIDE_CARD findings
   pointing into them): `s` → `specs/`, `.specify/`; `o` → `openspec/`. Full diffs, changed-line
   totals, token and wall totals still include artifacts; `changedLinesCode` counts non-artifact
   lines only. Rationale without this rule, multi-stage arms would be blocked by their own
   paperwork on every run (see §5).
6. **AGENTS.md is constant across all five arms** (auto-loaded by `opencode` in every worktree),
   so it is floor, not differentiator — the same was true in the parent battery.

Excluded tools and why (kept out rather than mis-measured):

- **Kiro** (AWS): closed-source, IDE-locked; EARS specs live inside the IDE. No headless CLI to
  drive, so it cannot sit in an executor-controlled battery.
- **BMAD-METHOD**: multi-agent orchestration framework (its own planner/architect/dev agents).
  Driving it would conflate *agent count* with *methodology* against a single-executor contract.
- **"GitHub Next"** is represented through Spec Kit, which is the GitHub Next team's product.

## 3. Headline results

`a`/`k`/`d` columns transcribed from [§3 of the parent report](README.md#3-headline-results);
`s`/`o` computed over 28 runs (CI over scored runs; cost includes INCOMPLETE).

| Arm | Runs | P/F/I | Score (95% CI) | Tokens/run | Est. $/run | Est. $/proven | Wall median / max (s) |
|---|---|---|---|---|---|---|---|
| `a` | 13 | 8/4/1 | 8/12 = 67% [39%, 86%] | 1,452,452 | 0.737 | 1.198 | 134 / 1345 |
| `k` | 14 | 9/5/0 | 9/14 = 64% [39%, 84%] | 1,106,311 | 0.563 | 0.876 | 156 / 953 |
| `d` | 3 | 0/2/1 | 0/2 = 0% [0%, 66%] | 2,032,480 | 1.027 | n/a (no proven) | 122 / 856 |
| `s` | 14 | 6/8/0 | 6/14 = 43% [21%, 67%] | 5,692,391 | 2.925 | 6.824 | 1982 / 2880 |
| `o` | 14 | 5/8/1 | 5/13 = 38% [18%, 64%] | 4,141,766† | 2.103† | 5.888† | 824 / 3082 |

† Arm `o`'s ledger under-counts one INCOMPLETE row by 10,789,012 tokens (flush lag on the timed-out
`om-win-arm64` apply stage; §9 defect 5). Corrected: 68,773,729 tokens total; $/proven in
[6.97, 9.13], realistically ≈ 7.0–7.4 given that stage's composition (94% cache-read). The
correction does not move any verdict.

**The cost structure.** Per proven delivery: `s` is **5.7×** `a` and **7.8×** `k` on raw totals,
and the gap *widens* once the two suspected mis-grader tasks (§5) come off both sides — see the
sensitivity table below. The extra spend is not mystery overhead: §7 shows it decomposes into
planning stages plus re-reads of everything those stages wrote.

**Sensitivity (scored / est. $ per proven delivery):**

| Basis | `a` | `k` | `s` | `o` (ledger / corrected) |
|---|---|---|---|---|
| all tasks | 8/12 · $1.198 | 9/14 · $0.876 | 6/14 · $6.824 | 5/13 · $5.888 / $6.97–9.13 |
| excl. `rs-signal-id-retry` | 8/11 · $0.436 | 9/12 · $0.436 | 6/12 · $4.768 | 5/11 · $4.504 / $5.27–6.92 |
| excl. `rs` + `docs-config-risk` | 8/10 · $0.414 | 9/11 · $0.466 | 6/10 · $4.280 | 5/9 · $3.884 / $4.42–5.91 |

With the mis-graders removed, **the ratio settles at 9–11× per proven delivery** and stays there
under the flush correction. Quality stays at or below parity throughout.

## 4. Per-task verdict map

Verdicts per arm/run in score order. `a`/`k`/`d` from [§4 of the parent report](README.md#4-per-task-verdict-map-drael);
`*` = adjusted view noted in §5.2.

| Task (split) | `a` | `k` | `d` | `s` | `o` |
|---|---|---|---|---|---|
| docs-config-risk (train) | FAIL | FAIL, PASS | FAIL | FAIL, FAIL | FAIL, FAIL |
| docs-doc-comments (test) | PASS, PASS | PASS, PASS | — | PASS, PASS | PASS, PASS |
| om-win-arm64 (test) | PASS, PASS | PASS, PASS | — | FAIL*, FAIL | FAIL, INCOMPLETE* |
| py-actor-session-header (train) | PASS, PASS | PASS, PASS | — | PASS, PASS | PASS, PASS |
| py-mcp-meta (train) | FAIL, FAIL | FAIL, FAIL | FAIL | FAIL, FAIL | FAIL, FAIL |
| py-sessionend-cap (test) | PASS, PASS | PASS, PASS | — | PASS, PASS | FAIL‡, PASS |
| rs-signal-id-retry (train) | INCOMPLETE, FAIL | FAIL, FAIL | INCOMPLETE | FAIL, FAIL | FAIL, FAIL |

‡ `o1` made a correct, arguably better implementation (host-keyed timeout table) that the oracle's
mini-evaluator cannot resolve — §5.3, defect 3.

**The map's shape is the finding.** The three tasks that fail everywhere (`docs-config-risk`,
`py-mcp-meta`, `rs-signal-id-retry`) fail on `s`/`o` with the *same blocking reasons* they fail on
`a`/`k`. The four tasks that pass on the single-shot arms pass on the chains — with near-identical
delivered code (§7.3). The chains neither fix the failures nor introduce answer-quality regressions
anywhere except the methodology-specific `om` pollution (§5.2).

## 5. Failure attribution

Blocking reasons per FAIL/INCOMPLETE run, counted over the 28 runs:

| Blocking reason | `s` (8 blocked) | `o` (9 blocked) | Class |
|---|---|---|---|
| `oracle_fail` | 5 | 6 | task/instrument (§5.1, §5.3) |
| `regression_fail` | 1 (instrument-caused, defect 1) | 0 | instrument (§9) |
| `unrequestedNewTests` | 3 (tests added: 2, 3, 3) | 2 (tests added: 1, 4) | task-intrinsic agent habit |
| OUTSIDE_CARD / unrequested new files | 3 runs (4+2 files) | 1 run (1 file) | habit / method-specific |
| `agent_timeout:apply` | 0 | 1 | reliability (§6) |

(Runs can carry more than one reason; e.g. `rs` arm-`s` run 1 also drew OUTSIDE_CARD.)

### 5.1 Task-intrinsic failures (all arms, unchanged by methodology)

- **`py-mcp-meta` — the test-adding habit.** All 8 chain runs (like all 5 parent-battery Drael runs
  and most historical Codex runs) made the correct core fix — the oracle passes in every one — and
  were blocked by adding tests the card did not request. The habit does not shrink with planning:
  tests added per run: `a` 1,1 · `k` 1,1 · `d` 1 · `s` 2,3 · `o` 1,4. Small n; direction only.
- **`rs-signal-id-retry` — suspected mis-grader.** 8/8 chain runs `oracle_fail` on the same
  single-line co-occurrence check (parent battery: 11/11 across models). Multi-stage planning buys
  nothing here: the idiomatic multi-line fix fails the grader regardless.
- **`docs-config-risk` — placement-sensitive oracle.** 4/4 chain runs `oracle_fail` for prose
  placement of a phrase the oracle only finds inside table cells (parent battery: same class).

### 5.2 Methodology-specific: `om-win-arm64` artifact pollution

The task asks the agent to *prove what cannot be observed from this x64 host* — the reference
answer is "change nothing, say OBSERVER_MISSING". Its oracle fails if **any** checkout path
changed. Single-shot arms write no durable files → pass (a 2/2, k 2/2). Chain arms write 7–15
spec/design/task artifacts into the checkout → oracle fails on all 4/4 runs, even where the answer
is otherwise correct:

| Run | Repo changes (true code lines) | OBSERVER_MISSING in scored answer | Verdict | Adjusted view (artifact dirs tolerated) |
|---|---|---|---|---|
| `s1` | none (0) | yes (all 4 stages) | FAIL (oracle) + regression FAIL (defect 1, also instrument) | **PASS-equivalent** |
| `s2` | edited `docs/install/GETTING_STARTED.md` + new `docs/acceptance/windows-arm64-2026-10-26.md` (194 lines) | yes | FAIL (oracle, outside_card=2) | **still FAIL** — genuine scope drift: the chain went and edited install docs nobody asked it to touch |
| `o1` | none (0) | yes (apply stage) | FAIL (oracle) | **PASS-equivalent** |
| `o2` | new `docs/acceptance/windows-arm64-verify-2026-10-26.md` (+76 lines) | **no** (absent from the final apply stage) | INCOMPLETE (apply timeout) | **FAIL-equivalent** — wrote a doc, dropped the marker, and timed out |

So the chains' *answer quality* on this task splits one genuine trap taken (`s2` editing
`GETTING_STARTED.md` is the only scope drift anywhere in either battery) from measurement
interference. The interference is real enough to need a rule change before SDD chains can be
measured fairly on any "hold the line" task (defect 4).

### 5.3 Instrument-specific: oracle form limits

`py-sessionend-cap o1` (FAIL) introduced `END_ACK_TIMEOUTS = {"claude": 3.0, "codex": 2.0}` and
called `request(..., timeout=END_ACK_TIMEOUTS[host])` — a clean per-host constant table with an
explanatory comment, arguably the best implementation in the battery. `o2` (PASS) used the literal
`timeout=2.5`. The oracle's mini-evaluator resolves literals, names, tuples/lists/sets and
conditionals — but **not dict subscripts** — so `o1`'s waits list is unresolvable and it fails with
*"could not be resolved"*. Same class as the `rs` single-line fragility: the grader checks form,
not intent. This is the second instance of that class found in these batteries.

## 6. Reliability

- **28/28 terminal verdicts, zero driver losses.** One stage timeout (`o` `om-win-arm64` apply at
  the 2400 s cap, after a 682 s propose) → 1 INCOMPLETE. Zero no-session stages; the per-entry
  fault tolerance in the driver was never triggered (`ENTRY FAILED`: 0 in the run log).
- **Paired agreement (run 1 vs run 2 per task): `s` 7/7; `o` 5/7.** Both `o` splits are now
  characterized and neither is answer variance: `om-win-arm64` (FAIL vs INCOMPLETE) is the timeout
  above; `py-sessionend-cap` (FAIL vs PASS) is the dict-form oracle limit above. On answer
  quality alone, both chains are paired-consistent on all 7 tasks.
- **Ledger completeness.** Every row carries full subtree usage. One post-hoc correction flagged:
  the `om-win-arm64 o2` apply stage flushed usage after scoring (defect 5). Nothing else moved
  between scoring time and report time (verified per-row, 27/28 exact).
- **Wall.** `o` median 824 s vs `s` 1982 s — the 4-stage shape carries ~2.4× the wall of the
  2-stage shape at equal quality, which matters on top of token cost for interactive-ish use.

## 7. Token economics: where the money went

### 7.1 Stage decomposition (session DB, exact)

Tokens per stage (mean over 14 runs; sums reconcile exactly to arm totals, arm `o` pre-flush-fix):

| Arm | Stage | Tokens/run (mean) | Share of arm total |
|---|---|---|---|
| `s` | specify | 687,459 | 12.1% |
| `s` | plan | **1,616,952** | **27.4%** |
| `s` | tasks | 899,324 | 15.3% |
| `s` | implement | 2,488,655 | 43.8% |
| `o` | propose | 1,836,795 | 44.3% |
| `o` | apply | 3,075,613 | 74.3%† |

† Shares overlap to 118.6% because of the `o2` flush artifact (§9.5); ledger basis gives
propose 44.3% / apply 55.7%.

**The `plan` stage is the single most expensive stage of the Spec Kit chain** — more tokens than
`specify`, nearly as many as `implement`. On these small, fully-specced cards, planning spends the
most of the budget producing a plan for work the implement stage then re-reads. Wall time agrees:
per-stage means are specify 341 s, **plan 626 s**, tasks 425 s, implement 565 s (arm `s`);
propose 427 s, apply 524 s (median 358 s) (arm `o`).

### 7.2 The compounding mechanism

Each stage starts fresh but works in a checkout that already contains the previous stages' output,
so the implement stage pays to re-read the spec + plan + tasks (and `AGENTS.md`, card, prompt) it
just paid other stages to write. Artifact volume per run: `s` 13.4 files mean (max 15),
`o` 7.9 mean (max 9). There is no shared context to amortize this — fresh sessions are what makes
the chains auditable, and what makes them expensive. A single-shot run reads the inputs once.

Outlier watch: `rs-signal-id-retry s2` spent **14,345,229 tokens** (implement stage 1156 s) on a
small Rust change — an exploration spiral the chain shape amplified. Task totals on `rs`:
`a` 12.07M / 2 runs, `k` 7.83M / 2, `s` 24.23M / 2, `o` 13.65M / 2.

### 7.3 What the extra spend buys: measurable, none

On every task where both a chain and a single-shot arm pass, the delivered code is the same size —
often the same line:

| Task | True non-artifact code lines (per run) |
|---|---|
| py-actor-session-header | `a`/`k` one-line header change · `s` 2, 2 · `o` 2, 2 |
| docs-doc-comments | ~30-line doc comment block in all passing arms (`s` 30, 30 · `o` 30, 30) |
| py-sessionend-cap | literal timeout + hook `timeout: 5→3` in both files (`s` 26, 18 · `o` 33, 17) |

The chains' marginal output is the spec/design/tasks documents themselves. If a human consumes
those documents, the process earns its keep as documentation — several are genuinely good:
`o1`'s `design.md` independently verified the byte-identity test constraint before choosing its
approach, and `o1`'s sessionend implementation (§5.3) is the strongest delivery in the battery.
The do-less cost model prices deliveries, not documents; under it, the chain premium is pure
overhead. That is a statement about *this consumption model*, not about SDD in general (§10).

### 7.4 Cost ratios (summary)

- Raw: `s` = 5.7× `a`, 7.8× `k` ($/proven); `o` = 4.9× `a`, 6.7× `k` (ledger basis).
- Mis-graders excluded: **`s` ≈ 10.9× `a` (sensitivity basis 2: 10.3×); `o` ≈ 10.3× `a`
  (corrected: 8.7–11.0×).**
- Never-counted-as-savings (protocol): every failed run's tokens in both arms (22 of 28 failed or
  were incomplete in cost terms — 13 of 28 delivered a proven result).

## 8. Qualitative notes

- **Headless adaptation worked.** 0/56 stages stalled waiting for input; assumptions were recorded
  in artifacts as instructed. The interactive defaults of both tools survive conversion to
  unattended execution without driver hacks.
- **OpenSpec's CLI round-trip is real work, not ceremony.** Arm `o` sessions genuinely ran
  `openspec new change` / `status` / `instructions` (telemetry off) and wrote deltas the tooling
  validates; `opsx-apply` consumed the propose-stage artifacts. This is the closest-to-vendor form
  of the two arms.
- **Spec Kit's constitution omission is cheap here** because `AGENTS.md` is an unusually strong
  governance substitute; on a bare repo the substitution would be weaker.

## 9. Instrument defects (cross battery, actionable)

1. **cp1252 crash in `diff_check.py` masks its own diagnosis.** `om-win-arm64 s1`: the agent's
   `specs/001-bench/data-model.md` contains one trailing-whitespace line inside a `│`-box diagram.
   `git diff --check` reports it; `diff_check.py` then crashes in `print()` (console encoding
   cp1252 cannot emit U+2502) before writing the intended FAIL message, so the ledger tail is a
   traceback, not the finding. Fix: force UTF-8 on the subprocess stdout path
   (`sys.stdout.reconfigure(encoding="utf-8")` / `PYTHONIOENCODING=utf-8`) and, ideally, accept a
   pathspec so artifact dirs can be excluded from the whitespace check for multi-stage arms.
2. **`ARTIFACT_PREFIXES` string-vs-tuple bug** in the as-run driver
   (`"o": ("openspec/")` iterated character-wise, matching any path starting with
   `o`/`p`/`e`/`n`/`s`/`c`/`/`). Verified impact: **none on verdicts** — an audit over all 28 rows
   (added non-artifact files vs ledger `unrequestedNewFiles`, and OUTSIDE_CARD filtering) found no
   swallowed or flipped finding; it undercounted `changedLinesCode` for arm-`o` rows whose changed
   files lived under `plugins/` (e.g. `py-sessionend-cap o1`: ledger 33 → true 33 ok, but
   `py-actor` rows showed code=0 vs true 2). The report tables use diff-recomputed values
   (`cross-method/rows.json` field `codeLinesTrue`); the committed driver carries the one-line fix
   with an inline note.
3. **`py-sessionend-cap` oracle cannot resolve dict subscripts** (§5.3) — a correct, superior
   implementation (`END_ACK_TIMEOUTS[host]`) fails. Recommended fix: teach the evaluator
   `ast.Subscript` on a literal mapping keyed by resolved names (~1 line + a bounded test); until
   then, treat that FAIL as instrument-limited, like `rs`.
4. **`observer_missing.py` vs spec artifacts.** Any changed checkout path fails the oracle, so any
   methodology that writes spec files into the tree is structurally doomed on hold-the-line tasks.
   Add an artifact-tolerant variant (ignore declared methodology dirs) or run such tasks with an
   artifact-branch convention; document whichever is chosen as canonical.
5. **Flush-lag under-count on timed-out stages.** Usage sampled immediately after
   `TimeoutExpired` missed 10,789,012 tokens on one apply stage (94% cache-read; settled later in
   the session DB). Flagged, bounds-stated, and reconciled per-row; a longer flat-retry window
   before scoring would close it. Canonical ledger numbers remain as-scored; the correction is
   recorded here and in `control-digests.json`.

## 10. What this run does and does not establish

**Establishes**

- First headless, same-executor, same-scorer **methodology comparison** of single-shot (with and
  without pinned Keel surfaces) vs the two mainstream open-source spec-driven chains.
- A **cost structure for multi-stage SDD under an agent executor**: ≈9–11× $/proven after
  mis-grader removal, decomposed (§7.1) into planning stages + artifact re-reads, with the Spec
  Kit `plan` stage as the largest single cost center.
- **Failure-mode attribution** across methodology: the three recurring failures are task/oracle
  properties (identical blocking reasons on all five arms); one failure is methodology-specific
  (`om` artifact pollution), with its adjusted view computed (§5.2); one oracle form-limit was
  newly documented (§5.3).
- **Reliability profile of both chains headless**: 27/28 clean terminal verdicts, 1 timeout,
  paired answer-agreement 7/7 on both arms, zero session losses.
- That the chains' delivered code on shared-PASS tasks is **size-identical** to single-shot
  deliveries (§7.3) — the process spends orders of magnitude more to produce the same diff.

**Does not establish**

- Anything about **human-in-the-loop** SDD economics: humans edit artifacts between stages for free;
  a fresh LLM session paid to re-read every page it wrote itself. The 9–11× is the cost of the
  *headless agent executor consuming the chain*, not of the chain as used by its target users.
- That Kiro/BMAD share this profile (excluded, §2) — no conclusion about them.
- Any **quality win** for the chains: on quality they tie the baselines (parity on shared-PASS
  tasks, same failures elsewhere) while costing far more.
- The long-run value of the documentation artifacts as maintenance assets — out of scope of the
  do-less cost model, and explicitly the argument in favor of SDD (§7.3).

## 11. Recommendations (ordered)

1. **For single-executor agent work on cards this size: do not pay for a 4-stage chain.** Use the
   pinned Keel surfaces (arm `k`): parity-or-better quality at the lowest proven dollar in the
   parent battery. Reserve SDD chains for contexts that consume their documents (human review,
   audit trails, multi-week features).
2. **If a chain is mandated, use the 2-stage shape** and merge specify+plan+tasks into a single
   design stage when the card is small: the `plan`/`specify`/`tasks` stages are 56% of arm `s`'s
   spend (§7.1) and their content largely restates the card.
3. **Fix defects #1, #3, #4, #5** (all small): they determine whether the next battery measures
   answers or instruments. Defect #2 is fixed in the committed driver.
4. **Re-run `om-win-arm64` under an artifact-tolerant oracle** to land a clean SDD data point for
   hold-the-line behavior (current adjusted reading: s 1 P*/1 F, o 1 P*/1 F-eq).
5. **Optional arm `sl`** (single merged design stage + implement) to isolate which part of the
   chain carries the residual premium after recommendation 2.

## 12. Artefacts and reproduction

| File | Content |
|---|---|
| [`cross-method/rows.json`](cross-method/rows.json) | The 28 ledger rows as written, plus `codeLinesTrue` / `artFileCount` / `addedNA` recomputed from the saved diffs (defect 2 correction) and `runDir` |
| [`cross-method/stats-cross.md`](cross-method/stats-cross.md) | Machine-generated tables behind §3–§7 (headline, per-run, per-stage walls, sensitivity, histograms, paired agreement) |
| [`cross-method/diffs/`](cross-method/diffs/) | The 28 delivery diffs (artifacts included), named `{NN}-{task}-{arm}{run}-{verdict}.diff` |
| [`control-digests.json`](control-digests.json) | `crossMethod` block: 11 surface pins, driver/plan digests, window, timeout cap, artifact-exclusion rule, ledger SHA-256 after this battery |
| `tools/token-bench/driver_cross.py`, `tools/token-bench/driver.py`, `tools/token-bench/PLAN_CROSS.json` | Committed drivers and plan. `driver_cross.py` = as-run code + the defect-2 fix + `import traceback` + path resolution relative to the repo (noted in its docstring); `driver.py` is the parent battery's driver |
| `tools/token-bench/doless/surfaces/spec-kit/`, `…/openspec/`, `…/openspec-init/` | Pinned command markdown, templates, and the pristine `openspec init --tools opencode` scaffold |
| Run transcripts (not committed) | `scratch\drael\tb-{task}-{arm}{run}-{hex}\{prompt.txt, answer.txt, events.jsonl, diff.txt\}` per stage |

Reproduce (Windows, per the repo toolchain section): set `TOKEN_BENCH_SCRATCH`,
`TOKEN_BENCH_GRAPHHELM_CLI` (and `OPENCODE_DB` if not default), pin opencode v2.1.4 +
`graphhelm.exe` at the digested SHAs, install `openspec` v1.15.0 (npm), then
`python tools\token-bench\driver_cross.py --plan tools\token-bench\PLAN_CROSS.json`.
Expected cost at list prices: ≈ $70 total for the 28 runs; ≈ 11 h wall sequential.

Canonical ledger: `tools/token-bench/doless/results.jsonl` (untracked by design) — 96 rows after
this battery (37 Codex + 29 Drael parent + 30 Drael cross incl. 2 smoke run-0). SHA-256 after BOM
strip: see `control-digests.json → resultsJsonlSha256AfterCrossBattery`.
