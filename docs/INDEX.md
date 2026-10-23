# GraphHelm documentation

Start with the [repository README](../README.md) for the current product boundary. GraphHelm is experimental: a specification describes intended behavior, while a getting-started guide or current test shows what can be tried now. Older acceptance records are historical evidence, not a release guarantee. [Source provenance](open-source/SOURCE_PROVENANCE.md) explains the one-commit public import and private issue references.

## Find your path

| I want to… | Read |
| --- | --- |
| Run something without an account or model key | [Offline quickstart](../QUICKSTART.md) |
| Install the CLI, Runtime, and Studio for a project | [Getting started](install/GETTING_STARTED.md) |
| Understand the current Studio | [Studio app README](../apps/studio/README.md) (Graph, Lanes, Proof) · [Studio MVP](ux/STUDIO_MVP.md) (first slice, historical) |
| Write or validate a graph | [Graph Engineer guide](graph-engineer/GRAPH_ENGINEER_GUIDE.md) · [Graph DSL](graph-engineer/GRAPH_DSL_SPEC.md) · [examples](../examples/) |
| Use GraphHelm's development method | [JPD guide](jpd/README.md) · [JPD design](harness/JOURNEY_PROVEN_DEVELOPMENT.md) · [Keel](../KEEL.md) · [Keel rationale](keel/KEEL_SPEC.md) · [Keel check](keel/KEEL_CHECK.md) · [Append effect contract](keel/EFFECT_CONTRACT.md) · [skills guide](skills/README.md) |
| Change this repository | [Contributing](../CONTRIBUTING.md) · [delivery process](process/DELIVERY.md) · [optimization plan, Oct 2026](process/OPTIMIZATION_PLAN_2026-10.md) |
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

- [Product requirements](product/PRODUCT_REQUIREMENTS.md) and [roadmap and acceptance](product/ROADMAP_AND_ACCEPTANCE.md); [MVP adoption candidates](product/MVP_ADOPTION_CANDIDATES.md) and the [spec-debt queue](product/SPEC_DEBT_TRIGGERS.md).
- [Decision register](DECISION_REGISTER.md), [reference stack and ADRs](reference/REFERENCE_STACK_AND_ADRS.md), and [glossary](GLOSSARY.md).
- [Dynamic harness](harness/HARNESS_SPEC.md), [Journey-Proven Development](harness/JOURNEY_PROVEN_DEVELOPMENT.md), [Keel position paper](harness/KEEL_PARADIGMS_PAPER.md), and [Keel specification](keel/KEEL_SPEC.md).
- [Graph Engineer guide](graph-engineer/GRAPH_ENGINEER_GUIDE.md) and [Graph DSL](graph-engineer/GRAPH_DSL_SPEC.md).
- [Context, knowledge, and Dreams](context/CONTEXT_KNOWLEDGE_DREAMS.md); [agents, skills, tools, and plugins](agents/AGENTS_SKILLS_PLUGINS.md); [Universal Model Gateway](models/UNIVERSAL_MODEL_GATEWAY.md).
- [Future directions](product/FUTURE_DIRECTIONS.md), [Studio vision](ux/STUDIO_SPEC.md), and [naming decision](product/NAMING_DECISION.md).
- [Threat model and isolation](security/SECURITY_ISOLATION_THREAT_MODEL.md) and [verification and deployment boundaries](operations/QUALITY_GATES_AND_DEPLOYMENT.md).

## Open source and evidence

- [Contributing](../CONTRIBUTING.md), [support](../.github/SUPPORT.md), [community conduct](../.github/CODE_OF_CONDUCT.md), [security reporting](../SECURITY.md), [governance and licensing](open-source/GOVERNANCE_AND_LICENSING.md), and [source provenance](open-source/SOURCE_PROVENANCE.md).
- [Benchmark protocol](keel/BENCHMARK_PROTOCOL.md) and [measured one-task pilot](keel/benchmark-evidence/task-1279-v11/README.md). Read each report's limits before comparing costs or quality.
- [Official provider and license references](reference/PROVIDER_AND_LICENSE_REFERENCES.md).
- [Clean-host rehearsal dated 2026-09-13](acceptance/install-rehearsal-2026-09-13.md) and [Ubuntu VPS rehearsal](../install/VPS_REHEARSAL.md) are point-in-time records. They do not validate today's head.

## Subsystem specifications, RFCs and releases

Specifications are normative (AGENTS.md ranks them third, after the decision register and accepted
ADRs); a dated filename records when the design was approved, not that it is out of date.

- Specifications: [GraphHelm design](specs/2026-08-08-graphhelm-design.md), [protocols and schema evolution](specs/2026-08-08-protocols-schema-evolution-design.md), [production event/evidence store](specs/2026-08-09-production-event-evidence-store-design.md), [safe persistence projection](specs/2026-08-09-safe-persistence-projection-design.md), [graph engine and governor](specs/2026-08-13-graph-engine-governor-design.md), [Runtime](specs/2026-08-13-runtime-design.md), [native token-efficient development contracts](specs/2026-08-22-native-token-efficient-development-contracts-design.md), [customs acting surface](specs/2026-09-11-customs-acting-surface-design.md), [Graph Architect](specs/2026-09-11-graph-architect-design.md), [architect judgments](specs/2026-09-16-architect-judgments-design.md), [methodology adoption](specs/2026-09-21-graphhelm-methodology-adoption-design.md), [Studio live team and proven journeys](specs/2026-10-05-studio-live-team-and-proven-journeys-design.md), [journey explore](specs/2026-10-06-journey-explore-design.md), [journey-first Keel](specs/2026-10-07-journey-first-keel-design.md).
- RFCs: [RFC-1951 coding scope and placeholder retirement](rfcs/RFC-1951-CODING-SCOPE-AND-PLACEHOLDER-RETIREMENT.md), [RFC-0036 governed pending graph](rfcs/RFC-0036-GOVERNED-PENDING-GRAPH.md), [RFC-0071 durable memory](rfcs/RFC-0071-DURABLE-MEMORY.md), [RFC-0072 Dreams shadow journey](rfcs/RFC-0072-DREAMS-SHADOW-JOURNEY.md).
- Releases: [0.1.0](releases/0.1.0.md), [0.1.1](releases/0.1.1.md), [unreleased](releases/UNRELEASED.md), [Windows binary](releases/WINDOWS_BINARY.md).
- Harness and Studio subsystems: [Graph Architect, first compile](harness/GRAPH_ARCHITECT.md), [native development contracts](harness/NATIVE_DEVELOPMENT_CONTRACTS.md), [native handoff observation](harness/NATIVE_HANDOFF_OBSERVATION.md), [context reuse on Windows](harness/CONTEXT_REUSE_WINDOWS_PROOF.md), [native chat bridge](studio/NATIVE_CHAT_BRIDGE.md), [node deliveries](studio/NODE_DELIVERIES.md), [trajectory view](ux/TRAJECTORY_VIEW.md).
- Operations and security: [MCP behind a reverse proxy](operations/MCP_REVERSE_PROXY.md), [VPS upgrade, backup and restore](operations/VPS_UPGRADE_BACKUP_RESTORE.md), [event/evidence store threat model](security/EVENT_EVIDENCE_STORE_THREAT_MODEL.md).
- Guides and journeys: [user journeys](guides/journeys.md), [beacon reload proof (#959)](journeys/issue-959-beacon-reload-proof.md), [lane Stop reconciles a merged task](journeys/lane-stop-merged-task.md), [JPD plugin plan](jpd/plans/2026-08-22-graphhelm-jpd-plugin.md).

## Acceptance records

Point-in-time evidence of what ran on a dated head; none validates today's head.

- [M05 acceptance map](acceptance/M05_ACCEPTANCE_MAP.md) and its runs: [M05 2026-08-16](acceptance/m05-run-2026-08-16/README.md), [M06 2026-08-17](acceptance/m06-run-2026-08-17/README.md) with its [fixture journey demo](acceptance/demos/m06-fixture-journey/README.md), [M07 2026-08-17](acceptance/m07-run-2026-08-17/README.md).
- M08 and M09 judging: [run 2026-08-18](acceptance/m08-run-2026-08-18/README.md), rejudges [1](acceptance/m08-rejudge-2026-08-18/README.md), [5](acceptance/m08-rejudge5-2026-08-18/README.md), [6](acceptance/m08-rejudge6-2026-08-18/README.md), [7](acceptance/m08-rejudge7-2026-08-18/README.md), [8](acceptance/m08-rejudge8-2026-08-18/README.md), [9](acceptance/m08-rejudge9-2026-08-18/README.md), [judge coverage](acceptance/m08-judge-coverage.md), [M09 judge run](acceptance/m09-judge-run-2026-08-19/README.md).
- M11 and the MVP: [first compile](acceptance/m11-first-compile-2026-09-11.md), [acting half](acceptance/m11-acting-2026-09-11.md), [architect judgments recipe](acceptance/architect-judgments-recipe.md), [integrated MVP verification](acceptance/mvp-integrated-2026-09-14.md), [optional GPT-6 Astra route](acceptance/gpt-6-astra-migration.md).

## Removed history (#1248)

These were deleted as shipped history; read any of them at the last commit that had it with
`git show 02df519c:<path>`: `docs/superpowers/` (implementation plans), `docs/milestones/` (milestone
narratives), `docs/audits/`, `docs/showcase/`, `CODEX_BOOTSTRAP_PROMPT.md`, the root `CHANGELOG.md`
(`docs/releases/` is the record; `schemas/CHANGELOG.md` is a different file and stays),
`MASTER_PRD.md` (its normative form is [product requirements](product/PRODUCT_REQUIREMENTS.md)),
and the Studio proof images under `apps/studio/proof/` and `apps/studio/evidence/`.
