# G Agent Roster — M09 Status Roll-Call

Generated: 2026-08-19 from orchestrator-board.md @ 53d212d  
Updated: major board change synced 2026-08-19 (PRs #71/73/75/77 landed, new assignments)  
Source: board line references only. Re-synced on orchestrator ping — H/C/A DONE, K done, E/A/J/M new assignments.

## Roster (13 agents — WHO-IS-IDLE column added)

| Agent | Role | Files Owned | Current State | Blocking | Idle? |
|-------|------|------------|---|---|---|
| A | Fable med | `.factory/a-agent-m10-proposal.md` | DONE: flake-2 + M10-D1 design (blueprint accepted; D1 coherence via identity+length). **NEW TASK:** schema-digest print subcommand (issue-first, typed-unbuilt; from factory suggestion; evidence=C's throwaway-test pain during #74 ritual). | None | **ACTIVE** |
| B | Fable med | `.factory/b-agent-issue55-memo.md` | DONE: #55 memo + storm study review. **BLOCKING C's #74:** awaiting B verdict on red receipt + fold-mismatch ruling (C proposes attention-route not refuse). B's S4 sabotage (fixed idempotency) queued in A gap. | C's #74 response | IDLE |
| C | Opus high | wake.rs + local.rs pens | DONE: wake-flakes study. **DONE: #71 flake-3 fix** (commit 0236ffa, RED observed recorded==1, GREEN 4/4 guards 24/24 unit; SABOTAGE pins fall). **BLOCKED on B:** awaiting #74 ruling. Owes: journal count S4. | B's #74 verdict | **WAITING** |
| D | Opus high | `.factory/d-agent-storm-study.md` | READY: storm fix (position-5 spec final; rung A + Patch 1 share wall-clock epoch stamps + cross-check EDGES/COUNTS). 9 sealed predictions. H4/H5 dead by data. Patch 1 validates rung A denominator. | Position 5 scheduled | IDLE |
| E | Sonnet extra | `.factory/draft-second-story.md` | DONE: review (2 kills: hang self-heals, orphan child). **NEW ASSIGNMENT:** rehearsal authorized now; paid judge run AUTO-AUTHORIZED if rehearsal green. Block device = deterministic Blocked, graceful pause via NotPaused refusal. | None | **ACTIVE** |
| F | Sonnet 5 | `.factory/f-agent-m09-close-skeleton.md` | **NOW: close-doc lane.** PR #77 (K's journals option B: test+docs+README+CHANGELOG updates; 6 citations spot-checked, SHA256SUMS recomputed, zero stale). [APPROVED], ready for owner merge. Fixed stale docs (m09-seeds.md ~3/4 vs H's 12/13). | Owner merge | IDLE |
| H | Fable extra | `.factory/h-agent-base-measurements.md` (main) | **BASE DONE** (~13min, serial). STORM 4/10 iso 2/3 suite (all READ 10060 TimedOut; H4 mutation-only H5 dead). SLEEPER 9/10 iso 3/3 deterministic (wake_http.rs:822). SWEEPS 0/10 0/3 (no stochastic; C's seam burden). Disk: 19.22GB free, ≥15GB for pos 5. Standing by ordered-runs-only. | None | IDLE (standby) |
| J | Fable extra | `.factory/j-agent-flake2-review-prep.md` | **DONE: flake-2 review** (PR #73 final signoff GREEN, five-gate grep all pass). **NEW ASSIGNMENT:** wake_wait design (product hole: never consults receipt; timeout answers lease-alone; no surface distinction "nothing happened" vs "burned"; owner/PRD decides). | None | **ACTIVE** |
| K | Opus 5 med | `.factory/k-agent-doctor-journals.md` | DONE: queue item 4 (git empty-dirs). OPTION B APPROVED: test+per-archive READMEs+SHA256SUMS+lib.rs generator+bare-transcript labels. PR #76 (branch 5d8ed46+167a1a8) awaits N's verdict + cargo slot. PR #77 merged main. Disk: 0.9GB deleted. | N's #76 verdict + cargo | WAITING |
| L | Opus 5 med | `.factory/l-agent-storm-review-prep.md` | PREP DONE. **ACTIVE TASK:** close-doc cold check (grep dead claims + dimension violations against M's table; severable when position-5 opens). 5-gate storm contract. Denominator refinement adopted (epoch clock req). | Position 5 opens | **ACTIVE** |
| M | Opus 5 med | `.factory/m-agent-*.md` | LEDGER SEALED. **NEW ASSIGNMENT:** ledger close-out (hold kill-bar WORDING with author at seal; CONV: CONV-2 survives one-writer, CONV-1 dented per-request-open=cache-coherence; SF-1+SF-2 legality-shaped guards ratified). Score H's measurements. | Close-doc scheduled | **ACTIVE** |
| N | Opus 5 med | `.factory/n-agent-oracle-audit.md` | **AUDIT DONE.** **ACTIVE REVIEWER:** #76 (K's journals branch 5d8ed46+167a1a8, awaiting verdict). Receipt assertion in #74 (non-blocking per B). Scores C's 0/10 suite vs base 3/3. SF-1 documented. | #76 review | **ACTIVE** |
| G | Haiku, clerical | `.factory/g-agent-roster.md` (THIS) | **NOW:** re-sync roster + cargo queue on orchestrator ping. **NEW:** WHO-IS-IDLE column (allocation at glance). Standing tasks: (1) re-sync on ping; (2) cargo queue visibility. | None | IDLE (by design) |

## Cargo Queue (Scarce Resource — Single Priority)

Current order (H is sole runner):

| Position | Agent | Task | Status | Next Blocker |
|----------|-------|------|--------|---------|
| 1 | H | Base measurement (W1 protocol: N≥10 iso, 3x suite, verbatim failure text) | **DONE** (~13min, serial) | — |
| 2 | C | Red observation + flake-3 fix + sabotage (commit 0236ffa) | **DONE** (RED observed, GREEN 4/4 guards 24/24 unit) | — |
| 3 | A | Flake-2 work (N=20 final vs base) | **DONE** (PR #73 20/20 vs base) | — |
| 4 | K | Journals option B (#75 moved up, now #77 merged) | **DONE** (PR #77 landed+merged main df5e431) | — |
| 5 | C+D | Flake-3 fix+sabotage + storm re-baseline + rung A + Patch 1 (position-5 coupled) | **QUEUED** (C blocked on B for #74; D standby) | B's #74 verdict |
| 6 | E | Story rehearsal (scripted, free queue) | Queued | Flake-critical work + judge authorization |

**Queue discipline:** No cargo for G (Haiku clerical). Orchestrator gates order. H runs only. Disk gate: ≥15GB required for position 5; cleanup a-agent+k-agent first (lanes closed by then). Parent-of-53d212d N≥10 for C-P1 pending (CANCELLED per M: 0/N-vs-0/N proves nothing).

## Orchestrator Delegation & New Product Decisions

**Delegation summary:** orchestrator OWNS product-quality decisions (decide best-for-product independent tokens/time; escalate only new-money/owner-vision/final merge).

**Key rulings (2026-08-19):**
- 0. RING-AFTER-APPEND REORDER: REJECTED (unrecoverable loss > benign transient)
- 1. CITE-or-MARK = official docs rule (docs that cannot lie > prose comfort)
- 2. CHANGELOG label site INCLUDED (honest shipped doc > frozen wrong)
- 3. classify_layout: .tmp/active RECOVERABLE on open (transient dirs, zero integrity; separate guard+sabotage for blobs STRICT)
- 4. REBASE at milestone close (linear history, full gate after)
- 5. JUDGE RUN: rehearsal authorized now; paid run AUTO-AUTHORIZED if rehearsal green
- 6. M10: approved; premise KEEP multi-process; D1 supply coherence (stat/head check under per-op lock)
- RDV-EQUAL: approved IN-M09 (PR 2 live, issue #74, C writes, B reviews)
- JOURNALS: option B approved (K executes in #75, F reviews, then #77 merged main)

## Notes

- PRs landed: #71 (flake-3), #73 (flake-2), #75/#77 (journals option B merged to main df5e431)
- #74 red observed (recorded==1 vs 0): recorded at wake.rs:719, C's flagged reorder paid off first run (silent-defect measured not inferred)
- PRs #75/#77 merged main; others in flight (C's #74 blocked on B)
- Oscillating-disk rule: cleanup judged twice under load; one green ≠ proven (transient exhaustion masquerades as flake)
- PRE-EDIT ANNOUNCEMENT rule: name pen row before first keystroke (compliance-based, not mechanism yet)
- GREP-AS-GATE rule: after retraction/correction, grep dead claim's name in owned artifacts (executed like test)
- Stash@{0} = historical reference; never pop; delete after flake-3 lands
- Stand-by constraint (N found, adopted): A's fix repairs harness, never weakens oracle (reason+burn+proxy count asserted; no quarantine/retry-until-green)
- Misreading block (M sealed): 0/13 sweeps ≠ window-3 gone (belt is legality-blind; deterministic seam untested)
- Ledger practice: kill bars phrased RATE-EXCLUSION bounds (0/N~10 excludes >~26%, not establishes zero); M co-holds kill-bar wording at seal
- SF-1 pattern: legality-shaped guards (silence indistinguishable from loss; wake, executor retry, sweep instances; N's per-guard assertions = reference doc)

## Status Summary

**Roster state:** synced 2026-08-19 (corrected A/K/N stale rows)  
**Cargo queue:** positions 1-4 DONE; position 5 (C+D coupled storm) QUEUED, C blocked on B  
**Key state:** H base DONE; C/A flake DONE; A M10-D1 DONE (premise KEEP, D1 coherence accepted); K #76 awaiting N verdict; N active reviewer for #76  
**Active now:** A (schema-digest print), E (rehearsal), J (wake_wait), L (close-doc cold check), M (ledger close-out), N (reviewing #76).  
**Idle by design (position-5 storm):** H, B, D. K waiting N verdict.  
**Re-sync protocol:** batch to orchestrator pings only (row-by-row chase too costly when assignments move fast).  
**Standing tasks:** (1) re-sync on orchestrator ping; (2) cargo queue visibility; (3) WHO-IS-IDLE tracking
