# GraphHelm documentation

Start with the [repository README](../README.md) for the current product boundary. GraphHelm is experimental: a specification describes intended behavior, while a getting-started guide or current test shows what can be tried now. Older acceptance records are historical evidence, not a release guarantee. [Source provenance](open-source/SOURCE_PROVENANCE.md) explains the one-commit public import and private issue references.

## Find your path

| I want to… | Read |
| --- | --- |
| Run something without an account or model key | [Offline quickstart](../QUICKSTART.md) |
| Install the CLI, Runtime, and Studio for a project | [Getting started](install/GETTING_STARTED.md) |
| Understand the current Studio | [Studio MVP](ux/STUDIO_MVP.md) · [Studio app README](../apps/studio/README.md) |
| Write or validate a graph | [Graph Engineer guide](graph-engineer/GRAPH_ENGINEER_GUIDE.md) · [Graph DSL](graph-engineer/GRAPH_DSL_SPEC.md) · [examples](../examples/) |
| Use GraphHelm's development method | [JPD](harness/JOURNEY_PROVEN_DEVELOPMENT.md) · [Keel](keel/KEEL_SPEC.md) · [skills guide](skills/README.md) |
| Change this repository | [Contributing](../CONTRIBUTING.md) · [delivery process](process/DELIVERY.md) |
| Report a security issue privately | [Security policy](../SECURITY.md) |

## Current implementation and operations

- [Provider-less mode](product/PROVIDER_LESS_MODE.md): the no-credential execution contract.
- [System architecture](architecture/SYSTEM_ARCHITECTURE.md) and [data and protocols](architecture/DATA_AND_PROTOCOLS.md).
- [Observability and recovery](operations/OBSERVABILITY_AND_RECOVERY.md).
- [Tools-only Runtime](operations/TOOLS_ONLY_RUNTIME.md): real tool nodes without a model credential.
- [Chat surface](ux/CHAT_SURFACE_SPEC.md): host plugin and operator skills.
- [Skills catalog and left-to-right workflow](skills/README.md).
- [Schemas](../schemas/), [conformance fixtures](../conformance/), and [execution examples](reference/EXAMPLE_EXECUTIONS.md).

## Product design and contracts

These documents contain approved requirements and planned surfaces as well as implemented ones. Check the [README's current-capability table](../README.md#what-works-today) before treating a design as shipped.

- [Master PRD](../MASTER_PRD.md), [product requirements](product/PRODUCT_REQUIREMENTS.md), and [roadmap and acceptance](product/ROADMAP_AND_ACCEPTANCE.md).
- [Decision register](DECISION_REGISTER.md), [reference stack and ADRs](reference/REFERENCE_STACK_AND_ADRS.md), and [glossary](GLOSSARY.md).
- [Dynamic harness](harness/HARNESS_SPEC.md), [Journey-Proven Development](harness/JOURNEY_PROVEN_DEVELOPMENT.md), [Keel position paper](harness/KEEL_PARADIGMS_PAPER.md), and [Keel specification](keel/KEEL_SPEC.md).
- [Graph Engineer guide](graph-engineer/GRAPH_ENGINEER_GUIDE.md) and [Graph DSL](graph-engineer/GRAPH_DSL_SPEC.md).
- [Context, knowledge, and Dreams](context/CONTEXT_KNOWLEDGE_DREAMS.md); [agents, skills, tools, and plugins](agents/AGENTS_SKILLS_PLUGINS.md); [Universal Model Gateway](models/UNIVERSAL_MODEL_GATEWAY.md).
- [Future directions](product/FUTURE_DIRECTIONS.md), [Studio vision](ux/STUDIO_SPEC.md), and [naming decision](product/NAMING_DECISION.md).
- [Threat model and isolation](security/SECURITY_ISOLATION_THREAT_MODEL.md) and [target quality/deployment architecture](operations/QUALITY_GATES_AND_DEPLOYMENT.md).

## Open source and evidence

- [Contributing](../CONTRIBUTING.md), [security reporting](../SECURITY.md), [governance and licensing](open-source/GOVERNANCE_AND_LICENSING.md), and [source provenance](open-source/SOURCE_PROVENANCE.md).
- [Benchmark protocol](keel/BENCHMARK_PROTOCOL.md) and [measured one-task pilot](keel/benchmark-evidence/task-1279-v11/README.md). Read each report's limits before comparing costs or quality.
- [Official provider and license references](reference/PROVIDER_AND_LICENSE_REFERENCES.md).
- [Current-main documentation audit dated 2026-08-31](audits/CURRENT_MAIN_DOCUMENTATION_AUDIT_2026-08-31.md), [clean-host rehearsal dated 2026-09-13](acceptance/install-rehearsal-2026-09-13.md), and [Ubuntu VPS rehearsal](../install/VPS_REHEARSAL.md) are point-in-time records. They do not validate today's head.
