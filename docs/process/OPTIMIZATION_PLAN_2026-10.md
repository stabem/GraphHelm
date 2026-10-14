# Optimization plan, October 2026

Summary: one ordered list of tickets that removes dead code, fixes the measured O(n²) paths,
shrinks what every agent must read or send, and makes agent status a single machine-verified
source. Written from six read-only audits of head `c90cd4de` (2026-10-13). Nothing here was
built or run; every claim carries the path and line it was read at, and every ticket names the
command that proves it.

How to use this page: a lane claims one ticket, opens its issue with the ticket's card, Codex
implements, Claude reviews ([DELIVERY.md](DELIVERY.md)). Tickets inside a wave are independent
unless a dependency is named. Waves are ordered by value per line changed, and wave A is the
owner's priority. Line counts are estimates from the audit, not promises.

## 0. Rules for this work

- **Dead means proven dead.** A zero-reference grep is a lead. Before deleting, account for clap
  subcommands (`apps/cli/src/args.rs`), MCP dispatch (`apps/cli/src/commands/mcp/tools.rs`),
  HTTP routes (`apps/cli/src/commands/serve/mod.rs:738-882`), hooks (`plugins/graphhelm/hooks`),
  persisted and wire shapes (`schemas/`, `core/protocols/src/event.rs`). Record `Retirement:` in
  the PR (AGENTS.md, "Retire displaced code").
- **Slop means tokens without a decision.** Prose, comments, tests and tool descriptions that an
  agent must read and that change nothing it does. Measured in bytes loaded per session.
- **Proof before claim.** Each ticket names its proving command. A byte-identical contract
  (digest, error pointer, CLI help text) gets a golden test before the refactor, not after.
- **No cargo stress on the shared machine.** Build slot, `--jobs 6`, `--test-threads=2`
  ([LANES.md](LANES.md) §3). Clippy only on touched crates.
- **One PR per ticket.** No umbrella PRs. A ticket that grows past its card is split.

## Wave A. Agent status: one source, machine-verified, crash-safe

Today there is one durable store (the append-only event log) but five status vocabularies
written by four different parties, and the only place a task's step (plan, implement, review,
merge) is computed is the browser (`apps/studio/src/runtime/team-tasks.ts:566-786`). The
Runtime admits `task.*` records but never folds them, so the CLI, the MCP `status` tool, the
briefing and the hooks cannot answer "what step is issue N in". The last 25 commits touching
the fold files are all `fix(studio)`. Open issues #903, #919, #1058, #1118 and #1138 are
symptoms of the same shape.

| Surface | Writer | Verified by | Stale on crash |
|---|---|---|---|
| Node state (`NodeState`, `core/protocols/src/simulation.rs:24-45`) | Runtime fold | deterministic replay | aggregate reads `running` forever (`apps/cli/src/commands/execution/mod.rs:935-942`) |
| `task.*` records (`tools/task-record/task_record.py:40`) | the lane, the orchestrator as the lane, the reconciler as the lane | shape only (`apps/cli/src/commands/execution/documents.rs:127-330`) | yes: no lease, heartbeat voluntary |
| `run_team_reported` (`plugins/graphhelm/hooks/run_team.py:29`) | the agent | nothing (`acceptance: unobserved`) | yes |
| Lane liveness (`silent`, `free`, `stale`) | nobody; Studio derives from record age | three different thresholds in three files | n/a |
| Session ended (`plugins/graphhelm/hooks/session_hook.py:639-650`) | SessionEnd hook | idempotency key | SessionStart emits nothing; hard crash emits nothing |
| Journey live session (`apps/cli/src/commands/journey_live.rs:1621-1633`) | Runtime, file with `expiresAt` and `pid` | TTL, reaped | no (the one surface done right) |

### A1. Runtime-owned task projection (depends on nothing; A2, A5, A7 depend on it)

- Paths: new `core/execution/src/tasks.rs`; route `GET /v1/executions/{id}/tasks` in
  `apps/cli/src/commands/serve/{mod,routes}.rs`; MCP tool `tasks` in
  `apps/cli/src/commands/mcp/tools.rs`; `graphhelm execution status --tasks`;
  `apps/studio/src/runtime/team-tasks.ts` becomes a reader of the route.
- Promise: the step, lane, PR, head, claim sequence and last heartbeat of every task come from
  one Rust fold, and the Studio shows what the CLI shows.
- Proof: port `apps/studio/src/runtime/team-task-events.test.ts` cases into
  `core/execution/tests/tasks.rs` as a golden corpus; one Vitest cell asserts the Studio's
  reader equals the route reply on the same fixture.
- Keel row: new public surface (full card, name the route and tool). Size L.

### A2. Claims carry a lease

- Paths: `apps/cli/src/commands/execution/documents.rs:268-291`;
  `extensions/builtin/graphhelm-development-contracts/schemas/task-event.schema.json`;
  `core/events/src/sweep.rs:47-124`; `core/events/src/projection.rs`;
  `plugins/graphhelm/hooks/tasks_reconcile.py:206-222`.
- Promise: a `task.claimed` without a heartbeat inside its lease becomes `overdue_exception`
  on sweep, and the reconciler releases it as `graphhelm`, never as the lane.
- Proof: Rust test "claim, no heartbeat, sweep at lease+1s emits overdue; heartbeat at lease-1s
  emits nothing"; `test_lane_stop_hook.py` cell for the release.
- Keel row: persistence and compatibility (expanded route, Law 3 test). Size M.

### A3. Heartbeat is automatic

- Paths: `plugins/graphhelm/hooks/lane_stop_hook.py:107`; `session_hook.py:wait_for_lane`
  (`:349-400`); `tools/codex-launcher`; `tools/task-record/task_record.py:49`.
- Promise: a live lane heartbeats without being told; the 300 s cooldown has one owner (the
  Runtime reply), not a constant copied into the hook.
- Proof: `test_lane_stop_hook.py` asserts one `task.heartbeat` lands after Stop and a second
  within 300 s is refused.
- Keel row: bounded code change. Size S.

### A4. Authenticated lane identity

- Paths: `apps/cli/src/commands/serve/mod.rs:300-336,963-973,1528-1550`;
  `tools/task-record/task_record.py:16-19,659-660`;
  `apps/cli/src/commands/execution/documents.rs:150-186`;
  `plugins/graphhelm/skills/graphhelm-team/SKILL.md:32-40`.
- Promise: a `task.*` record names the lane that holds the credential, so no session can write
  as another lane. The orchestrator records `claimed` as itself with `assignedBy`; the lane
  records its own first `claimed`. `task.review_assigned` and `task.closed` get an identity rule.
- Proof: CLI tests "shared token + task.claimed is refused" and "credential for lane A naming
  lane B is refused".
- Keel row: permissions and security (expanded route, JPD flow). Size M. The same defect class
  as #1129 to #1133, now closed for task records.
- Landed as #1148: the credential is derived (`sha256(owner token, execution, lane)`) and minted
  by `graphhelm tasks credential`, so no new persisted state and no Runtime restart; the Runtime
  verifies by recomputation. Open for A2/A6: the hooks that call `task_record.py` need the lane's
  credential file minted, and `task.review_assigned` / `task.closed` keep today's identity rule.

### A5. One identity namespace

- Paths: `plugins/graphhelm/hooks/session_hook.py:582-620,639-650`;
  `apps/cli/src/commands/tasks.rs:74`; `apps/studio/src/App.tsx:2359-2366`;
  `apps/studio/src/runtime/run-team.ts:143-152`.
- Promise: SessionStart emits `agent_session_started{lane}`; `agent_session_ended` carries the
  lane; the Studio joins on lane, so a Claude Desktop session end sets `lane.endedAt` (today
  only Codex launcher runs do, because hook actors are `{host}-session-{id}` and task actors are
  the lane name). Fixes the root of #903 and #1138.
- Proof: Vitest cell "Claude Desktop session end sets endedAt"; hook test for the started
  signal and its idempotency key.
- Keel row: new event kind (full card). Size S.

### A6. Hooks fail loud and bind every lane

- Paths: `plugins/graphhelm/hooks/task_hook.py:217,295`; `subagent_hook.py:204,273`;
  `lane_stop_hook.py:94,116-121,137`; `keel_record.py:55,79`; `keel_stop_hook.py:87,123`;
  `plugins/graphhelm/hooks/codex-hooks.json:104-167`.
- Promise: a bound hook that cannot deliver exits 1 with the same stderr line; title-bound and
  `team.json`-bound lanes emit task events (today `binding()` is called without the payload, so
  only env-bound lanes do); the reconciler's JSON is recorded as `tasks.reconciled` instead of
  discarded; the Codex manifest has the same Task and keel hooks and a byte-identical
  `lane_stop_hook.py` (CI test compares the two files).
- Proof: `test_task_hook.py` cell with `team.json` plus `session_title` and no env; cell
  asserting exit 1 on a refused POST.
- Keel row: bounded code change per hook. Size S.

### A7. One silence budget (depends on A1, A2)

- Paths: `apps/studio/src/runtime/lane-bars.ts:27`, `agent-board.ts:8,10`,
  `stage-health.ts:79`; `plugins/graphhelm/hooks/session_hook.py:284`;
  `apps/cli/src/commands/serve/monitor.rs:33-37,259-264`.
- Promise: liveness (`silent`, `free`, `stale`) is computed once, in the Runtime, from the lease
  and declared budgets, and served on the tasks route; the Studio and the hooks read it. Today
  there are four thresholds (30 min, 15 min, 2 h, 30 min waiter) in four files.
- Proof: a source guard, like `core/process/tests/no_console_window.rs`, fails if any of the
  four files declares a liveness literal.
- Keel row: bounded code change. Size S.

### A8. Docs say what the code does

- Paths: `docs/process/LANES.md:259-262`; `docs/process/DELIVERY.md:281-377`;
  `plugins/graphhelm/skills/graphhelm-team/SKILL.md:32-40,53,60`; `plugins/graphhelm/README.md`.
- Promise: `review_assigned` is documented as folded into `pr_opened` (the code already does
  it); the lease and credential rules from A2 and A4 are written where lanes read them; the
  "the Runtime cannot stop impersonation" sentence goes when A4 lands.
- Proof: a grep guard that the command table in LANES.md §4 equals `task_record.py KINDS`.
- Keel row: docs. Size S. Lands last in the wave.

## Wave B. Retire the gate era

The gate, receipt store, merge-proof and presser were retired by owner order on 2026-09-24
(AGENTS.md:85). Their code, tests and docs still ship and still cost every agent that greps.

### B1. Delete the gate runner

- Paths: `ci/gate.ps1`, `ci/gate-*.tests.ps1` (30 files), `ci/slot-lock*`, `ci/classify-run*`,
  `ci/run-class.ps1`, `ci/select-scope*`, `ci/run-ps-suites*`, `ci/fixtures` for the gate; keep
  `closing-keywords.ps1`, `postgres.ps1`, `tool-versions.json`, `required-features.ps1` and any
  script with a live caller (grep each name across AGENTS.md, docs, scripts, hooks first).
- Promise: `ci/` drops from about 34,300 to about 4,000 PowerShell lines and no document names
  `gate.ps1` as a step.
- Also removed with it: `apps/cli/src/commands/gate/` (`graphhelm gate classify-red`, 275
  lines), `args.rs:52,831-870`, `tests/gate_classify_red.rs`, `tests/fixtures/classify-red/`;
  `core/architect/src/judgment/red.rs:7,121,462` and
  `core/architect/tests/red_banner_producer.rs` (they parse gate banners);
  `tools/acceptance-map/src/lib.rs:337,345`; `docs/acceptance/gate-red-classification-2026-09-18.md`;
  `docs/operations/QUALITY_GATES_AND_DEPLOYMENT.md` gate sections.
- Do not touch: `NodeType::Gate`, `EventKind::GateVerdict`, `GateCertified`
  (`core/protocols/src/graph.rs:230,331`, `event.rs:232-233`), `tests/gate_http.rs`,
  `commands/quality.rs`. Those are graph Gate nodes, a product feature and a persisted event
  kind.
- Proof: `cargo metadata --locked` green; `graphhelm --help` no longer lists `gate`;
  `grep -rn "gate.ps1" --include=*.md --include=*.rs --include=*.ps1 .` returns only history
  docs that B4 removes.
- Keel row: public CLI surface removed (full card, name it). Size M. The `gate.classify-red`
  command id disappears; record it in `docs/releases`.

### B2. Remove the freeze guard and ci-canary

- Paths: `core/quality/src/lib.rs:466-540` (`freeze_violation`, `freeze_violation_with_lockfile`,
  `RUN_MANIFEST_STORE` pointing at `.factory/gate-runs/`); `core/quality/tests/{freeze,
  freeze_enforced,shared_source_invariant_predicate}.rs`; `tools/ci-canary/` and `Cargo.toml:32`;
  comments at `tools/source-invariants/detect.rs:13,37`.
- Promise: about 1,200 lines gone; no production caller existed (callers are the three test
  files only).
- Proof: `cargo test -p graphhelm-quality --locked -- --test-threads=2` green;
  `grep -rn "ci_canary::" .` empty.
- Keel row: bounded code change with a test deletion record naming the observer that still
  covers each obligation (none do; say so). Size S.

### B3. Instruction diet: AGENTS.md 297 to about 150 lines

- Paths: `AGENTS.md`, `CLAUDE.md`, `docs/process/DELIVERY.md`, `docs/process/LANES.md`,
  `docs/keel/KEEL_SPEC.md:3-7,61`, `extensions/builtin/graphhelm-development-contracts/skills/keel/REFERENCE.md`.
- Promise: every Claude and Codex session loads about 3.5k fewer tokens, and no rule is stated
  twice. Concretely: drop AGENTS.md:3 and the English-only duplicate (CLAUDE.md:11-13);
  cut :85-89 to one line; drop :20 (bootstrap in the precedence list), :201-204, :251-260,
  :297 (the deadline paragraph moves to the keel REFERENCE review section as one line); fold
  :173-175 into :210-230 and replace that list with a pointer to the keel skill; move :117-124
  to the `reached_tests.py` docstring; collapse :272-296 to five bullets pointing at DELIVERY.md.
  DELIVERY.md 445 to about 280: delete the History section (:401-445, its `git show c080739b`
  instruction cannot run on a public clone), strip the 32 `#NNN` and "owner order <date>"
  parentheticals, delete the retired-mechanism sentences (:3-4, :191, :265). LANES.md 353 to
  about 250: §3 stays the only build-slot text; the identity line and actor rules live there
  only. KEEL_SPEC.md: replace the three revision paragraphs with a version line; decide whether
  Law 5 (:61) still describes anything live.
- Proof: `wc -l AGENTS.md` at or under 160; a docs guard that no rule sentence appears in two
  of the three files (grep the ten duplicated sentences listed in the audit); `git diff --check`.
- Keel row: docs. Size M. Review by a blind subagent that reads only the result.

### B4. Docs purge, about 700 KB

- Delete: `docs/superpowers/` (10 files, 428 KB, DELIVERY.md:275 already declares them
  superseded), `docs/milestones/` (12 files, 308 KB, or one `HISTORY.md` under 200 lines),
  `docs/gates/`, `docs/audits/`, `docs/showcase/`, `CODEX_BOOTSTRAP_PROMPT.md` (AGENTS.md:259
  calls it historical), the 2026-08 `docs/specs/*` whose design shipped, `CHANGELOG.md` (1,120
  lines, untouched since #256, no tool reads it; `docs/releases/<version>.md` is the record),
  `apps/studio/proof/` and `apps/studio/evidence/` (7 PNGs, 740 KB, referenced nowhere).
- Move: `docs/studio-perf-2026-10-11.json` and `docs/studio-speed-2026-10-09.json` under
  `tools/*/results/`.
- Link every surviving orphan from `docs/INDEX.md` (the audit lists 21).
- Decide `MASTER_PRD.md` against `docs/product/PRODUCT_REQUIREMENTS.md` (AGENTS.md:18 ranks the
  latter higher); keep one.
- Proof: a docs guard that every `docs/**/*.md` is reachable from `docs/INDEX.md`, README or
  AGENTS.md; `git diff --check`.
- Keel row: docs. Size M.

### B5. Plugin README and evals

- Paths: `plugins/graphhelm/README.md` (352 lines; heading says 0.1.25, plugin.json is 0.1.31;
  19 "receipt" mentions to classify as wake receipt or retired receipt store);
  `plugins/graphhelm/evals/` (10 dirs with no runner and no results),
  `evals/graphhelm-development-contracts`.
- Promise: README 352 to about 200 lines with no prose that repeats LANES.md; evals either get
  a runner and committed results or the prompted twins are deleted (#184 decides which).
- Keel row: docs. Size S.

## Wave C. Algorithmic hotspots

### C1. Driver keeps an incremental projection

- Paths: `core/runtime/src/driver.rs:72-74,462-476,1036-1300`;
  `core/events/src/local.rs:2862-2874`; `core/events/src/customs.rs:193,252,276`;
  `core/events/src/sweep.rs:70`.
- Today: `record_outcome_with_evidence` and `reread_async` re-read and replay the full stream
  once per outcome and up to six times per pass; `read_replay_stream` scans every batch of
  every stream and clones each event. Per execution this is O(H²) in the event count. The
  comment at `driver.rs:1175` already names it. `customs.rs:276` replays from zero a second
  time inside one call.
- Promise: the driver holds a `ProjectionGeneration` (`core/events/src/projection.rs:1146`,
  already used by the postgres store, backup and CLI status) and applies only events after the
  last seen sequence; `LocalEventRepository` gets `read_stream_after(scope, stream, seq)`.
  O(H²) becomes O(H).
- Proof: the `load_count` test (`local.rs:1742`) shows O(1) loads per pass; `driver_contract`
  and `gate_nodes` tests stay green. Respect the pause and plan-gate ordering warned at
  `driver.rs:1295`.
- Keel row: concurrency (expanded route). Size L.

### C2. Governor apply uses indexes

- Paths: `core/governor/src/apply.rs:162-235,850-880,987-1010`.
- Today: per assignment a linear scan of `draft.operations` (:220-226, :1004); a linear scan of
  map keys instead of `contains_key` (:230-235); duplicate obligation check inside a loop
  (:875); the draft hashed twice (:162, :987).
- Promise: one `BTreeSet` of AddNode ids, `contains_key`, one obligation set, one hash.
  O(A·Ops) becomes O(A+Ops). Error variants unchanged.
- Proof: existing governor tests green; a property test with 2,000 operations finishes under
  the existing proptest budget.
- Keel row: bounded code change. Size S.

### C3. Keel prove parses the diff once

- Paths: `core/policy/src/keel_prove.rs:143,149-159,189,423,428,740`; `core/policy/src/keel.rs:534`.
- Today: the same diff is parsed three times per prove call and `new_rust_tests` scans forward
  from every `#[test]` line (O(L²) in added lines).
- Promise: parse once into `Vec<DiffFile>`, pass slices, single pass with a pending-attribute
  state.
- Proof: existing keel_prove tests green; same output on the development corpora.
- Keel row: bounded code change. Size S.

### C4. Studio: stable envelope identity and one commit per pass

- Paths: `apps/studio/src/components/panel.tsx:846-907,1347,1493`.
- Today: `useEnvelopes` returns a fresh object every render (the comment at :904 claims the
  opposite), so nine `useMemo`s in `App.tsx` (:1975, :2006, :2165, :2171, :2172, :2226,
  :2230, :2252, :2397) recompute on every 4 s poll tick and 30 s clock tick; each carrier
  triggers its own `setEnvelopes` with two Set copies (N renders, O(N²) copying); the hook runs
  three times on the same events (App, AgentPanel, RunPanel).
- Promise: memoised return keyed on `[envelopes]`, module constant for the empty case, collect
  then one `setEnvelopes`, panels receive envelopes as a prop. Also the direct fix for #1137.
- Proof: extend the #1132 regression test in `panel.test.ts` with a render-count assertion.
- Keel row: bounded code change; journeys touched named in the card. Size S.

### C5. Studio: the #1132 key applied to its three siblings, filter before sort

- Paths: `apps/studio/src/components/panel.tsx:746,785,824` (`usePersonas`,
  `useNativePersonaLinks`, `useActorAliases` still depend on `events` identity);
  `apps/studio/src/App.tsx:1790-1823` (deps list `events` and `eventList`, which is a memo of
  `events`), `App.tsx:3183` (inline `filter` with `includes` in the render path);
  `apps/studio/src/runtime/team-tasks.ts:826` (sorts every event, then filters),
  `team-tasks.ts:571-778` (`slices.filter/find` per record).
- Promise: each hook keys on the sequences of the carriers it reads; the task fold filters
  before sorting and folds only records after the last folded sequence; the cited-records
  filter is a memoised `Set`.
- Proof: Vitest cells reusing the #1132 pattern; a timing cell over a 1,250-envelope fixture
  showing the fold runs once per new record. Becomes unnecessary for the fold itself once A1
  lands; do the hook keys regardless.
- Keel row: bounded code change. Size M.

### C6. Studio: one evidence cache, one clock

- Paths: `apps/studio/src/runtime/client.ts:2110` (batches, no cache); `App.tsx:1744-1762`
  (unbounded promise map); `panel.tsx:179-190` (`OPENED_EVIDENCE`); timers at `App.tsx:611,
  2145, 2159`, `slot-poll.tsx:28`, `task-graphs.tsx:34`, `mission-view.tsx:267`,
  `proof-run.tsx:53`, `native-chats.tsx:139`, `journey-flows.tsx:47`.
- Promise: one bounded evidence cache in the client; the native-request poll joins the main
  tick; one `useNow(intervalMs)` hook replaces the four clocks.
- Proof: `client.test.ts` cell "a digest is fetched once across panel and App"; a cell counting
  live intervals with a run selected (target: one poll, one clock, one slot poll).
- Keel row: bounded code change. Size M.

## Wave D. Inputs: less to read, less to send, one schema

### D1. MCP description diet

- Paths: `apps/cli/src/commands/mcp/tools.rs:29-384` (44 tools, about 14.4 KB of descriptions),
  `:397` (`mutating_schema` repeats a 140-char `ifMatch` description on about 25 tools), the
  stub tools `present` (:300, :423), `resolve_contract` (:408), `accounting` (:480),
  `compile_context`.
- Today: `tools/list` costs about 26 to 28 KB (about 7k tokens) per connect; the four longest
  descriptions are `start` 1,013 chars, `synthesize` 722, `wake_wait` 624, `claim` 604. Stubs
  that take no arguments still ship a description.
- Promise: every description at or under 300 chars, one line plus the route; the `ifMatch`
  text hoisted to one sentence; the four stubs leave `tools/list` until they do something (they
  stay as HTTP routes for the parity guard). About 8 to 10 KB per session saved.
- Proof: a test in `tools.rs` that serialises `tools/list` and asserts total bytes under a
  named budget and no description over 300 chars; `tests/development_surface_parity.rs`
  updated for the hidden stubs; `tests/surface_completeness.rs` green.
- Keel row: wire contract (compatibility, expanded route). Size S.

### D2. One schema source for MCP and WebMCP

- Paths: `apps/cli/src/commands/mcp/tools.rs:385-1253` (hand-written schema builders);
  `apps/studio/src/webmcp/adapter.ts:142-760` (hand-copied, already drifted:
  `events.limit` is unbounded in Rust, 1..1000 on HTTP at `routes.rs:3025`, 1..200 in Studio
  at `adapter.ts:392`; Studio names `approve_node` and `amend_node_budget` where Rust has
  `approve` and `amend_budget`); `schemas/graph-signal.schema.json`.
- Promise: `schemas/mcp-tools.json` is emitted from the Rust `TOOLS` table and imported by
  the adapter; `signal.signal` references `graph-signal.schema.json` instead of
  `{"type":"object"}` (`tools.rs:669`); bounds agree on 1..1000.
- Proof: a Rust test that the emitted file equals the committed one; a Vitest cell that the
  adapter's tool names and required fields equal the file; WebMCP keeps the old names as
  aliases for one release.
- Keel row: new public surface and compatibility (full card). Size M.

### D3. Server-defaulted signal and claim fields

- Paths: `apps/cli/src/commands/mcp/tools.rs:340-351,669,845,1300`;
  `apps/cli/src/commands/serve/routes.rs:3106-3114`.
- Today: a signal needs 1 + 7 required fields (9 leaf values); `id`, `emittedAt` and `source`
  are derivable from the session; agents learn the shape only from the refusal example and the
  code comment records sessions refused twice before succeeding. The `claim` description says
  the graph is required; `graph_source` returns `Snapshot` when absent, so agents resend whole
  graphs for nothing. `clear.claimSeq` could default to the node's open claim as `waitSeq` does.
- Promise: a progress signal is `{executionId, signal:{type, severity, description, evidence}}`;
  the seal covers the server-filled values; the claim description is true; `evidence` items are
  typed.
- Proof: `tests/mcp_stdio.rs` cell "minimal signal is accepted and the stored envelope carries
  server-filled id, emittedAt, source"; a cell that the sealed digest covers them.
- Keel row: wire contract and security (expanded route, JPD flow). Size M.

### D4. Axum extractors and path confinement

- Paths: `apps/cli/src/commands/serve/routes.rs` (27 `serde_json::from_slice(&body)` blocks, no
  `Json<T>` extractor; three `after`/`limit` parsers at :715, :1179, :3006 plus ad-hoc splits at
  :1957, :5368, :6225; `percent_decode` at :753 used by only some); `routes.rs:143-153,
  3114` and the `fixtures`/`manifest` inputs read arbitrary host paths; the confinement pattern
  already exists at `routes.rs:4842-4847`.
- Promise: a `JsonBody<T>` extractor whose rejection is the four-key envelope with the same
  pointer and message; a `Page{after,limit}` extractor with per-route bounds; `file`,
  `fixtures` and `manifest` paths confined to `--project`. About 300 to 400 lines out of
  `routes.rs`.
- Proof: existing `api_http.rs` and `runtime_http.rs` error-pointer tests stay byte-identical;
  a new cell "a path outside the project is refused with the existing code".
- Keel row: security and compatibility (expanded route). Size M. Lands after B1 so the gate
  routes are not refactored and then deleted.

### D5. Hooks spawn fewer processes; keel policy parsed once

- Paths: `plugins/graphhelm/hooks/keel_stop_hook.py:26,50,57,80,112`; `lane_stop_hook.py:111,
  116,127`; `keel_pretool_hook.py`; `apps/cli/src/commands/keel.rs:306,839-865,987`;
  `extensions/builtin/graphhelm-development-contracts/schemas/keel-policy.schema.json`
  (referenced by no code).
- Today: the two Stop hooks spawn up to about ten processes per agent turn (four git calls, a
  `graphhelm keel check`, a detached waiter that spawns `wake-wait`, `tasks reconcile`, a
  re-exec with `--scan`); PreToolUse runs four or five git calls per edit while no card
  exists; `keel.yaml` is parsed twice per command and the card schema is parsed from a string
  on every validation; `keel.yaml` is never validated against its own schema.
- Promise: one Stop hook process, the scan in-process, the merge-base cached per HEAD under
  `.graphhelm/`; `LazyLock` for the policy and card schema; a test validates `keel.yaml`
  against `keel-policy.schema.json`. Ten processes become three or four; one git call per edit.
- Proof: hook tests count subprocess calls per invocation; the keel schema test.
- Keel row: bounded code change. Size S.

### D6. CLI arg structs shared

- Paths: `apps/cli/src/args.rs` (`events: PathBuf` declared 25 times with its own doc;
  `keyring`/`key_id`/`events`/`execution` repeated as Option in seven structs at :260, :291,
  :402, :420, :872, :1256, :2190; 21 `project: Option<PathBuf>`; four `flatten` uses).
- Promise: `EventsArg` and `OptionalRecordArgs` flattened in; `--help` output byte-identical.
- Proof: a golden test over `graphhelm <cmd> --help` for the touched commands, written before
  the change.
- Keel row: public CLI surface (full card). Size S.

## Wave E. One helper, not seven

| Helper | Copies | Keep | Delete |
|---|---|---|---|
| SHA-256 hex / `sha256:` wire prefix | `core/architect/src/template.rs:148`, `core/events/src/canonical.rs:60,64`, `core/schema/src/extension.rs:3132`, `core/tool-broker/src/record.rs:15`, `tools/development-benchmark/src/lib.rs:546`, `core/schema-evolution/src/canonical.rs:77-80`, CLI `journey_preview.rs:1245`, `keel_validation.rs:36`, `skills.rs:172`, inline in `routes.rs:5636,5746,5844,6081`, `journey_flow.rs:411,485` | one `graphhelm_protocols::hash::{sha256_hex, wire_sha256}` | the rest |
| Atomic write | CLI `gateway/route.rs:365`, `journey_flow.rs:938`, `schema/migrate.rs:136`, `journey_explore.rs:483`, `runtime_record.rs:124,170`, `serve/lifecycle.rs:98` | `journey_flow.rs:938` moved to a shared module | six; check fsync and temp-name semantics first |
| Hex key decode / encode | `events/config.rs:105,125`, `gateway/mod.rs:300,319`; hand-rolled encode at `secret_file.rs:231`, `mcp/mod.rs:245`, `runtime_record.rs:101` while `hex::encode` has 17 uses | one `decode_hex_key`, `hex::encode` | the rest |
| Reparse-point check | `core/events/src/local.rs:4029`, `core/extension-host/src/activation.rs:555`, `core/schema/src/extension.rs:1640`, `adapters/host-adoption/src/inventory.rs:397`, `adapters/sealed-key-provider/src/lib.rs:957`, `adapters/tool-host/src/snapshot.rs:592`, plus 14 files using `FILE_ATTRIBUTE_REPARSE_POINT` | one `graphhelm_process::fs::is_reparse_point` with identical cfg arms | the rest; security-sensitive, review each |
| Clock trait | `graphhelm_protocols::Clock` (`core/protocols/src/actor.rs:39`) and `RetentionClock` (`core/events/src/retention.rs:819`); fakes `FixedClock` ×3, `FrozenClock` ×1 | protocols `Clock` and one test fake | the rest |
| Canonical JSON | `core/protocols/src/development.rs:623` (recursive `format!` + `join`, O(n·d)), `core/schema-evolution/src/canonical.rs:67`, `core/events/src/canonical.rs:38-43` | one streaming writer in protocols | two; **high risk**: digests are persisted. Golden-digest tests over `schemas/releases/*` and the development corpora before switching. Its own ticket, last in the wave. |
| HTTP test harness | `apps/cli/tests/api_http.rs:38-1560` (self-tested) and copies of `token_path` (11), `post_json` (10), `serve` (9), `read_token` (8), `wait_for_health` (8), `get_json` (6), `credential_set` (6) across 14 test files; `repository_root` in 14, `cli()` in 12, `owner_token` in 7 | `tests/support/http.rs` | the copies; extend `http_helper_inventory.rs` to guard these names. About 1,000 to 1,400 lines. |
| Studio transport | `apps/studio/src/runtime/client.ts:540-622` (`#request`) and `:2050-2096` (`#image`) repeat token check, abort, deadline race and diagnostics | one `#transport` | about 35 lines |
| MCP dispatch | `apps/cli/src/commands/mcp/tools.rs:1524-2287`: a 760-line `match` with 45 pass-through `api.request` arms | a `route` column on `ToolSpec` | the plain GET/POST arms, about 300 lines |
| Mutation routes | `routes.rs:3216-4700`: 13 `parse_mutation_headers` + 20 `run_idempotent_mutation` blocks, about 35 lines each with 3 unique | one `mutate(...)` helper | about 200 lines |
| Included test shims | `tools/workspace-walk/walk.rs` (13 `#[allow(dead_code)]`, `include!`d by 4 tests), `tools/source-invariants/detect.rs` (included by 15) | one dev-dependency crate with real `pub` items | the 14 allows |
| Stale allows | `apps/cli/src/commands/mcp/client.rs:8` (comment says it dies with Task 5; Task 5 landed), `mcp/session.rs:21,24` | | three lines; turns the compiler back on for the module |

Each row is one ticket. Keel row: bounded code change, except canonical JSON (compatibility,
expanded route). Proof for each: byte-equality on known outputs written before the change,
then the touched crate's tests.

## Wave F. Studio dead code and test pruning

### F1. Dead code sweep

- `apps/studio/src/rail-width.ts:43-77` (`CHAT_*`, `clampChatWidth`, `loadChatWidth`,
  `saveChatWidth`; App imports only `RAIL_*`), orphan types in `runtime/types.ts:318-325,497,
  515-522` (`OpenWait`, `LiveSessionsView`, `ObserverSetupRecord`; the Studio never calls the
  observer-setup route), ten orphaned JSDoc blocks (`App.tsx:129-130,590-596`,
  `graph/model.ts:52`, `client.ts:91,386-389,1376-1380`, `team-tasks.ts:490-495,815-819`,
  `types.ts:183-187,505-509`), about 60 `export`s used only in their own file, unused CSS
  (`styles.css:1311-1319,1883,2620-2646`, `mission-view.css:50-51,147-148`,
  `delivery.css:113-114`).
- Add the reverse CSS guard to `styles.test.ts` (it checks class to rule today, not rule to
  class), with an allowlist for interpolated prefixes (`delivery-outcome-*`,
  `native-chat-request-*`, `task-node-*`, `team-bot-*`).
- Proof: `tsc --noEmit`, `vitest --maxWorkers=1` for `styles.test.ts`.
- Keel row: bounded code change. Size S, about 110 lines net.

### F2. App.tsx hooks and App.test.tsx pruning

- `App.tsx` is one component (`:185-3625`) with 89 `useState`, 32 `useEffect`, 28 `useMemo`.
  Extract, in this order, one per PR: `useExecutionPoll` (:254-282, :599-708),
  `useTeamReads` (:1764-1823), `useJourneys` (:2189-2293), `useRuntimeConnection`,
  `useProjectPrefs`, `useSay`, `useDraftStart`, `useModelsRoutes`, `useWebMcp`. Lines move,
  not vanish; the win is that C4 to C6 and A1 touch a hook, not the 3,625-line file.
- `App.test.tsx` (5,429 lines, 195 tests): delete the duplicates of component tests
  (`:1796` vs `models.test.tsx:117`; `:1781` vs `models.test.tsx:389`; `:1044` "draws no
  connection" vs `board.test.tsx:458`), the incidental-text tests (`:1013`, `:3489`), move
  `:2048` into a new `addproject.test.tsx`; rename the twelve "round-2" to "round-4" describes
  (`:2824-3999`) after the feature they test. Each deletion carries its record naming the test
  that still covers the obligation.
- Proof: `vitest --maxWorkers=1` green; the Studio checked live before merge.
- Keel row: bounded code change per hook; test-audit record per deletion. Size L, split.

## Guards that keep it clean

Each lands with the ticket that first needs it, as a source guard in the style of
`core/process/tests/no_console_window.rs`:

1. `tools/list` byte budget and 300-char description cap (D1).
2. `schemas/mcp-tools.json` equals the Rust table and the WebMCP adapter (D2).
3. No liveness literal outside the Runtime (A7).
4. Hook manifests for Claude and Codex register the same hooks; `lane_stop_hook.py` byte-identical (A6).
5. Every `docs/**/*.md` reachable from `docs/INDEX.md` (B4).
6. AGENTS.md at or under 160 lines; no rule sentence in two instruction files (B3).
7. CSS rule to class reverse check (F1).
8. `http_helper_inventory.rs` covers the shared harness names (E, HTTP harness row).
9. `keel.yaml` validates against `keel-policy.schema.json` (D5).

## Order and size

| Wave | Tickets | Size | Depends on |
|---|---|---|---|
| A | A1, A3, A4, A6 first (independent); then A2, A5; then A7, A8 | 2 L, 3 M, 4 S | — |
| B | B1, B2, B4 (independent); then B3, B5 | 3 M, 2 S | B3 after B1 so it does not cite deleted files |
| C | C1, C2, C3 (Rust, independent); C4, C5, C6 (Studio, in order) | 1 L, 3 M, 2 S | C5 fold part superseded by A1 |
| D | D1, D5, D6 (independent); D2 then D3; D4 after B1 | 3 M, 3 S | — |
| E | one PR per row; canonical JSON last | 11 S/M | HTTP harness row before any new HTTP test file |
| F | F1; then F2 hook by hook | 1 S, 1 L split | F2 after C4 to C6 |

Total estimated removal: about 30,000 PowerShell lines, about 700 KB of docs, about 5,000
Rust lines (tests included), about 500 Studio lines, and about 3.5k tokens per agent session
plus about 10 KB per MCP connect. The algorithmic changes are C1 (O(H²) to O(H) per
execution), C2, C3 and C4 to C5 (per-poll work proportional to new events, not all events).

## What this plan does not do

- It does not merge crates or move modules for their own sake. `graphhelm-simulation` (a
  9-line facade), `graphhelm-quality`, `schema-evolution`, `host-adoption` and
  `codebase-memory-mcp` are single-consumer leaf crates and stay.
- It does not touch `adapters/tool-host/tests/fixtures/context-quality/tree` (7,959 lines of
  copied source). It is a frozen corpus by design (`context_quality.rs:5,57`); exclude it from
  greps instead.
- It does not open any issue by itself. A lane opens the ticket's issue when it claims it.
