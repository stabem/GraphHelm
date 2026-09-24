# Milestone 05 - Runtime: design

Status: implemented in full — Milestone 05 complete (05a–05f; acceptance map gate-verified). Derived by aggregating the existing normative documentation plus two new owner
decisions recorded as D-039 and D-040; every requirement cites its source.

## 1. What this milestone is

Milestone 04 proved governance and durability of execution with an effect-free executor: a published
graph runs, pauses, crashes, recovers, resumes, completes and replays byte-identically — but no node
ever does real work. Milestone 05 replaces the fixture behind the `NodeExecutor` seam with real
work: model calls through the Universal Model Gateway, tool calls through a Tool Broker, inside
Tier 0/1 isolation — behind the same contracts, so everything Milestone 04 proved keeps holding.

It also gives GraphHelm its first three *surfaces*: the Public Runtime API (the single official
entry point the architecture already mandates), a chat surface — GraphHelm as an MCP server
operable from inside Claude Code and Codex (D-039) — and a local read-only monitor page, the first
Studio slice (D-040).

## 2. Sources

| Requirement area | Source |
|---|---|
| MVP scope: single-node Runtime, local Studio, Codex/Claude native adapters, BYOK, Tool Broker for repository/shell/tests, Tier 0/1, public API/CLI | `docs/product/ROADMAP_AND_ACCEPTANCE.md` §3.2 |
| Public Runtime API surface and rules (idempotency keys, expected versions, local identity) | `docs/architecture/SYSTEM_ARCHITECTURE.md` §3.1 |
| Every Studio action must be possible via the public API/CLI | `docs/DECISION_REGISTER.md` (hard constraints) |
| Route types incl. Codex CLI/SDK and Claude Code/SDK official flows; route manifest; credential broker; usage normalization; exhausted-capacity policy | `docs/models/UNIVERSAL_MODEL_GATEWAY.md` §2, §4, §7, §11, §12 |
| Tools as MCP/local process/container/HTTP; plugins never in-process | `docs/agents/AGENTS_SKILLS_PLUGINS.md` §11-12, `docs/graph-engineer/GRAPH_ENGINEER_GUIDE.md` |
| Isolation tiers 0/1; credentials never in the sandbox that runs untrusted code | `docs/security/SECURITY_ISOLATION_THREAT_MODEL.md`, register hard constraints |
| Execution/feedback, signals, governor | `docs/harness/HARNESS_SPEC.md` §18-20 |
| Pause immediate-stop, cancel compensation, retry categories | `docs/operations/OBSERVABILITY_AND_RECOVERY.md` §12-14 |
| Studio as the local control plane; degraded mode reads the last local snapshot | `docs/ux/STUDIO_SPEC.md` §1, §"degraded" |
| Chat-first operation inside coding agents; monitor before full Studio | D-039, D-040 (new, this milestone) |

## 3. New decisions, recorded

**D-039 — Chat-first operation via an official MCP server.** GraphHelm ships an MCP server as a
first-class surface, so a coding agent chat — Claude Code and Codex both speak MCP — can start,
observe, signal, approve, pause, resume and cancel executions from inside the conversation. It is
an adapter over the Public Runtime API and never a second path: anything the MCP surface can do,
the API and CLI can do identically (this is already a register hard constraint for Studio; D-039
extends the same rule to the chat surface). Distribution inside the agents follows each host's
native packaging (a Claude Code plugin/skill wrapping the MCP server; Codex's MCP configuration),
but the contract is the MCP server, not the wrapper.

**D-040 — The monitor precedes Studio.** The first UI is a local, read-only monitor page served by
the Runtime itself: executions, node states, the triage view, events tail. It is a Studio *slice*,
not a fork — it reads only the Public Runtime API, so full Studio later replaces it without a
migration. Full Studio (chat, canvas editing, marketplace, the organizing web app) stays out of
Milestone 05 and remains its own milestone, optional in the owner's words.

## 4. What already exists

Do not rebuild these. Milestone 05 composes them.

- The complete Milestone 04 stack: pure contracts, durable projection, scheduler, governance
  decisions, lifecycle, and the CLI driver with `drive_to_quiescence` — the driver's loop is the
  reference implementation the real runtime generalizes.
- `NodeExecutor` — the seam, with `FixtureExecutor` as its only implementor.
- The sealed key provider and evidence encryption (M03) — the credential broker's storage and the
  signal-envelope externalization both build on it.
- The append-only store, projections, replay, integrity checkpoints (M03).
- 25 event kinds, the operator CLI, the acceptance map.

## 5. Boundary

**In scope:** the Public Runtime API (HTTP, local identity, idempotency keys and expected versions
on every mutation); a Gateway slice — route manifest, BYOK Anthropic/OpenAI API routes, Claude
Code and Codex subscription-runtime routes via their official CLIs, credential broker over the
sealed provider, usage normalization minimal; a Tool Broker slice — repository, shell, tests — in
Tier 0/1 isolation; the real `NodeExecutor` — async execution, prompt assembly from the node
contract, outcomes carrying artifacts and evidence, encrypted signal-envelope externalization,
immediate-stop; the MCP server surface (D-039); the read-only monitor (D-040); and the Milestone
04 deferred ledger items that need a runtime: the resume spec cross-check, dispatch fairness, and
per-edge-type readiness once edge conditions can be evaluated.

**Out of scope:** full Studio and the organizing web app (own milestone, per D-040); multi-node
scheduling; Tier 2/3; hosted cloud; marketplace; Knowledge Graph and Dreams beyond event emission;
the Harness Compiler's intake/profiling/synthesis pipeline (graphs still arrive authored);
enterprise SSO; compensation for external effects beyond recording them.

## 6. Decisions to resolve before planning, resolved

**6.1 The executor seam goes async without breaking the pure crate.** `NodeExecutor::execute` is
synchronous and `core/execution` must stay clock-free and I/O-free. Resolution: the pure crate
keeps the synchronous seam (simulation and tests keep working unchanged); the runtime defines its
own `AsyncNodeExecutor` in a new `core/runtime` crate and the driver moves there, generalized from
`apps/cli`'s loop. The CLI keeps its local synchronous path; the API server drives through the
async one. The decision rule stands: every `next_state` still comes from `apply_transition`, and
the driver still never invents an outcome.

**6.2 Real outcomes carry more than a `NodeOutcome`.** A model call produces content; content is
free-form; free-form content never enters an event (D-036). Resolution: the executor returns
`NodeOutcome` plus evidence/artifact references, the content externalized through the existing
sealed path before the outcome event is appended — the same evidence-before-append rule the
`signal` command already enforces, now for node work.

**6.3 Credentials live behind the broker, never in the workspace.** The register's hard
constraint. API keys load from the OS keychain or the sealed provider's storage; the Claude/Codex
subscription routes hold no key at all — they invoke the official CLIs, which own their own auth.
The Tier 1 workspace where tools run never sees either.

**6.4 The MCP server is stateless over the API.** It holds no store handle and no driver; every
tool call maps to one Runtime API request, so chat, CLI, monitor and future Studio can never
disagree about state. Notifications ride the API's event stream.

**6.5 The monitor is read-only by construction.** It is served from the Runtime binary, renders
the same JSON the `status` command produces, and contains no mutating call — approval and pause
stay in chat and CLI until full Studio, because a UI that can mutate is Studio, and Studio has a
spec this slice must not fork.

## 7. Plan decomposition

Each plan produces working, testable software on its own, in dependency order.

- **05a — Public Runtime API.** HTTP server over the existing command layer: execution lifecycle,
  status, events tail; local identity; idempotency keys and expected versions per §3.1. Pins the
  HTTP stack as a deliberate ADR (none is pinned today).
- **05b — Gateway slice.** Route manifest, BYOK Anthropic/OpenAI adapters, Claude Code and Codex
  CLI runtime adapters, credential broker over the sealed provider, exhausted-capacity mapping to
  `NeedsCapacity`.
- **05c — Tool Broker and Tier 0/1.** Repository/shell/tests tools as brokered local processes;
  Tier 0 for cognitive nodes, Tier 1 workspaces for execution nodes; credentials structurally
  outside the workspace.
- **05d — The real executor.** `core/runtime` with `AsyncNodeExecutor`, prompt assembly, outcomes
  with externalized evidence, encrypted signal envelopes, immediate-stop, the M04 ledger items
  (resume cross-check, dispatch fairness, edge conditions).
- **05e — The chat surface (D-039).** The GraphHelm MCP server over the API; packaging for Claude
  Code and Codex; the operator story driven from a chat, end to end.
- **05f — The monitor (D-040), gate and closure.** The read-only local page, the milestone
  acceptance run per §3.4's applicable steps, the acceptance map, the final review.

## 8. Acceptance

- A published graph with agent and tool nodes runs to completion doing real work — a model call
  and a repository/shell/test cycle — with every transition durable and the full history replaying
  byte-identically, exactly as the effect-free milestone proved.
- The same execution can be started, observed, signalled, approved, paused, resumed and cancelled
  from: the CLI, the HTTP API, and a Claude Code or Codex chat via the MCP server — with identical
  observable state at every point.
- The monitor shows a running execution's states, triage list and events without offering a single
  mutation.
- Credentials are demonstrably absent from every Tier 1 workspace.
- An exhausted subscription route parks the node in `NeedsCapacity` — wait, not aggressive retry —
  per the gateway spec.
- The full local gate is green, both PostgreSQL locale passes included.
- No surface recalculates the attention verdict. The CLI, the HTTP API, the MCP server and
  the monitor all derive it from the same predicate, so they cannot disagree about whether
  the operator needs to wake up. (Added 2026-08-17 by owner decision, after M07 produced the
  property and M07's own acceptance map correctly REFUSED to anchor it: a clause added at the
  end of a milestone by its author is a promise nobody made. The order was promise first,
  bindings second, count last — never a number pushed until an assert agrees.)

## 9. Risks

- **Scope gravity toward Studio.** The monitor slice will invite "just one button". D-040 exists
  to refuse it: any mutation in a UI is Studio's, and Studio is not this milestone.
- **Host-agent API drift.** Claude Code and Codex evolve fast; the MCP server must track the
  protocol, not either host's extensions, or the chat surface forks per host.
- **The async seam leaking into the pure crates.** `core/execution` stays sync and pure; the
  moment `core/runtime` types appear in a pure crate's signature, the 04 property suite stops
  meaning anything. The purity invariants must extend to the new crate boundary.
- **Real nondeterminism.** Model output is not replay-stable; what replays is the *record* —
  outcomes, evidence digests, decisions — never the generation. The acceptance criterion is
  worded on the history, not the content, deliberately.
