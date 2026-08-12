# Approved decision register

This document consolidates the product decisions already made. They are normative for version 0.1 of the specification.

| ID | Theme | Decision |
|---|---|---|
| D-001 | Topology | Local Studio as control plane; runtime and data on the user's VPS. |
| D-002 | VPS connection | Existing VPS connected via SSH; installation and updates via Docker. |
| D-003 | Autonomy | Full autonomy governed by the harness; no mandatory approval between phases. |
| D-004 | Scope | Generalist agent operating system, not just a coding tool. |
| D-005 | Harness | Harness synthesized per task from atomic capabilities; no fixed pack per domain. |
| D-006 | Mandatory rules | Deterministic Policy Engine enforces invariants and gates according to risk signals. |
| D-007 | Agents | Ephemeral agents are synthesized with contracts and registered capabilities. |
| D-008 | Agent persistence | Useful agents are saved to the Project Agent Registry and can be reused. |
| D-009 | Agent memory | Limited, auditable memory, with evidence, confidence, validity, and expiration. |
| D-010 | Project truth | Three layers: Evidence/Event Store, Project Knowledge Graph, and Living Documentation. |
| D-011 | Dreams Engine | Full autonomy governed over knowledge, documents, agents, skills, and indexes. |
| D-012 | Dreams and code | Dreams does not alter code directly; it creates a normal, substantiated task for the Graph Engine. |
| D-013 | Isolation | Adaptive Tier 0–3 isolation, elevated as risks are discovered. |
| D-014 | Models | Universal Model Gateway with BYOK, direct APIs, aggregators, native runtimes, and local models. |
| D-015 | Subscriptions | Official connection with ChatGPT/Codex and Claude/Claude Code accounts when supported by the provider. |
| D-016 | Subscription fallback | On reaching a subscription limit, pause; do not spend BYOK/OpenRouter automatically. |
| D-017 | Context | Layered Context Compiler with node-specific capsules and justified expansion. |
| D-018 | Graph at runtime | Adaptive, versioned graph, changed only by the Graph Governor. |
| D-019 | Sovereignty | The user can pause, remove gates, skip phases, and connect directly to deploy. |
| D-020 | Agent replacement | A replacement agent never starts automatically after manual intervention; it requires confirmation. |
| D-021 | Ghost nodes | Proposed expansions appear as transparent nodes that consume no tokens before approval. |
| D-022 | Modes | Autopilot, Supervised, and Manual Graph, switchable during execution. |
| D-023 | Node editor | Full editor: objective, prompt, model, context, skills, tools, contracts, gates, and retries. |
| D-024 | Editing a saved agent | Changes to a node apply only to the current execution; promotion requires an explicit action. |
| D-025 | Editing at runtime | Visual changes are immediate; operational changes go through a transactional Graph Draft. |
| D-026 | Chat | Contextual Command Router; operational mutations become drafts for confirmation. |
| D-027 | Hierarchy | Workspace → Project → Subproject, with selective inheritance of context and policies. |
| D-028 | Collaboration | Single-user first, but identity, authorization, and auditing ready for teams. |
| D-029 | Open source | Framework, Runtime, and Studio fully open source; no essential function closed. |
| D-030 | Licensing | Dual license: community AGPLv3 and an alternate commercial license. |
| D-031 | Contributions | Non-exclusive CLA, with ICLA and CCLA, allowing commercial relicensing. |
| D-032 | First slice | Complete developer-first vertical slice, without limiting the generalist architecture. |
| D-033 | Current state | Produce complete documentation before any implementation. |
| D-034 | Name | GraphHelm is the product name selected for development, subject to legal clearance and namespace reservation before public launch. |

## Mandatory consequences

- No component may assume a fixed workflow called "software pack", "marketing pack", or equivalent.
- An agent never receives the project's entire history by default.
- An executor cannot approve its own result alone when independent review is required.
- The owning user can override or bypass gates, but the system records risks, waivers, and downstream results.
- Model credentials must not be accessible in the same sandbox that runs untrusted code.
- The official desktop app cannot use private endpoints unavailable to external clients.
- Every Studio action must be possible via the Runtime's public API/CLI.
- The Event Store cannot be rewritten by the Dreams Engine.
