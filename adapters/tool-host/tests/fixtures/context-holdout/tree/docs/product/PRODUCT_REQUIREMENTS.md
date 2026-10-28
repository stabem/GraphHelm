# Product Requirements

## 1. Normative scope

This document translates the vision of the MASTER PRD into verifiable requirements. Identifiers must be preserved in issues, acceptance tests, and RFCs.

## 2. Functional requirements

### FR-001 — Self-hosted installation

The user must be able to install the Runtime on an existing Linux VPS via SSH and Docker, without creating an account on a central service.

**Acceptance:** the bootstrap validates architecture, disk, memory, Docker/Podman, ports, Git, and persistence; shows a plan before altering the VPS; allows updating and uninstalling.

### FR-002 — Local Studio

The Studio must operate as a local control plane, reconnecting to the Runtime without interrupting executions.

### FR-003 — Context hierarchy

There must be Workspace, Project, and Subproject, with selective inheritance for policies, agents, skills, documents, sources, and credentials.

### FR-004 — Universal intake

The user must be able to submit natural language, files, images, allowed links, node selections, and document references.

### FR-005 — Command Router

Each message must produce `intent`, `target`, `confidence`, `operational_effect`, and `interpretation`. Graph mutations create a draft, not a silent change.

### FR-006 — Task Profile

The system must classify, at minimum:

- probable domain and multi-domain;
- complexity;
- depth;
- affected surface;
- regression risk;
- security risk;
- reversibility;
- uncertainty;
- research need;
- data sensitivity;
- estimated cost and duration;
- need for human decision;
- minimum isolation tier.

### FR-007 — Capability catalog

Capabilities must be searchable by objective, input/output, permissions, cost, history, runtime, isolation, and compatibility.

### FR-008 — Per-task harness

Each execution must have an immutable, versioned Harness Manifest. There must be no mandatory dependency on fixed packs per domain.

### FR-009 — Typed Graph DSL

Nodes and edges must declare contracts. The linter must block technical incompatibilities before execution.

### FR-010 — Synthesized agents

The system must create specific agents when no saved agent satisfies the need. Every definition requires an objective, capabilities, permissions, schemas, evidence requirements, and completion criteria.

### FR-011 — Project Agent Registry

Agents must be savable, versionable, evaluable, derivable, suspendable, archivable, and reusable within the project scope.

### FR-012 — Temporary node changes

Editing an agent instance on the graph does not alter the persistent definition. Saving/promoting requires an explicit command.

### FR-013 — Universal Model Gateway

Must support aggregators, BYOK, direct APIs, official native runtimes, and local models through replaceable adapters.

### FR-014 — Pause on subscription limit

When a subscription route reaches its limit or loses session, dependent nodes enter `waiting_for_model_capacity`; there must be no automatic paid fallback.

### FR-015 — Context Capsules

Each node receives a versioned capsule, with included items, excluded items, provenance, token budget, and expansion policy.

### FR-016 — Expansion request

The agent must be able to request more context by declaring the reason, the missing information, and the expected impact. The Context Compiler accepts, reduces, or rejects it.

### FR-017 — Event Store

Prompts, decisions, events, diffs, tests, models, costs, artifacts, waivers, and mutations must be recorded in an append-only manner.

### FR-018 — Knowledge Graph

Claims must have status, confidence, temporal validity, provenance, and relations `supports`, `contradicts`, `supersedes`, `derived_from`, and `applies_to`.

### FR-019 — Living Documentation

Readable documents must be versioned and linked to claims/evidence. Updates must produce a diff and justification.

### FR-020 — Graph Engine

The engine must execute DAGs and governed graphs with forks, joins, conditions, retries, timeouts, compensations, checkpoints, and human decisions.

### FR-021 — Graph Governor

Agents cannot alter the topology directly. They must emit a `graph_signal`. The Governor publishes a new version after policy, dependency, cost, and lint checks.

### FR-022 — Ghost nodes

When the interaction policy requires approval, an expansion appears as a visual proposal without runtime, context, or model consumption.

### FR-023 — Operating modes

There must be Autopilot, Supervised, and Manual Graph modes, switchable during execution.

### FR-024 — Sovereign control

The owner can pause, cancel, remove a node, change model, edit an edge, skip a gate, and go straight to deploy when technically feasible.

### FR-025 — Waiver

Skipping an obligation creates a waiver with actor, scope, risks, graph version, and duration. The system must not automatically reinstate the ignored gate.

### FR-026 — Confirmed replacement

After the user deactivates an agent, any proposed replacement remains stopped until explicit confirmation.

### FR-027 — Transactional Graph Draft

Operational changes must be grouped, analyzed, and applied atomically. Only affected branches are paused/invalidated.

### FR-028 — Full node editor

The user can edit objective, instructions, agent, model, skills, tools, context, schemas, completion criteria, isolation, resources, retries, gates, memory policy, and edge conditions.

### FR-029 — Adaptive isolation

Each node receives a tier, filesystem, network, secret scope, and resource limits. The runtime can elevate the tier based on signals.

### FR-030 — Capability leases

Access to a tool, network, filesystem, secret, or production must have a scoped, revocable, and recorded lease.

### FR-031 — Tool Broker

Agents must access the environment through mediated, typed tools. Untrusted code does not receive model credentials.

### FR-032 — Quality gates

The system must support deterministic tests, model-based evaluation, independent review, security, performance, consistency, documentation, source, and custom criteria.

### FR-033 — Completion evidence

Each node and execution must declare what proves completion. Output without evidence may be marked as partial, not as fully validated.

### FR-034 — Bias control

The harness must be able to enforce independent reviewer, blind review, provider diversity, prompt diversity, and disagreement resolution.

### FR-035 — Dreams Engine

Must operate in a Shadow Workspace, validate changes, receive independent critique, and perform an atomic commit or discard.

### FR-036 — Dreams without direct code changes

Findings that require code must become normal tasks with origin `dream_generated`.

### FR-037 — Parallel documentation and agent

The graph can update claims, indexes, and documentation while another branch executes, as long as dependencies and snapshots avoid reading inconsistent state.

### FR-038 — Agent panel

The Studio must show status, role, model, node, tokens/capacity, duration, tools, context, and the last event for each agent.

### FR-039 — Docs and files panel

Documents, files, images, artifacts, diffs, tests, and sources must be linked to the project and to the nodes that produced or consumed them.

### FR-040 — Audit and replay

The user must be able to replay the timeline, compare Graph Versions, open inputs/outputs, and export the Execution Manifest without secrets.

### FR-041 — Public APIs

Everything the Studio does must be available via API and, when applicable, CLI and SDK.

### FR-042 — Extensions

Plugins must declare type, capabilities, contracts, permissions, minimum isolation, platforms, and version compatibility.

### FR-043 — Local observability

Tokens, quotas, costs, latency, failures, graph mutations, context usage, and quality scores must be available locally.

### FR-044 — Future collaboration

Even in single-user mode, every action must have an `actor`. The authorization model must accept user, service account, and agent identity.

### FR-045 — Export

Project, agents, skills, graphs, policies, and execution manifests must be exportable. Secrets never go in by default.

## 3. Non-functional requirements

### NFR-001 — Privacy

No data is sent to the maintainer by default. External telemetry is opt-in, documented, and can be disabled.

### NFR-002 — Secret security

No secret in logs, artifacts, context capsules, exported prompts, or crash dumps. The secret scanner must run on persistent outputs.

### NFR-003 — Resilience

The Runtime survives the Studio closing. A VPS reboot recovers executions from durable checkpoints.

### NFR-004 — Idempotency

Mutation commands, events, and retries must have idempotent IDs.

### NFR-005 — Compatibility

Schemas and APIs follow SemVer. Breaking changes require migration and a changelog.

### NFR-006 — Studio performance

The canvas must maintain fluid interaction with 1,000 nodes and virtualize details. State updates must be incremental.

### NFR-007 — Context efficiency

The Context Compiler must measure precision, recall, redundancy, and tokens saved per capsule.

### NFR-008 — Bounded expansion

Every execution has limits on nodes, depth, mutations, retries, cost, and wall-clock time; loops without progress are detected.

### NFR-009 — Auditability

Every relevant model decision must record candidates, score, constraints, and the chosen route, while respecting provider confidentiality.

### NFR-010 — Accessibility

Keyboard navigation, visible focus, contrast, labels, text alternatives, and reduced-motion mode.

### NFR-011 — Portability

The reference Runtime supports Linux x86_64 and arm64. The Studio supports Windows, macOS, and Linux.

### NFR-012 — Replaceability

Stores, model adapters, sandbox adapters, evaluators, and retrievers must have public interfaces.

## 4. Business rules

- A gate can be `required`, `recommended`, or `optional`.
- `required` can be waivable by the owner, except in cases of technical impossibility or a strict policy configured by the owner themselves.
- A waiver does not convert nonexistent evidence into satisfied evidence.
- A `succeeded` node can be invalidated by a later mutation if its semantic input changed.
- An expired memory does not automatically enter a Context Capsule.
- A suspended agent is not selected by the matcher, but remains reproducible by version.
- The Graph Governor preserves completed outputs only while the dependency hash remains valid.
- A subscription model route cannot spend BYOK API without manual selection.
- Dreams never erases the Event Store nor expands its own permissions.
- A plugin does not receive network or secrets without explicit declaration and compatible policy.
