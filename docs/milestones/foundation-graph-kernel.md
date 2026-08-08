# Foundation Graph Kernel

Status: implemented on milestone branch. Toolchain: Rust 1.97.1, edition 2024.

This milestone is the offline, deterministic base of GraphHelm. It validates the checked-in Graph DSL, publishes immutable content-addressed versions, enforces semantic and policy gates, applies transactional drafts, records append-only events, simulates state transitions without effects, replays projections, and exposes the complete boundary through a JSON CLI.

## Crate boundaries

| Crate | Responsibility |
|---|---|
| `graphhelm-protocols` | Stable wire types, diagnostics, events, drafts, waivers, states, injected clock and ID traits. |
| `graphhelm-schema` | Bounded YAML/JSON loading and Draft 2020-12 validation from embedded schemas only. |
| `graphhelm-graph` | Canonical semantic JSON, SHA-256 identity, immutable versions, and deterministic lint. |
| `graphhelm-policy` | Ordered obligations and owner-waiver eligibility. |
| `graphhelm-events` | Locked, checksummed JSONL batches and pure replay projections. |
| `graphhelm-governor` | Isolated candidate mutation, full validation pipeline, waiver generation, and atomic publication. |
| `graphhelm-simulation` | Bounded, fixture-driven state transitions with no graph payload execution. |
| `graphhelm-cli` | Machine-readable orchestration only. |

Dependency direction remains inward toward protocols. No core crate depends on the CLI, and event replay does not depend on graph implementation code.

## Semantic identity

The digest is lowercase `sha256:` plus SHA-256 of compact canonical UTF-8 JSON. It includes `apiVersion`, `kind`, semantic labels, graph topology, node behavior, expressions, policies, budgets, completion rules, and unknown operational node fields. It excludes graph identity/history fields (`id`, `name`, `executionId`, `version`, `basedOn`) and every node `ui` subtree. Object keys are sorted recursively; array order remains meaningful.

Golden digests cover all three canonical examples. A 256-case property test varies UI coordinates and proves hash stability; operational edge mutations must change the digest.

## Validation, lint, and policy

Schema resources are embedded with `include_str!`, registered by their checked-in `https://p50.dev/schemas/...` IDs, and compiled with `jsonschema` retrieval features disabled. Inputs are limited to 4 MiB.

Stable schema codes are `GHS001_PARSE`, `GHS002_SCHEMA`, and `GHS003_TYPED`. Semantic lint implements `GHG001` through `GHG014` plus warning `GHG101_DEFAULT_TIMEOUT`, with escaped JSON Pointers and deterministic `(path, code, message)` ordering. Policy resolution is limited to `satisfied`, `unsatisfied`, `waived`, and `impossible`. Only complete owner overrides with named requirements and acknowledged risks can waive logical quality obligations. Schema failures, stale bases, hard denies, uncontrolled cycles, missing deploy targets, and missing compensation remain blockers.

## Durable mutation and replay

`GraphVersion` fields are private and have no mutable getter. Draft operations run against a cloned candidate. The candidate is schema-validated, linted, and policy-evaluated before a successor is constructed. Success appends one ordered batch containing proposal, obligation decisions, waivers, publication, and application. Rejection appends proposal/available decisions/rejection but never exposes a successor.

Each newline-terminated event-store line is one `StoredBatch` with stream ID, starting sequence, complete envelopes, and a checksum over canonical envelope JSON. Append holds an advisory exclusive lock, validates committed history, truncates only an interrupted non-newline tail, writes one buffer, flushes, and calls `sync_data`. A corrupt committed line fails closed. Replay requires a contiguous single stream and reconstructs graph, draft, waiver, node-state, and simulation status without external snapshots.

## Simulation subset

The simulator never executes node payloads. It schedules entrypoints and eligible successors in stable node-ID order, emitting `queued -> running -> outcome`. Fixture outcomes are `success`, `failure`, or `unknown`; omitted outcomes default to success. Conditions support booleans, literal `true`/`false`, fixture keys, and `nodes.{id}.output.passed == true|false`. Any unsupported condition follows `onUnknown`; absent behavior pauses with `GHSIM001_UNKNOWN_CONDITION` rather than guessing.

## CLI

```text
graphhelm graph validate FILE
graphhelm graph lint FILE
graphhelm graph hash FILE
graphhelm graph simulate FILE --events PATH [--fixtures PATH]
graphhelm graph draft apply BASE_FILE DRAFT_FILE --actor owner-local --events PATH
graphhelm graph replay --events PATH
```

Every domain command prints exactly one JSON document. `--pretty` changes whitespace only. Success exits `0`; validation/lint/domain failures exit `2`; policy/concurrency application failures exit `3`; I/O or internal failures exit `4`. Paths are reported using caller spelling and no secret, credential, backtrace, or home-directory canonicalization is emitted.

## Verified acceptance

The local Windows gate passes formatting, warning-free Clippy, locked metadata, all workspace tests, and CLI acceptance. There are 63 automated tests across protocol round trips, offline schemas, hash properties, immutability, all lint classes, policy states, append/corruption/replay, transactional drafts, deterministic simulation, and cross-process CLI flows. CI repeats the same gates on Windows and Linux without service containers, provider tokens, model calls, Docker, browsers, or production access.

## Explicitly out of scope

This milestone does not implement the Harness Compiler, Graph Governor service/API, Runtime API, Context Compiler, agent/model/provider execution, Tool Broker, sandbox, database adapters, Knowledge Graph, Dreams Engine, deployment execution, or Studio. Those remain independent plans in `docs/superpowers/plans/2026-08-08-graphhelm-program-plan-index.md`.
