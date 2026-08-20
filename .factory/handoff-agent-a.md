# Factory handoff packet - Agent A (beacon, not queue)

Updated: 2026-08-19 morning. Session restarted, reported in to orchestrator, AWAITING ASSIGNMENT.

PROVENANCE RULE (binding): only what this agent wrote. ASCII-only.

## Staleness anchors - CHECK THESE, do not judge by elapsed hours

| Anchor | Value when written | How to check |
|---|---|---|
| main HEAD | efd85d0 (M09 A merged, #70) | `git log --oneline -1 origin/main` |
| work branch | issue-m09-arming-the-alarm @ 53d212d, NOT rebased on main | `git log --oneline origin/main..issue-m09-arming-the-alarm` |
| M08 issue #63 | CLOSED | `gh issue view 63 --json state` |
| orchestrator board | .factory/orchestrator-board.md, mtime 2026-08-19 08:58 | read it; board outranks this packet |

## Where the pen is
NOT WITH ME. M09 stabilization phase runs under the ORCHESTRATOR session
("Orquestrador GraphHelm"). The board (.factory/orchestrator-board.md) is the live source
of truth for roles and rules; this packet only records what A knew when it wrote.

## My state (2026-08-20, night closed — CURRENT)
- #143 MERGED: PR #145 squashed as main e60a27d, closing #143 + #146. MAIN GREEN.
  The regression lane end-to-end: E found it (3/3 deterministic), mechanism found by
  arithmetic (opens always serialized; the M09 guard's green lived on redundant work
  #137 removed), fixed with the shared fast path + exclusive upgrade w/ redo, the full
  gate caught my mirror-drift hole on the new rule's first application, D's review
  landed one real finding (upgrade re-classifies under its own lock — fixed f49b2ae),
  residuals filed as #147 with reachability triggers. Slot handed to C (#96) by
  explicit custody line. Night total for this lane: #87c1 merged (warm reads
  100x-1100x) + its regression found and fixed within hours, one defect class killed
  (clean-check asks the real planner).
- I am IDLE. Next known: M11 decision on #87 commit 2 (requirements sealed); #147
  triggers if publish's match-check ever changes.

## My state (2026-08-20 early — superseded)
- #143 (main red, caused by my #137): FIXED + PR https://github.com/stabem/GraphHelm/pull/145
  (2 commits: shared fast-path open for provably-clean stores w/ exclusive upgrade+redo;
  planner-split killing the mirror-drift class after the FULL GATE caught .tmp hole —
  the measurement-never-substitutes-gate rule paid on first application). Guard red
  3/3->green; sabotage reproduces original numbers; gate GREEN 420 lines. D reviews on
  the PR (4 self-named attack vectors incl. markers_clean residual mirror as seed);
  orchestrator merges on D's verdict. SLOT released.

## My state (2026-08-19 late — superseded)
- M10 D1 design ACCEPTED as blueprint (.factory/a-agent-m10-d1-design.md, worktree +
  shared copy). Orchestrator resolved the embedded decision: line-boundary truncation
  stays accepted as today, default OFF confirmed — M10 replicates today byte-for-byte;
  hardening is a separate behavior change. Implementation opens when M09 closes.
- CURRENT: BOTH LANES CLOSED with J's double [APPROVED]. #72 LANDED: ef51193
  fast-forwarded onto milestone branch (d10916b -> ef51193) and PUSHED to origin.
  #85 (schema digest) approved, merge on orchestrator's desk. Seed 9 filed with F
  (GHCLI017-on-stdout), independently verified by F, endorsed by J. I am IDLE.
  Next: M10 implementation when M09 closes; DISK PREP + storm are others' lanes.
- #78 DONE LOCALLY (superseded detail): commit 4720dc0 on issue-78-schema-digest-print @ f2efb94
  (rebased to new main tip). Full evidence: red 2/2 no-impl, green 2/2, sabotage red on
  both guards' own asserts (schema_cli.rs:1276+:1305), suite 29/29. PUSH+PR SUSPENDED:
  disk hit 9.0G < 10G floor after the suite run — stopped and reported per rule.
- #72 ARM NOT STARTED (zero cargo): branch issue-72-wake-oracle-discrimination @
  d10916b (milestone lane), typed work parked as .factory/a-agent-issue72.patch
  (4 hunks: phase-3 delay seam + serve_with_env + committed green-half test); run plan
  sealed in .factory/a-agent-issue72-evidence.md. Landing target = milestone branch,
  CONFIRM with orchestrator before push.
- #87 COMMIT 1 MERGED: PR #137 squashed as main 6d389eb (against ea23101 exact). D1
  commit-1 lane CLOSED. Commit 2 (serve long-lived handle, 38 call sites) deferred to
  an M11 decision, requirements already sealed: CONV-1 substitution, ABAB interleaved
  measurement, per-row machine fields, with_caller attribution, revisit the named
  trigger (staleness + O(suffix) contention). I am IDLE, queue empty.
- #87 COMMIT 1 SHIPPED TO PR (superseded detail): https://github.com/stabem/GraphHelm/pull/137 (ea23101,
  base main; merge = orchestrator's). Sample of record = run-3 (a-87-after-rows-run3.txt,
  sealed after THREE contamination windows adjudicated: D's cargo, H's exe-lock verdict,
  J's attempt-1 -> runs 1+2 discarded, run-3 in J's coordinated gap, J-confirmed +
  mtime-resolved sighting). M scored: 100x-1100x confirmed; three-regime split leads the
  PR (1.45x/50x point-reads per run-3; read_replay 19x/50x honest; open O(n) by scope).
  Commit 2 requirements recorded: CONV-1 substitution, ABAB, per-row fields, with_caller.
- #87 COMMIT 1 (superseded detail: was) EXECUTED: ea23101 (NOT pushed; awaits
  M's scoring + orchestrator flow). 52/52 green, 3 impl sabotages (2 surgical), paired
  measurement done: warm reads 1061ms->0.42ms @1k, NEAR-FLAT 0.36/0.42/0.64ms across
  100/1k/5k (the journal-load share claim); cold open still O(n) BY SCOPE; baseline-5k
  honest NOT-RUN (fixture build >590s = the O(n^2) measuring itself). First-round red
  was MY wrong fixture scope (execution-1 vs execution-fixture), one root, trace-
  diagnosed, recorded. Raw rows + evidence in shared .factory/. Unexplained raw for D:
  after-open ~30% faster than baseline-open, outside the claim.
- M10-D1 = ISSUE #87, TYPED-UNBUILT DONE (commit 1, superseded detail): branch issue-87-verified-prefix-
  cache @ 0f4e7fe, patch parked .factory/a-agent-issue87-core.patch (+471/-21, events
  crate only), slot-time plan in .factory/a-agent-issue87-evidence.md. SCOPE SPLIT
  awaiting orchestrator ratification: commit 1 = events-crate cache (3 loads -> 1+hits
  per handle); commit 2 = serve long-lived handle (38-site ripple, separate). 8 guards
  committed incl. 2 named expected-greens (SC1/SC3), SC6 sabotage-the-cache blade, SC7
  budget property, fresh-handle-as-oracle error comparisons. At slot: guards green ->
  impl-sabotage reds per guard -> full crate suite (triage R1-widening test breaks as
  named expected-greens) -> paired FRESH baseline + AFTER (M's cells first). BINDING:
  same-session/same-disk pair; median AND p99 AND max.
- STORM LANE: H's UNCONDITIONAL CO-SIGN issued (7/7 control logs row-verified in shared
  a-storm-controls/; resume settled via journal third-apparatus; M scored both cells +
  guard verbatim in record; D's own O-prediction missed 10x -> structural cost feeds
  M10-D1; tail headroom ~1.5x at p99 vs ~8x at median = live finding with D).
- STORM LANE COMPLETE (record closed: .factory/a-agent-storm-execution-record.md).
  Final state: Run 0/1 pair -> code exonerated (disk not specifically established, M's
  label fix); Run 2b with D's per-pid sink 10/10 PASSES; D1 headroom ~8x (8xO=0.56-0.64s
  vs 5s); Run 3 3/3 PASSES; controls revised by D, both sabotages observed failing;
  FINAL REVERT verified (empty diff on 4 files, ledger intact, clean bin rebuilt).
  3 instrument defects found by execution (sink create_new/multi-pid; C1 sabotage
  unobservable; C2 >=1 redundant-blade), all D-acknowledged and fixed/ruled. I am IDLE.
- STORM EXECUTED (superseded detail; transferred from H; my session's permissions allowed the builds — no
  bypass; the anti-laundering rule was honored by deferring to my own session's controls
  and by the standing order to FULL-STOP if denied). Record:
  .factory/a-agent-storm-execution-record.md. Headline: Run 0 flip 0/10 @ 53d212d on
  today's disk -> M's cell fired: DISK explains 4/10->0/10, code exonerated. Run 1 clean
  0/10 @ ef51193. Run 3 probe-off 3/3. Run 2 BLOCKED: instrument sink bug (create_new vs
  multi-pid env inheritance; cli_start creates the file, serve panics) -> 10 non-results;
  fix is D's (3 candidate shapes sent). Controls C1/C2 sabotage-form rulings also still
  D's. Tree: D's 10 edits applied + rebuilt; stash ledger @{0}=candidate-fix intact;
  fingerprint sha256 65c7ead2... in .factory/a-storm-edits-fingerprint.diff.

## My state (2026-08-19, flake #2 EXECUTED — see above for current)
- Branch issue-19-flake2-sleeper-receipt-wait in my worktree (base 53d212d, NOT pushed):
  c9c1188 capture instrumentation, dca0461 receipt-wait fix.
- Evidence: red 2/2 pre-fix (same form as H's base), 20/20 green post-fix, S10
  connect-only sabotage red (2 vs 1), deadline-zero sabotage red 3/3, wake_http suite
  19/19. Oracle untouched. Debt issue #72 (delay-hook + SJ2/SJ3/SJ5, blocked on
  serve/wake.rs pen).
- J's verdict: UNCONDITIONAL [APPROVED] (J verified the raw am3-logs directly; report
  matched artifacts 1:1). AM3 = case (c): 0-in-20 suite (base 3/3) + 0-in-20 standalone
  (base 9/10), my tree alone; product attribution no new evidence.
- LANE CLOSED 2026-08-19: PR #73 landed fast-forward on milestone branch (aac0d67 ->
  576e553), J final signoff 5/5 greps, full gate GREEN on landed tree (exit 0), final
  N=20 on landed tree 20/20 by name, zero deadline-fired forms. Three closure cells
  named: my fix alone 0/20+0-in-20 (pre-C), C's alone 0/10, final tree 0-in-20.
  Reframe honored: my change = harness hardening + diagnostics; rate drop = C's fix.
- serve/wake.rs pen HANDED BACK TO C (orchestrator order: owner-approved defect fix #74
  outranks hardening debt). #72 DEFERRED until #74 lands. My ring-instrument and S7
  delay-hook drafts stay parked in .factory/.
- Ring-after-append: delegated to orchestrator; my product argument sent (position:
  do NOT swap — trades a benign already-absorbed transient for an unrecoverable
  burned-but-never-rung wake loss; current design fails in the safe direction).
- Board note accepted: final N=20 was routed to H but run by me (measurer==author
  class, flagged even with clean result); raw logs staged for M's scoring.
- N's S4 journal-count run by me at aac0d67 (detached, reverted): armed=15 consumed=1,
  N's mechanism CONFIRMED, belt blind (passed green over 14 vanished consumptions).
- Unchased observation: GET /health THROUGH the counting proxy hangs the test (2x
  reproduced); connect-only and MCP via proxy fine. Reported to orchestrator, not mine.

## My state (2026-08-19, after M10 proposal)
- DELIVERED: M10 proposal at .claude/worktrees/a-agent-c35d10/.factory/a-agent-m10-proposal.md
  (worktree detached at 53d212d). Accepted by orchestrator, on the board, owner decides.
  Key facts inside: the 48%/15->36ms/3x numbers have NO repo source; the 3x is structural
  (driver.rs:66 = read_replay_stream + next_sequence + append = 3 full-journal load_state);
  serve opens a fresh store per request and its M05a lock justification is stale
  (open_inner releases locks at local.rs:390-397).
- CONVERGENCE noted by orchestrator: my per-request-open finding = same mechanism D found
  behind the api_http storm. One mechanism, two symptoms.
- NEXT ROLES (two): (1) WRITER of flake #2 fix, `a_sleeper_wakes_on_a_peer_append_...`
  (wake_http.rs:690) — PRE-ASSIGNED, activates when W1 captures the failing assertion
  TEXT (the text picks the mechanism). My per-mechanism plan: .claude/worktrees/
  a-agent-c35d10/.factory/a-agent-flake2-plan.md (4 branches + mechanism-blind step
  zero: sidecar stderr capture, ring() error naming, dead-child check). Reviewer: B or
  orchestrator designates. (2) standby REVIEWER of D's storm instrumentation (with B).
  Read-only until W1 numbers land. Do not pre-write in C's or D's areas.
- Tier note (owner): A+B = Fable medium, C+D = Opus high, E = Sonnet. Subtle reasoning
  routes to A/B.

## Rules in force I verified on arrival (board + orchestrator transcript)
- Only W1 runs cargo builds/tests while flake base measurement runs. Everyone else READ-ONLY.
- Pair serve is OFF ON PURPOSE (less machine noise during timing measurement). Do not restart
  it as "recovery" - down is the intended state. .factory/bin/ is gone from the tree too.
- stash@{0} on the main checkout = unproven flake-#3 candidate fix. Never pop onto a
  measurement run.
- Causal claims about flakes need: with-number, base-number, N, scope. Else returned unread.

## Coordination state on arrival
- Orchestrator: local_2c498863-e1b8-46c8-b8c0-5237bcad87ad (running)
- B Agent: local_95f9697e-62b8-4a13-9cc7-735da9739022 (archaeology of #55, clean reviewer)
- C Agent: local_0d501afb-ab6f-47d5-8ab1-0c6b9d584c9b (wake_http flakes mechanism, read-only)
- D Agent: local_a99cb12f-66cd-4511-9ea1-65f381ed9240 (api_http storm mechanism, read-only)
- A (me): no task yet; sent arrival report, waiting for a disjoint angle.

## REPORT CHANNEL (owner order, 2026-08-20, reinforced — binding forever)
EVERY report goes to the ORCHESTRATOR session, NEVER to the owner. The owner reads what
the orchestrator consolidates; review/verdict/handoff/status all travel by send_message
to the orchestrator. (A review report reached the owner today; the existing
owner-is-not-a-channel rule is now owner-reinforced.)

## SKILLS DIRECTIVE (owner, 2026-08-20, binding on every future report incl. c2)
1. EVERY report opens with "SKILLS USADAS: [list]" (e.g. codebase-memory
   (get_code_snippet on local.rs), caveman) — reports without the line come back.
2. CODE LOOKUPS: mcp__codebase-memory-mcp__search_code/search_graph/trace_path/
   get_code_snippet BEFORE any location-grep, cited; manual only for known exact-line,
   git provenance, or a DECLARED fallback. NEVER index during measurement-in-flight.
3. CAVEMAN in reports. (Tools may need ToolSearch load or session restart — verify at
   next lane start, declare fallback if unavailable.)

## Tooling (owner-ordered, 2026-08-19, relayed by orchestrator)
- caveman-full compression for inter-agent messages and orchestrator reports.
- codebase-memory-mcp installed user-level; DO NOT index while the no-cargo rule stands
  (indexing is CPU-heavy, W1 measures timing flakes). After rule lifts: restart session,
  prefer graph queries over raw file reads.
- humanizer plugin for owner-facing prose only.
- agent-reach at ~/.agent-reach-venv/Scripts/agent-reach.exe (call exe directly).

## Standing facts still true from the last packet
- Judge runs are PAID, one at a time, owner-authorized.
- Disk fills; `df -h /f` before blaming a linker error.
- Assert at the finest grain the question has; sabotage a guard to prove it measures.
