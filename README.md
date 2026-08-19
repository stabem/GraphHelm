# GraphHelm — full product documentation

> **Repository:** https://github.com/stabem/GraphHelm
> **Licence:** MIT — see [LICENSE](LICENSE)
> **Start here:** [QUICKSTART.md](QUICKSTART.md) — a real run, offline, no account, no daemon
>
> **Product name:** GraphHelm
> **Original codename:** Programação 5.0
> **Category:** open source operating system for AI agents
> **Specification version:** 0.1.1

### What actually runs today

This section is the one a newcomer needs, and it is written last on purpose: it stated
`2026-08-08` for eleven days while the binary moved three milestones ahead of it. A newcomer
given only the old text concluded the project was still a validator and would have closed the
tab — measured, in a probe run against this README.

**Runs now, verifiable with the clone and no accounts:**

* Graph authoring: validate, lint, semantic hashing, immutable versions, drafts, waivers.
* Nine JSON Schemas with a verifiable catalog, an immutable `1.0.0` snapshot, and conformance
  fixtures.
* An append-only event store (local JSONL and PostgreSQL) with byte-identical replay,
  cryptographic erasure, and verified restore.
* **Execution**: start a graph, drive it, pause, resume, approve, cancel — offline via
  fixtures, or against a real model gateway and a brokered tool sandbox.
* **The one-glance answer**: `attention` says `needs_you`, `can_sleep` or `unknown`, with the
  reason and the node named, and time on the surface.
* **A read-only monitor**, an HTTP API, and an **MCP server** exposing the same operations to a
  chat client.
* **A quality gate** that refuses to certify itself against a suite of deliberately useless
  deliverables, and a **blind judge** — an evaluator that cannot see the code and only probes
  the shipped surface.

**Does not exist yet, said plainly because the specification below describes it in detail:**

* **Studio** — the visual application. Specified, not built. No command opens it.
* **Context Compiler** and **Dreams Engine** — specified, not built.
* Anything describing a VPS daemon deployment story.

**Known limitation you would otherwise hit:** `execution status` exits `0` whatever the answer,
so a script must read the `attention` field rather than the exit code.

GraphHelm is a local-first platform in which the user controls, through a visual interface, an agentic infrastructure running on their own VPS. Each request is classified, decomposed, and converted into an execution graph specific to the scenario. The system selects or creates agents, models, tools, context, isolation, tests, and reviews without relying on fixed domain-specific workflows.

## Foundation Graph Kernel

The first executable milestone is written in Rust 1.97.1. It provides offline YAML/JSON validation, semantic hashing, immutable versions, deterministic lint and policy, transactional drafts, waivers, side-effect-free simulation, an append-only Event Store, replay, and a JSON CLI.

```bash
cargo run --locked -p graphhelm-cli -- graph validate examples/graphs/software-feature.yaml
cargo run --locked -p graphhelm-cli -- graph lint examples/graphs/software-feature.yaml
cargo run --locked -p graphhelm-cli -- graph hash examples/graphs/software-feature.yaml
```

See [docs/milestones/foundation-graph-kernel.md](docs/milestones/foundation-graph-kernel.md) for contracts, commands, exit codes, security, and acceptance evidence.

## Protocols and schema evolution

The nine JSON Schemas have a verifiable catalog, canonical hashes, an immutable `1.0.0` snapshot, conservative compatibility analysis, exact SemVer rules, declarative and scoped migrations, public conformance fixtures, and a JSON CLI. The initial release does not publish any data migration; the provisional `p50.dev` identifiers have been preserved.

```bash
cargo run --locked -p graphhelm-cli -- schema catalog --catalog schemas/catalog.json
cargo run --locked -p graphhelm-cli -- schema check --baseline schemas/releases/1.0.0/catalog.json --candidate schemas/catalog.json
cargo run --locked -p graphhelm-cli -- schema conformance --catalog schemas/catalog.json --fixtures conformance/manifest.json
cargo run --locked -p graphhelm-cli -- schema view --catalog schemas/catalog.json --schema graph
```

See [docs/milestones/protocols-and-schema-evolution.md](docs/milestones/protocols-and-schema-evolution.md) for the catalog contract, limits, compatibility matrix, SemVer, migrations, security, rollback, and acceptance evidence.

## Production Event and Evidence Store

The Governor externalizes every free-form authoring value into encrypted Evidence and publishes a `PersistedGraphVersion` that keeps only safe topology and ordered content references inline. The local JSONL repository and the PostgreSQL adapter share one wire contract with forced row-level security, authenticated stream heads and checkpoints, legal holds, auditable cryptographic erasure, disposable projection generations, and encrypted backup with verified restore. Replay never requires plaintext.

```bash
cargo run --locked -p graphhelm-cli -- events verify --repository PATH
cargo run --locked -p graphhelm-cli -- events rebuild --config OPERATOR_CONFIG --workspace WORKSPACE --project PROJECT --stream STREAM
cargo run --locked -p graphhelm-cli -- events backup --config OPERATOR_CONFIG --output ARCHIVE
cargo run --locked -p graphhelm-cli -- events restore --config OPERATOR_CONFIG --archive ARCHIVE
```

See [docs/milestones/production-event-evidence-store.md](docs/milestones/production-event-evidence-store.md) for trust boundaries, repository formats, hash semantics, keys, retention, limits, diagnostics, rollback, and acceptance evidence.

The product is made up of three open surfaces:

1. **Framework** — Graph Engine, Harness Compiler, Context Compiler, Policy Engine, Agent Registry, Model Gateway, Dreams Engine, and protocols.
2. **Runtime** — daemon installed on the user's VPS, responsible for execution, sandboxes, events, artifacts, credentials, and jobs.
3. **Studio** — local application for chat, graph, running agents, documents, files, auditing, and sovereign workflow control.

## How to read this repository

- [MASTER_PRD.md](MASTER_PRD.md): consolidated, normative document.
- [docs/DECISION_REGISTER.md](docs/DECISION_REGISTER.md): all choices approved during the definition phase.
- [docs/product/PRODUCT_REQUIREMENTS.md](docs/product/PRODUCT_REQUIREMENTS.md): functional and non-functional requirements.
- [docs/ux/STUDIO_SPEC.md](docs/ux/STUDIO_SPEC.md): screens, components, states, and interactions.
- [docs/architecture/SYSTEM_ARCHITECTURE.md](docs/architecture/SYSTEM_ARCHITECTURE.md): high-level architecture and topology.
- [docs/harness/HARNESS_SPEC.md](docs/harness/HARNESS_SPEC.md): full specification of the dynamic harness.
- [docs/graph-engineer/GRAPH_ENGINEER_GUIDE.md](docs/graph-engineer/GRAPH_ENGINEER_GUIDE.md): guide for building capabilities, agents, gates, and extensions.
- [docs/graph-engineer/GRAPH_DSL_SPEC.md](docs/graph-engineer/GRAPH_DSL_SPEC.md): typed graph DSL.
- [docs/context/CONTEXT_KNOWLEDGE_DREAMS.md](docs/context/CONTEXT_KNOWLEDGE_DREAMS.md): low-consumption context, knowledge, and the Dreams Engine.
- [docs/agents/AGENTS_SKILLS_PLUGINS.md](docs/agents/AGENTS_SKILLS_PLUGINS.md): lifecycle of agents, skills, tools, and plugins.
- [docs/models/UNIVERSAL_MODEL_GATEWAY.md](docs/models/UNIVERSAL_MODEL_GATEWAY.md): BYOK, subscriptions, local routes, and capability policy.
- [docs/security/SECURITY_ISOLATION_THREAT_MODEL.md](docs/security/SECURITY_ISOLATION_THREAT_MODEL.md): isolation, secrets, and threat model.
- [docs/architecture/DATA_AND_PROTOCOLS.md](docs/architecture/DATA_AND_PROTOCOLS.md): entities, events, APIs, and contracts.
- [docs/operations/OBSERVABILITY_AND_RECOVERY.md](docs/operations/OBSERVABILITY_AND_RECOVERY.md): metrics, checkpoints, replay, and recovery.
- [docs/open-source/GOVERNANCE_AND_LICENSING.md](docs/open-source/GOVERNANCE_AND_LICENSING.md): MIT licence, contribution terms, and governance.
- [docs/product/ROADMAP_AND_ACCEPTANCE.md](docs/product/ROADMAP_AND_ACCEPTANCE.md): phases, acceptance criteria, and metrics.
- [docs/product/NAMING_DECISION.md](docs/product/NAMING_DECISION.md): name, positioning, and brand architecture.
- [CODEX_BOOTSTRAP_PROMPT.md](CODEX_BOOTSTRAP_PROMPT.md): initial prompt for planning and first implementation in Codex.
- [docs/reference/EXAMPLE_EXECUTIONS.md](docs/reference/EXAMPLE_EXECUTIONS.md): complete graph examples.
- [docs/reference/REFERENCE_STACK_AND_ADRS.md](docs/reference/REFERENCE_STACK_AND_ADRS.md): reference stack and architectural decisions.
- [docs/reference/PROVIDER_AND_LICENSE_REFERENCES.md](docs/reference/PROVIDER_AND_LICENSE_REFERENCES.md): verified official sources.
- [schemas/](schemas/): JSON Schema contracts.
- [examples/](examples/): example graphs and manifests.

## Constitutional principles

1. **No essential function depends on a proprietary server.**
2. **The harness proposes and governs; the user remains sovereign.**
3. **Each task receives a graph specific to it, not a fixed pack.**
4. **Every important claim requires provenance and evidence.**
5. **Agents share artifacts and compiled context, not entire chats.**
6. **Quality is proven by gates and evidence, not by the executor's self-confidence.**
7. **Permissions, context, and secrets follow the principle of least privilege.**
8. **Every operational mutation of the graph is versioned, transactional, and reversible.**
9. **The system must seek the smallest graph capable of producing sufficient evidence.**
10. **The core and the protocols are public, documented, and replaceable.**

## Scope of this package

This repository describes the entire product, including the generalist vision. The recommended first slice remains developer-first, but the architecture does not depend on the programming domain. Research, product, marketing, data, documents, design, automation, operations, and other work all use the same catalog of atomic capabilities and the same graph compiler.

## Legal status

The licence is MIT: anyone may use, modify and redistribute the code, including inside a closed commercial product. An earlier strategy proposed AGPLv3 with a commercial dual licence; it was reversed, and `docs/open-source/GOVERNANCE_AND_LICENSING.md` records that rather than hiding it.
