# Reference stack and initial ADRs

## 1. Status

The normative architecture is language-independent. This section recommends a reference implementation consistent with security, portability, community, and extensibility. No code has been implemented.

## 2. Reference stack

### Studio

- Tauri 2;
- React + TypeScript;
- React Flow or a canvas compatible via adapter;
- event-sourced/local cache state management;
- Monaco editor for DSL/schemas;
- Markdown/Mermaid renderers;
- OS keychain for local identity.

### Runtime core

- Rust for daemon, Graph Engine, Policy Engine, Tool Broker, Credential Broker, and sandbox orchestration;
- async runtime;
- gRPC/Connect-compatible API with Protobuf as the binary contract and JSON mapping;
- WebSocket/SSE-compatible event streaming.

### SDKs

- TypeScript;
- Python;
- cross-platform CLI;
- generated clients from the schemas/protocols.

### Extensions

- OCI containers as universal format;
- WASI/WASM for lightweight, more restricted components;
- MCP/HTTP/gRPC adapters;
- pure-data packages for skills/policies/schemas.

### Persistence

- PostgreSQL + JSONB + full-text + pgvector;
- content-addressed artifact store on filesystem, with S3-compatible adapter;
- Git for repositories and docs;
- local encrypted vault with adapter for Vault/KMS.

### Observability

- OpenTelemetry;
- structured logs;
- Prometheus-compatible metrics;
- optional external exporters.

### Isolation

- Docker/Podman rootless for Tier 1/2;
- seccomp/AppArmor/SELinux;
- gVisor/Kata/Firecracker adapter for Tier 3;
- Git worktrees/snapshots;
- egress proxy.

## 3. ADR-001 — Local Studio, Runtime on the VPS

**Status:** accepted.

**Context:** the user wants a local interface with execution/data on their own infrastructure.

**Decision:** Studio acts as the control plane; the VPS as the execution/data plane.

**Positive consequences:** privacy, continuous runtime, greater resources, controlled remote access.

**Negative consequences:** bootstrap, networking, certificates, and diagnostics are more complex.

## 4. ADR-002 — SSH only for bootstrap/maintenance

**Status:** accepted.

**Decision:** use SSH for diagnostics and installation. Normal operation occurs through the Public Runtime API with mTLS.

**Reason:** avoid modeling control in terminal parsing, and enable SDKs.

## 5. ADR-003 — Declarative, typed Graph DSL

**Status:** accepted.

**Decision:** YAML/JSON representation with its own schemas and semantics; immutable Graph Version.

**Rejected alternative:** storing the workflow only as application code or prompts.

## 6. ADR-004 — Policy Engine separate from the LLM

**Status:** accepted.

**Decision:** the LLM produces signals/proposals; a deterministic engine enforces invariants.

**Reason:** security and reproducibility.

## 7. ADR-005 — Append-only Event Store

**Status:** accepted.

**Decision:** transitions and evidence are immutable events. Projections can be rebuilt.

**Consequence:** storage/retention require a policy; auditing and replay become robust.

## 8. ADR-006 — Knowledge Graph in PostgreSQL initially

**Status:** recommended.

**Decision:** model entities/relations/claims in tables/JSONB with an adapter interface. Do not require a separate graph database in the first slice.

**Reason:** fewer operational components, simpler transactions and backup.

**Evolution:** adapters for Neo4j/AGE/others may exist.

## 9. ADR-007 — Content-addressed artifacts

**Status:** accepted.

**Decision:** artifacts immutable by hash, metadata in the database, bytes on filesystem/S3.

**Reason:** dedup, provenance, reproducibility, and caching.

## 10. ADR-008 — Core in Rust, extensions out of process

**Status:** recommended.

**Decision:** trusted core in Rust; extensions via container/WASI/process/remote protocols.

**Reason:** memory safety, performance, and not loading arbitrary plugins into the privileged process.

## 11. ADR-009 — Protobuf semantics with JSON/YAML views

**Status:** recommended.

**Decision:** runtime protocols defined in Protobuf or an equivalent IDL; Graph DSL/manifests in YAML/JSON with JSON Schema.

**Reason:** streaming, generated clients, and human experience.

## 12. ADR-010 — Queue initially in PostgreSQL

**Status:** recommended.

**Decision:** the single-node scheduler uses durable jobs/leasing in PostgreSQL. An external message bus is a future adapter.

**Reason:** reduce V1 complexity.

**Evolution condition:** multi-node scale, throughput, or isolation requirements demanding a NATS/Kafka-like bus.

## 13. ADR-011 — Separate secret broker

**Status:** accepted.

**Decision:** secrets are encrypted and resolved by a broker. Never in the Graph DSL/context artifact.

## 14. ADR-012 — Native model runtimes as first-class adapters

**Status:** accepted.

**Decision:** Codex/Claude Code are not treated merely as chat APIs. The adapter models session, tools, permissions, and quota.

## 15. ADR-013 — No automatic paid fallback

**Status:** accepted.

**Decision:** subscription quota pauses execution. Manual switch is required for BYOK.

## 16. ADR-014 — Graph mutation via Governor

**Status:** accepted.

**Decision:** agents emit signals; only the Graph Governor publishes mutations.

## 17. ADR-015 — Transactional user graph edits

**Status:** accepted.

**Decision:** visual layout is local/immediate; operational topology/config becomes an atomic Graph Draft.

## 18. ADR-016 — Agent node overlays do not promote the definition

**Status:** accepted.

**Decision:** runtime changes apply only to the execution. Reuse requires an explicit save.

## 19. ADR-017 — Dreams in a shadow workspace

**Status:** accepted.

**Decision:** cognitive changes are tested in a snapshot and committed atomically. Code changes become a normal task.

## 20. ADR-018 — AGPLv3 + commercial license

**Status:** accepted subject to legal review.

**Decision:** dual licensing with a non-exclusive CLA.

## 21. ADR-019 — Single-user first with actor identity

**Status:** accepted.

**Decision:** V1 has a local owner, but every event/action includes an actor and an extensible authorization model.

## 22. ADR-020 — No mandatory central service

**Status:** accepted.

**Decision:** update registry, extension registry, hosted telemetry, and provisioning are optional/replaceable.

## 23. Reference repository

```text
graphhelm/
├── apps/
│   ├── studio/
│   └── runtime/
├── core/
│   ├── graph/
│   ├── harness/
│   ├── policy/
│   ├── context/
│   ├── knowledge/
│   ├── agents/
│   ├── models/
│   ├── tools/
│   ├── sandbox/
│   ├── dreams/
│   └── protocols/
├── sdk/
│   ├── typescript/
│   └── python/
├── extensions/
│   └── builtin/
├── schemas/
├── conformance/
├── examples/
├── docs/
├── rfcs/
└── adrs/
```

## 24. Dependency principles

- core modules depend on interfaces, not adapters;
- Studio does not import Runtime internals;
- Policy Engine has no model dependency;
- Event schemas are shared package;
- plugins cannot link privileged core internals;
- model/provider SDKs live in adapters;
- database queries behind repositories;
- sandbox backends behind interface;
- no circular module dependencies.

## 25. Build/release principles

- reproducible builds target;
- signed artifacts;
- SBOM;
- pinned toolchains;
- migration checks;
- conformance suite;
- cross-platform Studio builds;
- x86_64/arm64 Runtime images;
- release manifest with hashes.
