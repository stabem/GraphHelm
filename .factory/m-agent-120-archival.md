# #120 — STORM ATTRIBUTION: merged archival record

Assembled by M Agent, 2026-08-19/20, **reading only, no cargo**. This is the single merged record
the close-doc evidence commit carries. It supersedes nothing; it **collects** what lived only in
working files.

## THE RULE THIS DOCUMENT IS BUILT AROUND

**A STORM RATE IS A PROPERTY OF `commit × session`. A standalone characterization is VOID without
its session.** (L's rule, adopted tonight.)

**The structure below exists to make pooling impossible.** Figures are grouped **by session
first**, never by commit. **No total in this file spans two sessions.** If you want a number that
does, it does not exist and this document is the reason you cannot assemble one by accident.

**CITE-or-MARK throughout.** Every figure names its file. Where I did not verify something, it says
so. **I ran nothing; every number here was produced by someone else and is transcribed with its
provenance.**

---

## SESSION 1 — morning, `53d212d`, H's base characterization

**Source:** `.factory/h-agent-base-measurements.md`. **Session window: 09:23–09:38** (log mtimes,
independently verified by me; see §5).

| Test | isolated | in-suite | failure text |
|---|---|---|---|
| `the_storm_holds_under_eight_concurrent_agents` | **4/10 FAIL** | **2/3 FAIL** | read-phase 10060 at `api_http.rs:464` (always) + `get_status` 10060 (sometimes) |
| `a_sleeper_wakes_…_zero_requests_in_the_window` | **9/10 FAIL** | **3/3 FAIL** | one form only, `wake_http.rs:822:5` |
| `concurrent_sweeps_never_double_consume_a_lease` | **0/10** | **0/3** | none observed |

- **Panic-line inventory, all 6 failing storm runs:** `:464` ×16, `:221` ×4, **zero** `:443`, **zero**
  `:460-461`.
- **Machine state at this session: UNRECORDED.** No `free_gb`, no fsync figure, no concurrent-cargo
  count. **This is the gap that cost the lane its attribution** — see §4.
- **Bounded after the fact:** the disk was **not at 0 GB** during this window (a full build plus 13
  test runs completed with zero disk errors). **Bound, not measurement.**

## SESSION 2 — evening, post-disk-prep, A's execution record

**Source:** `.factory/a-agent-storm-execution-record.md`. **Machine fields present per run**:
fsync 1.5–2.0 ms/op, ~19 GB free, zero concurrent cargo.

| Run | Commit | Result |
|---|---|---|
| Run 0 (control) | **`53d212d`** — same commit as session 1 | **0/10 FAIL** |
| Run 1 | `ef51193` | **0/10 FAIL** |
| Run 2 | instrumented | **10 NOT-RESULTS** (instrument sink bug; no storm numbers) |
| Run 2b | instrumented, sink v2 | **10/10 PASSES** |
| Run 3 | probe-off | **3/3 PASSES** |

- **Headroom (D1), this session:** opens/request 2.40–2.56; median request-open 29.1–32.6 ms; **max
  single open 0.26 s**; O = 70–80 ms; **8×O = 0.56–0.64 s against a 5 s budget ≈ 8× headroom**.
- **Pooled open distribution, n = 1521:** all-at-median 0.61 s; all-at-p99 2.81 s; **all-at-max
  5.22 s — crosses by 4%**.

## SESSION 3 — night, D's attribution run (interleaved)

**Source:** `.factory/d-agent-storm-rate-results.txt`; pre-registration
`.factory/d-agent-storm-attribution-PREREG.md`.

- **Design:** two isolated worktrees, separate `CARGO_TARGET_DIR`, **arms alternated run-by-run
  (ABAB)** — *"alternating rather than blocking is the whole point: this test is disk-conditional,
  and two blocks measure disk drift as if it were code."*
- **Instrument byte-identical in both arms**, inserted-region `sha256 d33e5a35fb6bba66034e8762d369f122`.
- **Positive control before the sequence:** PRE PASS, events=48, explicitly *"not part of the sealed
  20"*.

| Arm | Commit | FAIL / logged runs |
|---|---|---|
| **PRE** | `07b1243` | **0 / 11** |
| **POST** | `0fb0e66` | **7 / 11** |

- **`free_gb` constant at 503.9 across the session** — the prereg's void condition (>2 GB drift)
  **did not trigger**.
- **MARKED, NOT RESOLVED: 11 logged runs per arm against a pre-registered N=10.** The header names
  one PRE-side positive control as outside the sealed 20. **Whether the 11th POST entry is a
  control or an extra pair is not established by the file, and I did not resolve it by inference.**
  The 0-vs-7 split is unaffected by which reading is right; the **N** is what is imprecise.
- **Verdict against D's own pre-registered split** (POST 6–10 fail, PRE 0–2 fail): **matched.** His
  falsifier — *"H1 IS FALSIFIED if the two arms' counts are within 2 of each other"* — **did not
  fire.**

## SESSION 4 — night, D's restored-rate run (#123 fix arm)

Two files, and **they are not the same measurement.**

### 4a — contaminated, INCOMPLETE (`d-agent-123-restored-rate.txt`)

**PRE 3/4 FAIL, FIX 2/4 FAIL — four pairs only.** D recorded the contamination **at pair 2, not
afterwards**: he started a `cargo build` mid-sequence. His own note: ABAB protects the
**comparison** because both arms meet the same disturbance, but **the absolute rates in this file
are not a clean sample of an idle machine**, and a near-threshold result should be **re-run quiet
rather than argued.** **Not scoreable. Retained for provenance only.**

### 4b — QUIET, complete (`d-agent-123-restored-rate-QUIET.txt`)

- Machine at launch: **0 cargo/rustc/graphhelm processes, 505.1 GB free**, *"nothing else runs
  during this sequence."*
- Instrument region **identical to session 3's** (`sha256 d33e5a35…`). Slope re-checked **on this
  build** (2 events/round) rather than assumed to carry over. Guards green on this build:
  `execution_cli` 23/23.
- **Stopping rule committed BEFORE launch** (`.factory/d-agent-123-guard-split-SEALED.md`):
  **MOVED-BACK if arms differ by ≤1 of 10; NOT RESTORED if ≥6; INDETERMINATE otherwise.**

| Arm | Commit | FAIL / 10 |
|---|---|---|
| **PRE** | `07b1243` | **4 / 10** |
| **FIX** | `0fb0e66` + #123 | **1 / 10** |

- **Difference = 3. Rule fires: INDETERMINATE** — neither MOVED-BACK (≤1) nor NOT RESTORED (≥6).
  **Scored by the rule, not by the direction**, and the direction is favourable to the fix.

---

## §4 — THE COMMIT × SESSION BOUND, MEASURED

**The same PRE commit `07b1243` produced 0/10 in session 3 and 4/10 in session 4.**

| Session | Commit | FAIL / 10 | `free_gb` |
|---|---|---|---|
| 3 (attribution) | `07b1243` | **0 / 10** | 503.9 |
| 4b (restored-rate, quiet) | `07b1243` | **4 / 10** | 505.1 |

**This is the rule's own evidence.** A characterization of `07b1243` as "a 0/10 commit" or "a 4/10
commit" is **void**; both numbers are real and neither is the commit's property.

- **What it does NOT establish:** which session-level variable moved. `free_gb` differs by only
  1.2 GB; **fleet load and cache state at session 3 versus 4 are not recorded at the granularity
  that would separate them.**
- **The same lesson session 1 paid for**, now measured within one night rather than argued across a
  day: **`53d212d` was 4/10 in session 1 and 0/10 in session 2** — same commit, different sessions,
  and the disk prep sits between them.
- **THE HONEST NET, unchanged by any of tonight's runs: "THE FLAKE WAS THE DISK" IS EXACTLY AS
  UNESTABLISHED AS "THE FLAKE IS GONE."**

---

## §5 — PROVENANCE PER FIGURE

| Figure | File | Base commit | Pin / verification |
|---|---|---|---|
| Session-1 rates, panic inventory | `h-agent-base-measurements.md` | `53d212d` | log mtimes **verified by me** (run1 09:23:18 → run10 09:28:19; failing runs identifiable by size: 172 B = pass, larger = fail; 4 larger, matching the reported 4/10) |
| Session-1 disk bound | `d-agent-disk-timeline-evidence.md` | — | cache-lever mtimes **verified by me**: 14:46:48 and 14:46:49, both dirs target-less |
| Session-2 runs, headroom, n=1521 | `a-agent-storm-execution-record.md` | `53d212d`, `ef51193` | SHA verified **before** the first run (A's protocol) |
| Session-3 attribution | `d-agent-storm-rate-results.txt` | `07b1243` vs `0fb0e66` | instrument region `sha256 d33e5a35fb6bba66034e8762d369f122` |
| Session-3 pre-registration | `d-agent-storm-attribution-PREREG.md` | — | written before any run |
| Session-4a contaminated | `d-agent-123-restored-rate.txt` | `07b1243` vs fix | contamination recorded **at pair 2** |
| Session-4b quiet | `d-agent-123-restored-rate-QUIET.txt` | `07b1243` vs `0fb0e66`+#123 | same instrument sha; stopping rule pinned in `d-agent-123-guard-split-SEALED.md` |
| Scoring cells, all lanes | `m-agent-prediction-ledger.md` | — | `sha256 5be2d1535161e528…` (16-char prefix) |
| Close-out | `m-agent-ledger-closeout.md` | — | `sha256 8ba801f1b3388fc7…` |

---

## §6 — WHAT THIS RECORD DOES NOT CONTAIN

- **No pooled figure of any kind.** Not across sessions, not across scopes (isolated vs in-suite),
  not across arms. **The tables are per-session by construction.**
- **No mechanism for the storm.** Four hypotheses are dead or disfavoured; **H1 vs H2 vs H3 is
  untouched.** The night's runs measured a *rate movement attributable to a commit*, which is a
  different question from *why the test times out*.
- **No claim that session 3's 0/10 establishes zero.** D's own prereg says it: at N=10, 0/10 leaves
  a **~26% upper bound** — the design **can establish a MOVEMENT and cannot establish a ZERO.**
- **No resolution of the 11-vs-10 run count** in session 3 (§3, marked).
- **No machine variables for session 1**, because none were recorded. That absence is the record.
