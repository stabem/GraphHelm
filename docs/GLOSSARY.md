# Glossary

**Agent Definition** — persistent, versioned configuration of an agent.

**Agent Experience** — auditable memories and performance metrics linked to an agent.

**Agent Runtime** — ephemeral instance of an Agent Definition or agent synthesized within a node.

**Artifact** — persistent, content-addressed output, such as a patch, report, image, or build.

**Autopilot** — mode in which the harness executes and adapts the graph autonomously according to policies.

**Capability** — atomic unit describing something the system can do.

**Capability Lease** — temporary, scoped, and revocable authorization to use a capability.

**Completion Contract** — conditions and evidence required to consider a node or execution complete.

**Context Capsule** — minimal, versioned, context-specific package delivered to a node.

**Context Compiler** — component that retrieves, filters, compresses, and compiles Context Capsules.

**Dreams Engine** — asynchronous cognitive maintenance of documents, claims, memories, agents, skills, and indexes.

**Evidence** — referenceable proof that supports a claim or completion requirement.

**Event Store** — append-only log of events and operational evidence.

**Execution** — instance of work initiated by a user, event, schedule, or Dreams.

**Ghost Node** — node proposed visually, but not yet approved or executed.

**Graph Architect** — component that proposes the graph's topology and composition.

**Graph Draft** — transactional set of operational changes not yet applied.

**Graph Engineer** — person who creates capabilities, tools, skills, policies, evaluators, adapters, and contracts.

**Graph Governor** — component authorized to transform signals into new Graph Versions.

**Graph Signal** — structured finding emitted by an agent, tool, test, runtime, user, or Dreams.

**Graph Version** — immutable, executable snapshot of the graph's topology and configuration.

**Harness** — compiled setup for a task, including graph, agents, models, context, policies, budgets, and isolation.

**Hard Constraint** — technical or policy rule that is explicitly non-waivable.

**Knowledge Graph** — representation of the project's entities, claims, relations, temporality, and provenance.

**Living Documentation** — human-readable documents, versioned and materialized from claims/evidence.

**Manual Graph** — mode in which the user builds and modifies the workflow; the harness acts as a linter and assistant.

**Model Route** — usable connection to a model or runtime, with provider, auth, capabilities, and capacity state.

**Node** — unit of execution with a goal, contract, permissions, and lifecycle.

**Overlay** — temporary change applied to a node instance without altering its persistent definition.

**Policy Engine** — deterministic engine that enforces rules and invariants.

**Project Agent Registry** — catalog of persistent agents within a project's scope.

**Provenance** — origin and derivation chain of a piece of information, artifact, or decision.

**Rule Document** — atomic Living Documentation unit: one business rule per file with a stable id, referenced explicitly by node contracts and updated post-execution as a candidate through the claim lifecycle.

**Runtime** — daemon and services running on the user's VPS.

**Shadow Workspace** — isolated snapshot used to test Dreams changes before commit.

**Skill** — versioned operational guidance for applying capabilities.

**Studio** — local application that serves as the control plane and visual editor.

**Supervised** — mode in which relevant expansions await confirmation.

**Tool** — executable mechanism that provides capabilities.

**Tool Broker** — mediator of tool calls, permissions, secrets, and sandboxes.

**Waiver** — explicit record of an unmet obligation, by decision of an authorized user.
