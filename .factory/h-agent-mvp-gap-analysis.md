# MVP gap analysis — H Agent, 2026-08-19

**Base: `origin/main` @ `0fb0e66`** ("fix(execution): a queued node waits for its edges…",
PR #103). Every file citation below is against this base (`git show origin/main:…`);
issue states read live from `gh` at analysis time. Read-only pass; no cargo, no board pen.

**Yardstick — an interpretation, marked as such:** `MASTER_PRD.md` (712 lines) contains
NO explicit MVP section (grep for "MVP": zero hits; structure is full-vision, §1–§25).
The operative MVP definition is therefore **Phase 1 — developer-first vertical slice**
of `docs/product/ROADMAP_AND_ACCEPTANCE.md` (§3, lines 23–96): 22 scope items (§3.2) and
a 17-step acceptance scenario (§3.4). If the owner means a different MVP, this table
re-keys but the evidence rows survive.

## 1. Gap table — Phase 1 scope (§3.2), 22 items

| # | Scope item | Status | Evidence / pointer |
|---|---|---|---|
| 1 | local Studio | **ABSENT** | No Studio code; workspace members (Cargo.toml) = core/*, adapters/*, apps/cli, tools/* only. Would live in a future `apps/studio`. |
| 2 | SSH/Docker bootstrap | **ABSENT** | No bootstrap tooling in repo. Would live beside a Studio/installer. |
| 3 | single-node Runtime | **PARTIAL** | `core/runtime` + `core/execution` + `graphhelm serve` (HTTP API, `apps/cli/src/commands/serve/`) shipped across M05 (runtime.md; monitor #51 `ee3f128`, doorbell #53 `e438d2d`) and hardened through M08/M09 (`5ec1614`, `0f4e7fe`). Missing: a deployment/install story on a VPS (ties to item 2). |
| 4 | Workspace/Project/Subproject | **PARTIAL** | Workspace/project types in `core/protocols/src/persistence.rs`; project routing fixed for MCP in PR #91 `1b55a13` (issue #82 CLOSED); workspace-dir recovery PR #84 `f2efb94`. Subproject: not found — MARKED UNVERIFIED (no targeted search). |
| 5 | Codex/Claude native adapters | **PARTIAL→ABSENT** | `adapters/model-gateway/src/byok.rs` references anthropic/claude (32 grep hits) — that is the BYOK route. NATIVE (subscription) Codex/Claude adapters as distinct from BYOK: no evidence found. |
| 6 | BYOK/OpenRouter adapter | **SHIPPED** (adapter layer) | `adapters/model-gateway/src/{byok.rs,broker.rs,transport.rs}`; openrouter/openai references (45 hits byok.rs). End-to-end use in an execution: MARKED UNVERIFIED — no test cited here proves a real model call drives a node. |
| 7 | Graph DSL subset (core nodes) | **SHIPPED** | `core/schema` + `core/schema-evolution` (milestone protocols-and-schema-evolution.md); CLI `validate`/`lint`/`hash`; schema digest #85 `326799b`. |
| 8 | Graph Engine | **SHIPPED** | `core/graph` (canonicalization, hashing, versions) + milestone graph-engine-governor.md; foundation M01 (foundation-graph-kernel.md). |
| 9 | Graph Draft | **SHIPPED** | `core/governor` (transactional drafts) + CLI `draft.rs`; same milestone doc. |
| 10 | Autopilot/Supervised/Manual | **PARTIAL** | Graph-level `mode` exists but its surfaces over-promise dispatch control (issue #89 OPEN); dispatch-time user override has NO consumer (#93 OPEN); Waived/Skipped states table-legal but unproducible (#94 OPEN — D-019 sovereignty lever cannot be pulled). |
| 11 | Project Agent Registry | **ABSENT** | Actors exist as event attribution (`PersistedActor`, core/protocols); no registry of agents as a capability catalog. |
| 12 | Context Compiler | **ABSENT** | "Capsule" appears only in schema/graph TESTS (schema types); no compiler crate, no capsule assembly at execution time. |
| 13 | Event Store | **SHIPPED** | `core/events` + `adapters/postgres-event-store`; milestone production-event-evidence-store.md; backup/restore/verify/rebuild CLI (`commands/events/`); evidence-opens check #77 `df5e431`. |
| 14 | initial Knowledge Graph | **ABSENT** | No code hits; no crate. |
| 15 | Living Docs | **ABSENT** | Docs exist as files; no living-docs engine. |
| 16 | Tier 0/1 | **SHIPPED** | `adapters/tool-host/src/host.rs` implements Tier0/Tier1 (Tier2 referenced too); `core/tool-broker` leases (Capability/ToolLease used by serve). |
| 17 | Tool Broker (repo/shell/tests) | **SHIPPED** | `core/tool-broker` + `apps/cli/src/commands/tool/` + tool-host adapter; M05 runtime.md. |
| 18 | Policy Engine | **SHIPPED** | `core/policy` (M01, foundation-graph-kernel.md); deterministic, no LLM deps (AGENTS.md invariant). |
| 19 | quality gates | **SHIPPED** | `core/quality` + M06 #57 `01bd946` + ci/gate.ps1 (exit-code fix #97 CLOSED via `dff6e9d`; suite-list discovery #98 CLOSED; docs #102 MERGED `07b1243`). Known debt: #104 OPEN (acceptance-map red on main), #81 OPEN (restore-path timeouts flake the gate), #19 OPEN (timing flakes under load). |
| 20 | basic Dreams | **ABSENT** | Zero grep hits for "dreams" in core/ and apps/. |
| 21 | export/replay | **PARTIAL** | Replay: CLI `replay.rs` + rebuildable projections (store invariant) SHIPPED. Export-without-credentials as a product surface (PRD §4.10): `events backup` exists; a credential-free export manifest is MARKED UNVERIFIED. |
| 22 | public API/CLI | **SHIPPED** | `graphhelm` CLI (17 command families incl. execution lifecycle start/pause/resume/approve/signal/cancel/amend/status), serve HTTP API, MCP surface (`commands/mcp/`). |

**Tally: 9 SHIPPED, 5 PARTIAL, 8 ABSENT** (of 22).

## 2. Acceptance scenario (§3.4) — 17 steps, condensed honestly

- Steps 1–4 (install VPS, connect Studio, auth model route, import repo): **ABSENT** — no
  installer, no Studio; auth exists only as gateway config, not a flow.
- Steps 5–7 (request feature → Graph v1 → maps/plans/changes/tests/reviews): **PARTIAL** —
  executions run graphs with fixtures and drive nodes (`0fb0e66` itself fixes edge-gating);
  a HARNESS that synthesizes the graph from a prompt (PRD §10, Graph Architect) is ABSENT.
  Real model-driven code edit end-to-end: UNVERIFIED, no cited test.
- Steps 8–11 (remove review, force deploy, Draft shows risks, waiver recorded): **PARTIAL** —
  drafts + waivers exist (core/governor); forcing/dispatch override is exactly #93/#94/#89
  (OPEN); deploy capability ABSENT.
- Steps 12–13 (quota pause/resume): **PARTIAL** — pause/hold semantics heavily built (issues
  #80/#83 CLOSED via `0fb0e66`/`6193f5c`; #90/#92 OPEN for start-held and approve-without-
  resume); pause ON QUOTA specifically via gateway eligibility: MARKED UNVERIFIED.
- Step 14 (docs and claims updated): **ABSENT** (Living Docs).
- Step 15 (export reproduces timeline): **PARTIAL** (replay yes; export surface unverified).
- Steps 16–17 (context-efficiency figures, ReuseDecision events, reuse across work):
  **ABSENT** — no ReuseDecision in code (no grep hits expected; not searched — MARKED),
  no context compiler to reuse.

## 3. M09 close doc "What remains for M10" (arming-the-alarm.md:292) — status

| Item | Status at 0fb0e66 |
|---|---|
| Owner's founding numbers (48%/15→36ms/3x) in-repo reproduction | No landing found — no issue; **gap with no issue** (unless #87 subsumes it: #87 is the D1 fix, not the reproduction). |
| Storm mechanism + disk-vs-load attribution (fresh paired baseline) | Advanced OUTSIDE main: today's fleet measurements (h-agent-base-measurements.md, storm lane co-sign) produced the paired 53d212d/ef51193 same-disk result + tail bound — in `.factory/`, not in a merged doc. No issue tracks landing it. |
| Flake #3 oracle upgrade (per-round receipt-sequence identity) | No issue found in open set — **gap with no issue**, MARKED (title-only scan). |
| #81 timeout policy + mis-burn-to-attention wiring | #81 OPEN (orchestrator says fix imminent). Mis-burn surfacing: #95 OPEN is attention-adjacent but edge-gating, not mis-burn — **mis-burn wiring has no issue**, MARKED. |
| Nine seeds in m09-seeds.md | Not re-audited here (file exists at base); MARKED unread. |

## 4. Open issues mapped (gh, live)

**Issues that ARE Phase-1 gaps:** #89, #93, #94 (item 10 — sovereignty/modes); #90, #92,
#96 (execution-lifecycle completeness around item 22/steps 12–13); #95 (attention as part
of the operator surface, item 3).

**Issues that are NOT MVP gaps (hardening/debt beyond scope):** #104, #81, #19 (gate
health); #101 (single-authority duplication); #88 (wake-wait answer staleness); #87 (M10
perf design); #35 (spec-debt queue); #44 (ideation log).

**Phase-1 gaps with NO open issue — the finding (RESOLVED same day: all 16 posted as
tracking issues #105–#120 from `.factory/h-agent-gap-issue-stubs.md`, 2026-08-19):**
1. local Studio (item 1) → **#105**
2. SSH/Docker bootstrap (item 2) → **#106**
3. Dynamic harness / Graph Architect (PRD §10; acceptance steps 5–6) → **#107**
4. Context Compiler + capsules at runtime (item 12; steps 16–17) → **#108**
5. Codex/Claude NATIVE adapters (item 5, distinct from shipped BYOK) → **#109**
6. Project Agent Registry (item 11) → **#110**
7. initial Knowledge Graph (item 14) → **#111**
8. Living Docs (item 15; step 14) → **#112**
9. basic Dreams (item 20) → **#113**
10. Deploy capability + forced-deploy flow (steps 8–10) → **#114**
11. ReuseDecision/context-efficiency observability (steps 16–17) → **#115**
12. Export-without-credentials surface (item 21, the un-verified half — may CLOSE on
    investigation) → **#116**
13. From §3, the four M09 future-work orphans → **#117** (founding-numbers
    reproduction), **#118** (flake-#3 oracle upgrade), **#119** (mis-burn surfacing),
    **#120** (storm-attribution archival — rides the M10 close's evidence commit).

## 5. The defensible claim

**MVP (Phase 1) is INCOMPLETE.** What exists at `0fb0e66` is the governed-execution
SUBSTRATE — graph kernel/DSL/hashing, drafts+governor, deterministic policy, append-only
event store with replay/backup, single-process runtime with a full execution lifecycle
over HTTP/CLI/MCP, tool broker with Tier 0/1 hosts, quality gates, and a BYOK gateway
adapter: 9 of 22 scope items SHIPPED, 5 PARTIAL. What does not exist is the entire
user-facing and intelligence layer the PRD's main flow runs through: no Studio, no
bootstrap, no dynamic harness (nothing synthesizes a graph from a prompt), no context
compiler, no knowledge graph/living docs/dreams, no agent registry, no deploy path, no
reuse metrics — 8 of 22 ABSENT. At analysis time 13 named gaps carried no tracking
issue; same day they were posted as **#105–#120** (see §4), making the missing half
durable as issues. The pre-existing open set (15 issues) is dominated by hardening of
the shipped substrate, not by the absent layer; closing every pre-existing issue would
NOT have completed the MVP.

Sharper form for decision-making: **the repo has completed roughly the "engine room" half
of Phase 1 and has not started the half a user would see.** The acceptance scenario
(§3.4) cannot currently be executed past its graph-execution middle (steps 5–13,
partially) — it fails at both ends (install/Studio, docs/reuse).

*Cite-or-mark: every SHIPPED row names a crate/file/SHA/PR; UNVERIFIED marks mean I did
not run or grep the specific proof, not that evidence is known absent; ABSENT rows are
grep/tree-supported negatives at the named base, best-effort not exhaustive.*
