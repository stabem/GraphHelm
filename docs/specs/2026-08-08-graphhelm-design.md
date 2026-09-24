# GraphHelm — approved design

## Status

Design consolidated on 2026-08-08 and approved as the normative baseline for planning and incremental implementation. This repository does not yet contain any implementation.

## Vision

GraphHelm is an open-source operating system for AI agents. The Studio runs locally and controls a Runtime installed via SSH + Docker on the user's VPS. Every request goes through classification, capability discovery, agent synthesis/reuse, context compilation, graph architecture, policies, lint, and execution.

The harness is built per task from atomic capabilities; there are no fixed domain packs. The graph is adaptive, versioned, and editable. The user is sovereign and can pause, remove gates, or force deployment, with an impact report and waiver. Agents proposed after manual intervention do not start without confirmation.

The system uses an immutable Event Store, a Project Knowledge Graph, and Living Documentation. Context Capsules minimize tokens. Dreams maintains documents, claims, memories, agents, and skills in a shadow workspace; code findings generate normal tasks.

Models are accessed through the Universal Model Gateway: BYOK, direct APIs, aggregators, official subscription runtimes, and local models. Reaching a subscription limit pauses execution; there is no automatic paid fallback.

The project will be fully open source under the MIT license, with no second tier and no CLA. (This line was written on 2026-08-08 as AGPLv3 plus a commercial license; the decision was later changed — see ADR-018.)

## Complete decisions

See:

- `MASTER_PRD.md`
- `docs/DECISION_REGISTER.md`
- `docs/harness/HARNESS_SPEC.md`
- `docs/graph-engineer/GRAPH_ENGINEER_GUIDE.md`
- `docs/graph-engineer/GRAPH_DSL_SPEC.md`
- `docs/ux/STUDIO_SPEC.md`
- the remaining documents listed in `docs/INDEX.md`.

## Implementation gate

Implementation should only start after explicit user review of this documentation and creation of a separate plan. This repository contains no scaffold, product code, or infrastructure changes.
