# N Agent — FROZEN review criteria for #81 (restore timeout policy)

**FREEZE MARKER.** Written BEFORE K's diff exists, so the review cannot be shaped by what K
wrote. Base for every citation: `0f4e7fe`, read-only. My worktree is pinned at `efd85d0`; I have
run no cargo and need none for this. Anything added after K's PR lands goes in a DELTA section
below the marker, dated, never edited into the criteria above it.

Method rules binding this lane, applied to myself first: cite against the named base; check
EMITTERS, not tables; read what produces a value before asserting its shape; any timing claim
carries free_gb and concurrent-cargo.

---

## What the fix must do

The issue proposes three items. Restated as things a reviewer can check:

**F1 — elapsed gets its own distinguishable value.** A bounded step that exceeds its budget must
no longer be reported as `InvalidRestore`.

**F2 — the budget becomes an operation deadline, not N independent per-step caps.** Adding a
bounded step must stop silently adding another independent chance to fail.

**F3 — test budgets are anti-hang devices.** Where a bound exists only to stop a hang, tripping
it must be a real signal, and no assertion may pass or fail based on it.

---

## Gates I will apply

### G1 — the distinction must survive the CODE boundary, not just the enum
`BackupError::code()` (backup.rs:272-278) already flattens: `InvalidBackup` and `Unavailable`
BOTH map to `GHB001_BACKUP_INVALID`. So the code space is a smaller alphabet than the variant
space, and a new variant is not automatically a new code.

**Check:** a new elapsed variant must map to a code no other variant maps to. If it reuses
`GHB002_RESTORE_INVALID` or any existing string, the distinction dies exactly where the operator
reads it, and F1 is unmet however clean the enum looks. **PREDICTION, registered:** this is the
most likely place for a fix to look complete and not be.

### G2 — coverage must be checked at the EMITTERS, and one site has no `timeout(` to grep for
The issue counts 11 `process_timeout.min(...)` sites: ten at 30 s
(:899, :900, :902, :1036, :1107, :1218, :1308, :1367, :1454, :1645) and one at 10 s (:1486).

`:1486` is `acquire_target_exclusivity`, and it is **not** a `tokio::time::timeout` wrapper — it
is a hand-rolled deadline loop whose elapsed branch is `if connected > 1 || Instant::now() >=
deadline { return Err(BackupError::InvalidRestore) }` (backup.rs:1500-1502).

**Check:** a fix that greps for `timeout(` and patches the wrappers WILL MISS THIS SITE. It is
the only elapsed path with no timeout call to find. **PREDICTION, registered:** if the fix is
incomplete anywhere, it is here.

### G3 — the elapsed variant must not swallow CONTENTION
The same expression at :1500 returns one value for two different causes: a rival connection holds
the target (`connected > 1`) and the budget elapsed (`Instant::now() >= deadline`). Contention is
not timing and neither is corruption.

**Check:** if the fix maps that whole branch to elapsed, it has replaced one flattening with
another — a busy target reported as a slow one. The branch must be split, or the PR must say
explicitly that it was not and why. This is named NOW so it cannot be discovered later as a
surprise.

### G4 — the corruption paths must NOT move
`InvalidRestore` appears 219 times in `backup.rs` at base (tests included). The overwhelming
majority are genuine invalidity: truncated archive, MAC failure, marker contract unparseable,
quarantine/owner already present, query failures.

**Check:** a diff that converts a non-timing path to the elapsed variant is strictly worse than
the defect being fixed — a corrupt backup reported as a slow machine gets retried until it
appears to work, which is the dangerous direction the issue itself names. Every converted site
must be a timing path, and the count of remaining `InvalidRestore` emitters should fall by
roughly the number of elapsed sites, not by more.

### G5 — F2 must be checkable, not asserted
"One operation budget" is a claim about structure. **Check:** after the fix, adding a twelfth
bounded step must not increase the operation's total worst case. If the PR claims F2, it should
show the derivation (per-step caps computed from one deadline) or a test that fails when a step
is added with an independent cap. If F2 ships as "we lowered the caps", that is not F2.

### G6 — the red must be OBSERVED, and it is constructible even though the flake is not
The issue's own evidence is a flaky gate, which cannot be summoned. But F1's red is deterministic
and cheap: drive a restore with a `process_timeout` small enough that a step certainly exceeds
it, and observe the CURRENT code answering `GHB002_RESTORE_INVALID`. That is the red, it needs no
flake, and it must be recorded before the fix.

**Check:** if the PR reports no red, or reports the gate flake as its red, F1 is unproven.

### G7 — sabotage reported as WHICH edit plus raw output
Per lane rule. At minimum I expect two:
- revert the elapsed variant at ONE emitter and require the F1 guard to go red;
- if F2 ships, an edit that restores an independent per-step cap and requires the F2 guard to go
  red.
A guard nobody has watched fail measures nothing, and a sabotage declared in a docstring but not
run is worth nothing — that one cost this milestone a re-review already.

### G8 — the guard must not be satisfiable by a second defence
The trap I walked into on #76: a guard whose subject is protected by something else entirely
passes for a reason unrelated to what it claims. **Check:** for each new guard, ask what ELSE
would refuse if the thing under test were removed. If a restore fails during archive verification
before ever reaching the bounded step, a "timeout is distinguishable" test measures the archive
check, not the timeout. The fixture must reach the bounded step and nothing earlier may refuse.

### G9 — timing claims carry their conditions
The issue's own ~80 s figure (80.52 / 79.38 / 81.95 across three isolated runs) is cited without
free_gb or concurrent-cargo. Any timing number in the PR must carry both. If the PR re-uses the
issue's figure, it inherits the gap and should say so rather than repeat it as though sourced.

---

## What must not move

- Corruption, MAC failure, truncation, marker-contract and quarantine/owner-exists paths keep
  answering `GHB002_RESTORE_INVALID` (G4).
- `InvalidBackup` and `Unavailable` keep their existing codes unless the PR deliberately changes
  the code alphabet and says so — that collision is pre-existing and out of scope here, but a
  silent change to it is a scope leak.
- The two production `ProcessWatchdog::start_with_cancellation` calls (:1152, :1273) drive
  `pg_dump`/`pg_restore` children; a change to their cancellation semantics is a different
  subject and needs its own justification.

---

## What would make me reject on sight

1. A new variant sharing a code with an existing one (G1).
2. `:1486`/`:1500` untouched while the PR claims complete elapsed coverage (G2).
3. The `connected > 1` branch folded into elapsed without a sentence about it (G3).
4. Any test whose assertion can pass or fail on a budget (F3's own subject, violated in its own
   PR).
5. No observed red, or the gate flake offered as the red (G6).

## My registered predictions, so the deltas score against something

- **P1:** the fix will be complete at the `tokio::time::timeout` wrappers and incomplete at
  `:1486`. (Most likely single miss.)
  **VOID — see DELTA-0. Disclosed to the writer pre-write; the prediction has lost its subject
  and is NOT scoreable. Left here unedited because a frozen prediction that turns out
  unscoreable is still part of the record.**
- **P2:** **VOID — see DELTA-1, and voided on my own reading rather than the orchestrator's
  bookkeeping.** the elapsed variant will get a new code — but I flag G1 anyway because the cost of
  being wrong is that the fix reads complete and is not.
- **P3:** F2 (operation deadline) will be partially delivered or deferred, since it is the
  structural half; if deferred, the PR should say so rather than let F1's landing imply both.

If a delta contradicts a prediction, the prediction was wrong and the criteria stay as written —
that is what freezing is for.

---

# DELTA — filled in after K's PR lands, never before

## DELTA-0 — 2026-08-19, BEFORE K's PR, disclosure event (not a review finding)
The orchestrator disclosed my two BASE findings to K pre-write: the `:1486` hand-rolled deadline
loop that a `timeout(` grep misses, and the `connected > 1` contention-vs-elapsed distinction.
G3 went to K verbatim as a scope requirement.

Reasoning on the record, and I agree with it: those are FACTS ABOUT THE BASE, not review
criteria. Freezing protects the independence of the CRITERIA; it does not entitle a reviewer to
let a writer walk into a known-in-advance incompleteness and then bill a serial cargo slot to
manufacture a delta we already had.

Cost accounted honestly, both directions:
- **P1 is VOID and not scoreable.** Its subject was handed to the writer. That is the price.
- **G2 STAYS AND STILL APPLIES.** A gate is not a prediction. I still check whether `:1486` is
  covered — I simply can no longer claim to have foreseen a miss that K was warned about. Same
  for G3: the requirement stands, the credit for anticipating it does not convert into a delta.
- **P2, P3 and G1, G4-G9 remain frozen and undisclosed.** The diff against those is where this
  review's value now lives.

Recorded before K's PR so the reduction in what I can claim is on the record BEFORE the diff
arrives, not negotiated after it.

## DELTA-1 — 2026-08-19, still before K's PR. Correction to DELTA-0's count.
The orchestrator reports the disclosure was WIDER than stated when I wrote DELTA-0: **G1's
substance also went to K**, verbatim, including "your test must assert at the CODE level, not the
enum level". True count of disclosed items is three, not two. DELTA-0 was written on the wrong
number; it is left unedited and this entry corrects it.

Revised bookkeeping:
- **G1, G2, G3 — gate stays, credit voided.** All three checks still apply and I still run them.
  What died is any claim to have foreseen something the writer was warned about.
- **Still sealed: P3 and G4-G9.**

**AND ONE ITEM THE ORCHESTRATOR'S CORRECTION STILL MIS-FILES, in my favour, so I am moving it
myself: P2 IS ALSO VOID.** P2 predicted the elapsed variant would get a NEW code. K has now been
told explicitly to assert at the code level. A prediction about how a writer behaves unprompted
cannot survive the writer being prompted — the same reasoning that voided P1, applied to P2. The
orchestrator listed P2 as still sealed; it is not, and I would rather correct a ledger in the
direction that costs me than let a scoreable-looking row stand on a disclosed subject.

So: **still sealed is P3 and G4-G9. Nothing else.** Of those, G4 (corruption paths must not move)
and G8 (the guard must not be satisfiable by a second defence) are where this review's remaining
value actually sits.

## DELTA-2 — 2026-08-19, #121 reviewed (87d8ebd+3ed5e08 off 07b1243). VERDICT: APPROVED.

Scored at source, not from the PR body.

| gate | result | delta |
|---|---|---|
| G1 (new code, not just new variant) | PASS | `GHB003_DEADLINE_ELAPSED`, unique; a test asserts at CODE grain with its own note that variant-grain can lie. Credit voided (disclosed). |
| G2 (`:1486` hand-rolled site) | PASS | covered by hand, comment names why a grep misses it. Credit voided. |
| G3 (contention not swallowed) | PASS, EXCEEDED | split into PURE `classify_exclusivity`, unit-testable without a database. I asked for a decision; the fix supplied a decision plus a seam. Credit voided. |
| G4 (corruption must not move) | PASS — **and my premise was wrong** | I built G4 on the issue's "uniform `InvalidRestore`". The census found FOUR variants and THREE codes. Exactly three sites moved, all `timeout(...).await.map_err(...)`. |
| G5 (F2 checkable, not asserted) | PASS | `step() = remaining().min(ceiling)` — one budget, real derivation. Exemptions (cleanup/release) are named decisions with reasons. |
| G6 (red observed) | PASS, EXCEEDED | red with test name, panic site, left/right — plus a harness control green in the same run. My RC2 rule, applied unasked. |
| G7 (sabotage = which edit + raw) | PASS | six rows, panic sites, reverted clean. |
| G8 (no second defence) | PASS | four rows say the guard falls **ALONE**, and the choke-point revert carries a DEATH CONDITION (three fall, cancellation stays green). The #76 trap checked prospectively and found absent. |
| G9 (timing claims carry conditions) | **NOT TRIGGERED** | no wall-clock claim is made; notably the issue's unsourced ~80 s figure is NOT reused. Recorded as not-applicable rather than as a pass. |

**P3 WRONG, scored against me.** I predicted F2 would be partially delivered or deferred. It was
delivered in full, with named exceptions rather than gaps. That was my last sealed prediction and
the only one I lost on the merits rather than to disclosure: P1 and P2 died because their
subjects were handed over, P3 died because I was wrong about the writer.

**Final ledger for this lane: 0 of 3 predictions scored a hit. 9 gates applied, 8 passed, 1 not
triggered, 0 blockers.** The gates were worth writing and the predictions were not — which is
itself the lane's finding. A gate is a question you ask the diff; a prediction is a claim about
someone else's work, and I was wrong about it every time I could still be scored.

**On the two measured nulls** (one wrapper mapping, the exclusivity loop's budget derivation —
nothing falls at unit grain): real unguarded seams, MEASURED rather than assumed, labelled
perimeter-not-strength, with the exact sabotage edit recorded for each, and the author scored
their own seal error where they had predicted a guard that does not exist. By the #76 standard
that is a seed, not a hole hidden behind a green. Not a blocker; it is where the follow-up aims.
