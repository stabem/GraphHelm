# Approved decision register

This document consolidates the product decisions already made. They are normative for version 0.1 of the specification.

| ID | Theme | Decision |
|---|---|---|
| D-001 | Topology | Local Studio as control plane; runtime and data on the user's VPS. |
| D-002 | VPS connection | Existing VPS connected via SSH; installation and updates via Docker. |
| D-003 | Autonomy | Total autonomy governed by the harness; no mandatory approval between phases. |
| D-004 | Scope | Generalist agent operating system, not just a coding tool. |
| D-005 | Harness | Harness synthesized per task from atomic capabilities; no fixed domain-specific pack. |
| D-006 | Mandatory rules | Deterministic Policy Engine enforces invariants and gates according to risk signals. |
| D-007 | Agents | Ephemeral agents are synthesized with contracts and registered capabilities. |
| D-008 | Agent persistence | Useful agents are saved to the Project Agent Registry and can be reused. |
| D-009 | Agent memory | Limited, auditable memory, with evidence, confidence, validity, and expiration. |
| D-010 | Project truth | Three layers: Evidence/Event Store, Project Knowledge Graph, and Living Documentation. |
| D-011 | Dreams Engine | Total governed autonomy over knowledge, documents, agents, skills, and indices. |
| D-012 | Dreams and code | Dreams does not alter code directly; it creates a normal, substantiated task for the Graph Engine. |
| D-013 | Isolation | Adaptive isolation Tier 0–3, escalated according to discovered risks. |
| D-014 | Models | Universal Model Gateway with BYOK, direct APIs, aggregators, native runtimes, and local models. |
| D-015 | Subscriptions | Official connection to ChatGPT/Codex and Claude/Claude Code accounts when supported by the provider. |
| D-016 | Subscription fallback | When the subscription limit is reached, pause; do not automatically spend via BYOK/OpenRouter. |
| D-017 | Context | Layered Context Compiler with node-specific capsules and justified expansion. |
| D-018 | Runtime graph | Adaptive, versioned graph, altered only by the Graph Governor. |
| D-019 | Sovereignty | The user can pause, remove gates, skip phases, and connect directly to deploy. |
| D-020 | Agent replacement | A replacement agent never starts automatically after manual intervention; it requires confirmation. |
| D-021 | Ghost nodes | Proposed expansions appear as transparent nodes without consuming tokens before approval. |
| D-022 | Modes | Autopilot, Supervised, and Manual Graph, switchable during execution. |
| D-023 | Node editor | Full editor: objective, prompt, model, context, skills, tools, contracts, gates, and retries. |
| D-024 | Editing a saved agent | Node changes apply only to the current execution; promotion requires an explicit action. |
| D-025 | Editing during execution | Visual changes apply immediately; operational changes enter a transactional Graph Draft. |
| D-026 | Chat | Contextual Command Router; operational mutations become drafts pending confirmation. |
| D-027 | Hierarchy | Workspace → Project → Subproject, with selective inheritance of context and policies. |
| D-028 | Collaboration | Single-user first, but identity, authorization, and audit ready for teams. |
| D-029 | Open source | Framework, Runtime, and Studio fully open source; no essential function closed. |
| D-030 | Licensing | MIT, single license. Supersedes the earlier AGPLv3-plus-commercial plan: that design existed to hold a commercial lever, and the owner chose adoption over the lever. |
| D-031 | Contributions | No CLA. Inbound equals outbound under MIT, so there is nothing left for a contributor agreement to grant. Supersedes the ICLA/CCLA plan, which existed only to enable commercial relicensing. |
| D-032 | First slice | Complete developer-first vertical slice, without limiting the generalist architecture. |
| D-033 | Current state | Produce full documentation before any implementation. |
| D-034 | Name | GraphHelm is the product name selected for development, subject to legal clearance and namespace reservation before public launch. |
| D-035 | Event and evidence persistence | The replay-safe Event Journal is append-only; sensitive evidence is encrypted and kept separate, may undergo auditable erasure under explicit authority/policy, and never makes canonical replay dependent on plaintext. |
| D-036 | Safe persistence projection | The authoring graph and the `PersistedGraphVersion` are distinct representations: the Governor deterministically externalizes free-form content as encrypted Evidence, keeps only safe topology inline, and computes semantic identity from the ordered content digests. Required content that is deleted or unavailable blocks execution. JSONL and PostgreSQL immediately adopt the new format, with no legacy compatibility layer whatsoever (`no legacy compatibility layer`): pre-release internal compatibility, legacy importers, and the intermediate `1.1.0` release are removed, and the public `1.0.0` baseline is rebuilt. Persisted diagnostics contain no filesystem/source path or dynamic prose; the only permitted locator is a domain-scoped JSON Pointer limited to registered GraphHelm contract roots. The limited persistence waiver replaces the previous internal draft. |
| D-037 | Authoring paths in the safe projection | The Foundation contract allows paths in context source scopes, permission scopes, and isolation writable lists. The persisted projection never copies this plaintext: each position is mandatory, `restricted`, ordered Evidence, semantically hashed under one of the closed types `ContextPath`, `PermissionPath`, or `IsolationPath`. The previous eight-position enum was incomplete and is replaced by eleven positions. Since the product has not yet been published, the sole public `1.0.0` baseline is corrected in place, with no alias, translation, intermediate release, or legacy branch. |
| D-038 | Hand-rolled HMAC and base64 instead of the planned pinned crates | The plan fixed `hmac = 0.13.0` and `base64 = 0.23.1` as exact-pinned dependencies. Neither is declared; HMAC-SHA-256 and base64url are implemented in-tree. This deviation is accepted and recorded rather than reversed. Both implementations are small, closed, and now pinned to the standards' own vectors, and swapping the HMAC late would change the byte output that authenticated receipts, checkpoints and the revocation journal already depend on. The in-tree HMAC additionally keeps its inner and outer pads, derived subkeys and intermediate digests in `Zeroizing`, which a crate boundary would not guarantee. Adopting the pinned crates remains an open option and must be a deliberate, separately verified change, not a silent substitution in either direction. |
| D-039 | Chat-first operation via an official MCP server | GraphHelm ships an MCP server as a first-class surface, so a coding agent chat — Claude Code and Codex both speak MCP — can start, observe, signal, approve, pause, resume, and cancel executions from inside the conversation. It is an adapter over the Public Runtime API and never a second operational path: every Runtime read, mutation, and privileged capability used by chat is available identically through the API and CLI. Schema-bound local advisory artifacts may be authored without creating Runtime authority. Host-specific packaging (Claude Code plugin/skill, Codex MCP configuration) wraps the one server; the contract is the MCP server, not the wrapper. |
| D-040 | The monitor precedes Studio | The first UI is a local, read-only monitor page served by the Runtime itself — executions, node states, triage view, events tail — reading only the Public Runtime API so full Studio later replaces it without migration. It contains no mutating action: a UI that can mutate is Studio, and Studio (chat, canvas, the organizing web app) remains its own later milestone, optional. |
| D-041 | Journey-Proven Development and one extension model | Development assurance starts from the complete user journey: promises become typed observation obligations, and the harness selects the smallest proof methods adequate to their risk. Unit, property, integration, concurrency, and browser tests remain available but no one ritual is universal. Missing observation capability is a typed `OBSERVER_MISSING` refusal; later successful retries preserve the initial failure and lineage. A `flaky_pass` is never JPD-proven; it remains unresolved or requires a blocker-local owner waiver. Skills, observers, evaluators, agents, schemas, graphs, and host adapters are contributions of the existing versioned `Extension` model, never a second plugin format. JPD orchestration skills and host wrappers call GraphHelm only through public CLI, MCP, or HTTP contracts; executable contributions use their declared Extension and Tool Broker protocols and never import core internals. |
| D-042 | Containment-safe code-index provider boundary | GraphHelm may decode bounded, recorded `structuredContent` from a code-index provider without treating that decoder as a live MCP connection. Live provider retrieval requires a broker-managed MCP SDK session over an immutable, digest-pinned index snapshot copied into Tier 1, with a verified executable and `CBM_CACHE_DIR` confined to that sandbox. Index construction and retrieval are separate operations. Until those contracts exist, live retrieval fails closed as unavailable; GraphHelm never exposes the host provider cache, invokes the provider directly, auto-indexes, or invents invocation or executable identity absent from `ToolCallRecord`. |

## Mandatory consequences

- No component may assume a static workflow called "software pack," "marketing pack," or equivalent.
- An agent never receives, by default, the full project history.
- An executor cannot approve its own result alone when independent review is required.
- The owning user can override or ignore gates, but the system records risks, waivers, and subsequent outcomes.
- Model credentials cannot remain accessible in the same sandbox that runs untrusted code.
- The official desktop app cannot use private endpoints unavailable to external clients.
- Every Studio action must be possible via the Runtime's public API/CLI.
- The Event Store cannot be rewritten by the Dreams Engine.
- Agent agreement is advisory; deterministic policy and evidence decide gates.
- A successful retry cannot erase or relabel its earlier failed attempt as first-pass success.
