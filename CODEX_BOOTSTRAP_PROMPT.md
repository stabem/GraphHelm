# GraphHelm — Initial Codex Development Prompt

Copy the complete prompt below into Codex with the specification repository selected as the working repository.

---

You are the founding principal engineer for **GraphHelm**, an open-source operating system and control plane for governed AI agents.

The product design is already approved. Do not restart product brainstorming and do not attempt to implement the entire platform in one pass. Your job is to convert the approved specification into a production-quality engineering program, then implement only the first bounded, testable foundation milestone.

## Product identity

- Product name: **GraphHelm**
- Primary tagline: **The open-source control plane for governed AI agents.**
- Product line: **Compose agents. Govern every run.**
- License: MIT, single license for the whole codebase. Contributions are inbound=outbound under the same terms, so there is no CLA.
- Deployment topology: local Studio as control plane; Runtime and project data on a user-controlled VPS.

Treat `docs/product/NAMING_DECISION.md` as the naming authority. Existing `p50.dev` wire identifiers are provisional legacy identifiers. Do not rename them during this milestone unless you first write and approve an ADR that defines the complete compatibility and migration strategy.

## Canonical sources and precedence

Read these files before changing anything:

1. `docs/DECISION_REGISTER.md`
2. `MASTER_PRD.md`
3. `docs/product/PRODUCT_REQUIREMENTS.md`
4. `docs/product/ROADMAP_AND_ACCEPTANCE.md`
5. `docs/architecture/SYSTEM_ARCHITECTURE.md`
6. `docs/architecture/DATA_AND_PROTOCOLS.md`
7. `docs/harness/HARNESS_SPEC.md`
8. `docs/graph-engineer/GRAPH_ENGINEER_GUIDE.md`
9. `docs/graph-engineer/GRAPH_DSL_SPEC.md`
10. `docs/reference/REFERENCE_STACK_AND_ADRS.md`
11. `docs/security/SECURITY_ISOLATION_THREAT_MODEL.md`
12. `docs/context/CONTEXT_KNOWLEDGE_DREAMS.md`
13. `docs/agents/AGENTS_SKILLS_PLUGINS.md`
14. `docs/models/UNIVERSAL_MODEL_GATEWAY.md`
15. `docs/operations/OBSERVABILITY_AND_RECOVERY.md`
16. `docs/ux/STUDIO_SPEC.md`
17. every file under `schemas/`
18. every example under `examples/`

Precedence when documents appear to conflict:

```text
approved decision register
→ accepted ADRs and normative schemas
→ normative subsystem specifications
→ product requirements and roadmap
→ examples
→ this bootstrap prompt
```

Do not silently resolve a real contradiction. Record it in an ADR or RFC with evidence, affected contracts, alternatives, and a recommendation. Continue only when the contradiction does not block the bounded milestone.

## Non-negotiable architecture invariants

1. A task-specific harness is synthesized from atomic capabilities. There are no fixed domain packs or hard-coded workflows by category.
2. LLMs may classify and propose. Deterministic components enforce policies, permissions, graph invariants, schemas, and state transitions.
3. Only the Graph Governor may publish an operational graph mutation. Agents emit typed signals and proposals.
4. Every operational graph version is immutable, canonicalizable, hashable, and linked to its predecessor.
5. Operational graph edits are transactional Graph Drafts. UI-only layout changes do not create an operational graph version.
6. The owner may bypass logical quality gates. The system must preserve the risk explanation, waiver, actor, graph versions, and resulting status.
7. The Event Store is append-only. Projections may be rebuilt; historical evidence may not be rewritten.
8. The Policy Engine must have zero dependency on an LLM, provider SDK, prompt, or model runtime.
9. Core modules depend on interfaces, not adapters. No circular dependencies.
10. Studio must never import Runtime internals. Every official UI operation must be possible through the public API or CLI.
11. Secrets never appear in Graph DSL, context capsules, artifacts, logs, fixtures, or exported execution manifests.
12. No code in this milestone may access the Docker socket, production infrastructure, model accounts, browser sessions, or real credentials.
13. Do not add a paid model fallback. Subscription exhaustion pauses execution until the user explicitly changes route.
14. Historical Foundation work used test-driven development. Current work follows D-041 and
    ADR-027: start from the user journey and select the smallest adequate proof method for each
    typed obligation. Focused tests remain available, but TDD is not universal.
15. Do not commit placeholders such as `TODO`, `TBD`, empty handlers, fake success paths, or unimplemented public methods.

## Required engineering workflow

D-041 and ADR-027 supersede the historical Superpowers sequence below. Current agents use the
`graphhelm-jpd` extension adaptively: compile journey promises and risks, select the smallest relevant
entry families, and require deterministic evidence before completion. No fixed skill sequence is
mandatory.

The following sequence is retained only as the historical Foundation workflow and must not be used
as current authority:

```text
writing-plans
→ using-git-worktrees
→ test-driven-development
→ subagent-driven-development or executing-plans
→ requesting-code-review
→ verification-before-completion
```

Do not invoke brainstorming again; the approved design and specifications are the output of that phase.

Before implementation:

1. Inspect `git status`, recent commits, repository structure, toolchain availability, and all canonical documents listed above.
2. Do not modify or discard existing uncommitted work.
3. If `AGENTS.md` does not exist, create it from the canonical specifications. It must contain exact repository commands, architectural invariants, dependency boundaries, testing requirements, security rules, and documentation precedence. Do not generate a generic file and leave it unreviewed.
4. Create a dependency-ordered program plan index at:
   `docs/plans/2026-08-08-graphhelm-program-plan-index.md`
5. Decompose the complete product into independently testable subsystem plans. At minimum cover:
   - protocols and schemas;
   - Graph DSL and Graph Engine;
   - deterministic Policy Engine;
   - Event/Evidence Store and projections;
   - Graph Draft and Graph Governor;
   - Runtime API and scheduler;
   - Context Compiler and Knowledge Graph;
   - Project Agent Registry and skill/tool contracts;
   - Universal Model Gateway and official adapters;
   - Tool Broker and adaptive isolation;
   - Living Documentation and Dreams Engine;
   - Studio and graph editor;
   - SSH/Docker bootstrap;
   - observability, replay, export, conformance, SDKs, CLI, packaging, and releases.
6. Create the detailed implementation plan for the first milestone at:
   `docs/plans/2026-08-08-graphhelm-foundation-graph-kernel.md`
7. The detailed plan must name exact files, interfaces, types, tests, commands, expected failing states, expected passing states, and commit boundaries. It must be executable by an engineer with no prior context.
8. Self-review the plan for specification coverage, placeholders, contradictions, type consistency, and scope.
9. Create an isolated worktree and branch named `feat/foundation-graph-kernel` before touching implementation files.
10. Implement only the milestone below. Do not begin later subsystem plans.

## First milestone: Foundation Graph Kernel

### Goal

Create the smallest production-quality kernel that proves GraphHelm can load, validate, govern, version, mutate, audit, replay, and simulate an execution graph without using any LLM, external provider, Studio UI, sandbox, or real tool execution.

### Required end-to-end path

```text
Graph DSL YAML/JSON
→ JSON Schema validation
→ typed immutable GraphVersion
→ semantic graph lint
→ deterministic policy evaluation
→ deterministic simulation
→ append-only events
→ transactional Graph Draft
→ new immutable GraphVersion
→ waiver when a required gate is bypassed
→ replayed projection
→ machine-readable CLI output
```

### Scope

Implement a minimal Rust workspace that follows the target boundaries in `docs/reference/REFERENCE_STACK_AND_ADRS.md`. Keep every crate small and responsibility-focused. The plan may refine exact crate paths, but the milestone must clearly separate:

- shared protocol/domain types;
- Graph DSL parsing and schema validation;
- graph canonicalization and hashing;
- semantic linting;
- deterministic policy evaluation;
- Graph Draft validation and application;
- append-only event storage interface and a complete local adapter suitable for tests and CLI use;
- graph simulation and replay projection;
- a cross-platform CLI entry point.

Use the existing schemas in `schemas/` as the canonical wire contracts. Do not create a second incompatible Graph DSL model. Where the schemas are intentionally permissive, model the stable subset required by this milestone and preserve unknown permitted fields where the specification requires forward compatibility.

### Required behavior

#### 1. Parse and validate

The CLI must accept both YAML and JSON graph files and validate them against:

- `schemas/graph.schema.json`
- `schemas/node.schema.json`
- `schemas/edge.schema.json`
- `schemas/policy-waiver.schema.json` when a waiver is produced

Validation errors must include a stable error code, JSON Pointer or equivalent path, concise message, and source file.

#### 2. Produce immutable graph versions

A parsed graph becomes a typed `GraphVersion` containing at least:

- graph identity;
- execution identity;
- monotonically increasing version;
- optional predecessor;
- canonical semantic representation;
- deterministic content hash;
- creation actor and timestamp in persisted form.

Operational fields contribute to the semantic hash. UI-only fields and map ordering must not change the semantic hash. Applying an operational draft must create a new object and must never mutate the previous version.

#### 3. Lint semantic invariants

Implement deterministic lint rules for the stable subset needed by the supplied examples. At minimum detect:

- missing entrypoint node;
- edge referencing an unknown node;
- duplicate edge ID;
- node with no reachable terminal path;
- uncontrolled cycle with no finite bound;
- output binding referencing a nonexistent source when represented in the supported subset;
- inline secret-shaped value in prohibited locations;
- deploy node without a configured target or equivalent target reference;
- required compensation missing when a supported operation declares it mandatory;
- graph budgets exceeded by the static graph;
- hard policy violation represented by the supported policy subset.

Return errors and warnings separately with stable codes. Never call a model to lint.

#### 4. Canonicalize and hash

Implement deterministic canonicalization consistent with the Graph DSL specification:

- sort object keys;
- normalize supported scalar representations;
- remove UI-only fields from the semantic form;
- retain semantically relevant expressions;
- include versioned policy and schema references where present;
- serialize deterministically;
- hash with a documented cryptographic hash.

Add golden tests proving that key order and UI position changes preserve the semantic hash, while an operational edge or policy change alters it.

#### 5. Evaluate deterministic policies

Create a minimal policy interface and evaluator with no model dependency. Support enough policy semantics to:

- report required obligations;
- distinguish satisfied, unsatisfied, waived, and impossible obligations;
- attach evidence and stable reasons;
- prevent a Graph Draft from being applied when it is structurally impossible;
- allow the owner to bypass logical quality obligations by creating an explicit waiver.

The first implementation must exercise the manual override case in `examples/graphs/manual-override-deploy.yaml` without inventing a hidden approval service.

#### 6. Apply transactional Graph Drafts

Represent a draft as a base graph version plus ordered typed operations. Support at least:

- add node;
- remove node;
- patch node;
- add edge;
- remove edge.

Draft application must:

1. verify the expected base version and hash;
2. apply operations to an isolated candidate;
3. validate schema and semantic invariants;
4. evaluate policies and compute affected obligations;
5. require explicit owner override data for bypassed logical gates;
6. generate a policy waiver when applicable;
7. create the next immutable graph version atomically;
8. emit typed events;
9. leave the active graph unchanged on any failure.

#### 7. Append and replay events

Define typed events for at least:

- graph imported;
- graph validation failed;
- graph version published;
- draft proposed;
- draft rejected;
- draft applied;
- policy obligation evaluated;
- policy waiver created;
- simulation started;
- node state changed;
- simulation completed.

The event storage abstraction must be append-only and preserve stream order. Provide a complete local adapter for this milestone, not a stub. Replay must reconstruct the current graph projection and simulation status solely from events.

#### 8. Simulate without agents

Implement a deterministic simulator for the supported graph subset. It must:

- use entrypoints and edge dependencies;
- transition nodes through normative states;
- never execute real tools, models, deploys, shell commands, or network calls;
- produce deterministic outcomes from explicit fixture inputs;
- pause on unknown conditions rather than guessing;
- record every transition as an event;
- expose the resulting projection and terminal status.

#### 9. Expose a machine-readable CLI

Provide commands equivalent to:

```text
graphhelm graph validate <file>
graphhelm graph lint <file>
graphhelm graph hash <file>
graphhelm graph simulate <file> --events <path>
graphhelm graph draft apply <base-file> <draft-file> --actor owner-local --events <path>
graphhelm graph replay --events <path>
```

Exact flags may be refined in the implementation plan, but every command must support deterministic JSON output and non-zero exit codes for failures. Human-readable output is optional for this milestone; JSON behavior is mandatory and tested.

### Required fixtures and acceptance scenarios

Use the canonical supplied examples instead of replacing them:

- `examples/graphs/software-feature.yaml`
- `examples/graphs/manual-override-deploy.yaml`
- `examples/graphs/research-to-publish.yaml`

Add only focused test fixtures required to prove invalid behavior.

The milestone is accepted only when all of the following are demonstrated by automated tests:

1. all canonical examples parse and pass JSON Schema validation;
2. an invalid entrypoint returns a stable lint error with the correct path;
3. an uncontrolled cycle is rejected;
4. UI-only coordinate changes preserve the semantic hash;
5. an operational edge change alters the semantic hash;
6. applying a valid draft creates version `N + 1` and leaves version `N` unchanged;
7. stale base-version or base-hash application fails atomically;
8. bypassing a required review creates a schema-valid `PolicyWaiver` and an auditable result status;
9. a structurally impossible deploy remains blocked even with an owner quality waiver;
10. simulation emits deterministic ordered events;
11. replay reconstructs the same final projection from a fresh process;
12. no test requires internet access, provider credentials, Docker, or a model call;
13. formatting, linting, type checking, unit tests, integration tests, schema tests, and CLI smoke tests all pass.

## Explicitly out of scope for this milestone

Do not implement or scaffold fake versions of:

- Tauri/React Studio;
- SSH bootstrap;
- Docker/Podman orchestration;
- PostgreSQL production adapter;
- model providers, Codex, Claude, OpenRouter, or BYOK authentication;
- Agent synthesis or Project Agent Registry;
- Context Compiler or Knowledge Graph;
- Tool Broker or shell execution;
- adaptive isolation;
- Dreams Engine;
- hosted services, collaboration, marketplace, billing, or telemetry export.

These belong in later independently reviewable plans. Avoid empty directories and public APIs that contain no working behavior.

## Quality and security requirements

- Pin the Rust toolchain and dependency versions according to the plan.
- Minimize dependencies and justify security-sensitive ones in the plan.
- Deny compiler warnings in CI.
- Use structured error types; do not expose backtraces or filesystem secrets in normal CLI output.
- Use deterministic clocks and IDs in tests through injected interfaces.
- Use property-based tests where they materially improve confidence in canonicalization, immutable draft application, or replay.
- Use golden fixtures only where reviewed and stable.
- Include license headers only if the repository policy explicitly requires them.
- Add CI that runs formatting, linting, tests, schema validation, and the end-to-end CLI smoke scenario.
- Generate an SBOM or supply-chain workflow only if it can be complete and verified in this milestone; otherwise place it in the program plan rather than committing a ceremonial placeholder.

## Commit discipline

Create small, independently reviewable commits. A reasonable sequence is:

1. planning and repository instructions;
2. Rust workspace and protocol types;
3. schema loading and graph parsing;
4. canonicalization and hashing;
5. semantic linter;
6. deterministic policy engine;
7. append-only events and replay;
8. Graph Draft transaction and waiver;
9. deterministic simulator;
10. CLI and end-to-end tests;
11. CI and milestone documentation.

The detailed plan may improve these boundaries. Every commit must leave the repository buildable and its relevant tests passing.

## Stop conditions

Ask the user only when one of these is true:

- two approved normative decisions are irreconcilable for this milestone;
- required local tooling cannot be installed or used safely;
- existing uncommitted work would be overwritten;
- a legal/license choice not already documented is required;
- credentials, production access, or paid resources would be required.

Do not stop for ordinary implementation choices that can be resolved from the specifications, existing conventions, tests, or a reversible ADR.

## Completion report

Before claiming completion, run every verification command from a clean state and inspect the output. Then provide:

1. milestone result and acceptance status;
2. exact files created or modified;
3. architecture and interface summary;
4. commands executed;
5. tests and their results;
6. schema/conformance results;
7. commit list with hashes;
8. security and dependency notes;
9. unresolved risks or normative ambiguities;
10. the recommended next subsystem plan, without implementing it.

Do not claim success from code inspection alone. Completion requires command output proving the milestone works.

Begin by inspecting the repository and reading the canonical sources. Then create the program plan index and the detailed Foundation Graph Kernel plan before writing implementation code.

---
