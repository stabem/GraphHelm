# Reference stack and initial ADRs

## 1. Status

The normative architecture is language-independent. This section recommends a reference implementation coherent with security, portability, community and extensibility. No code has been implemented.

## 2. Reference stack

### Studio

- Tauri 2;
- React + TypeScript;
- React Flow or a canvas compatible via adapter;
- event-sourced state management/local cache;
- Monaco editor for DSL/schemas;
- Markdown/Mermaid renderers;
- OS keychain for local identity.

### Runtime core

- Rust for daemon, Graph Engine, Policy Engine, Tool Broker, Credential Broker and sandbox orchestration;
- async runtime;
- gRPC/Connect-compatible API with Protobuf as the binary contract and JSON mapping;
- WebSocket/SSE-compatible event streaming.

### SDKs

- TypeScript;
- Python;
- cross-platform CLI;
- generated clients from schemas/protocols.

### Extensions

- OCI containers as the universal format;
- WASI/WASM for lightweight, more restricted components;
- MCP/HTTP/gRPC adapters;
- pure-data packages for skills/policies/schemas.

### Persistence

- PostgreSQL + JSONB + full-text + pgvector;
- content-addressed artifact store on filesystem, with S3-compatible adapter;
- Git for repositories and docs;
- encrypted local vault with adapter for Vault/KMS.

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

**Context:** the user wants a local interface and execution/data on their own infrastructure.

**Decision:** Studio acts as the control plane; the VPS as the execution/data plane.

**Positive consequences:** privacy, continuous runtime, larger resources, controlled remote access.

**Negative consequences:** bootstrap, networking, certificates and diagnostics are more complex.

## 4. ADR-002 — SSH only for bootstrap/maintenance

**Status:** accepted.

**Decision:** use SSH for diagnostics and installation. Normal operation goes through the Public Runtime API with mTLS.

**Rationale:** avoid modeling control on terminal parsing and enable SDKs.

## 5. ADR-003 — Declarative and typed Graph DSL

**Status:** accepted.

**Decision:** YAML/JSON representation with its own schemas and semantics; immutable Graph Version.

**Rejected alternative:** storing workflow only as application code or prompts.

## 6. ADR-004 — Policy Engine separated from the LLM

**Status:** accepted.

**Decision:** the LLM produces signals/proposals; a deterministic engine enforces invariants.

**Rationale:** security and reproducibility.

## 7. ADR-005 — Append-only Event Store

**Status:** accepted.

**Decision:** transitions and evidence are immutable events. Projections can be rebuilt.

**Consequence:** storage/retention need a policy; auditing and replay become robust.

## 8. ADR-006 — Knowledge Graph in PostgreSQL initially

**Status:** recommended.

**Decision:** model entities/relations/claims in tables/JSONB and an adapter interface. Do not require a separate graph database in the first slice.

**Rationale:** fewer operational components, simple transactions and backup.

**Evolution:** adapters for Neo4j/AGE/others may exist.

## 9. ADR-007 — Content-addressed artifacts

**Status:** accepted.

**Decision:** artifacts immutable by hash, metadata in the database, bytes in filesystem/S3.

**Rationale:** dedup, provenance, reproducibility and cache.

## 10. ADR-008 — Core in Rust, extensions out of process

**Status:** recommended.

**Decision:** trusted core in Rust; extensions via container/WASI/process/remote protocols.

**Rationale:** memory safety, performance and not loading arbitrary plugins into the privileged process.

## 11. ADR-009 — Protobuf semantics with JSON/YAML views

**Status:** recommended.

**Decision:** runtime protocols defined in Protobuf or an equivalent IDL; Graph DSL/manifests in YAML/JSON with JSON Schema.

**Rationale:** streaming, generated clients and human experience.

## 12. ADR-010 — Queue initially in PostgreSQL

**Status:** recommended.

**Decision:** single-node scheduler uses durable jobs/leasing in PostgreSQL. An external message bus is a future adapter.

**Rationale:** reduce V1 complexity.

**Evolution condition:** multi-node scale, throughput or isolation demanding a NATS/Kafka-like bus.

## 13. ADR-011 — Separate secret broker

**Status:** accepted.

**Decision:** secrets encrypted and resolved by a broker. Never in the Graph DSL/context artifact.

## 14. ADR-012 — Native model runtimes as first-class adapters

**Status:** accepted.

**Decision:** Codex/Claude Code are not treated merely as chat APIs. The adapter models session, tools, permissions and quota.

## 15. ADR-013 — No automatic paid fallback

**Status:** accepted.

**Decision:** subscription quota pauses execution. Manual switch is mandatory for BYOK.

## 16. ADR-014 — Graph mutation via Governor

**Status:** accepted.

**Decision:** agents emit signals; only the Graph Governor publishes mutations.

## 17. ADR-015 — Transactional user graph edits

**Status:** accepted.

**Decision:** visual layout is local/immediate; operational topology/config becomes an atomic Graph Draft.

## 18. ADR-016 — Agent node overlays do not promote the definition

**Status:** accepted.

**Decision:** runtime changes apply only to that execution. Reuse requires an explicit save.

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

**Decision:** update registry, extension registry, hosted telemetry and provisioning are optional/replaceable.

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

## 26. ADR-021 — Separate production Event/Evidence Store

**Status:** accepted.

**Context:** the Foundation's local Event Store preserves append-only replay, but production persistence also needs to isolate projects, support concurrency, detect tampering, encrypt sensitive content and comply with retention, legal hold and erasure. Embedding prompts, outputs, logs or other sensitive payloads in the canonical history would make replay and auditability incompatible with authorized erasure. This ADR formalizes [D-035](../DECISION_REGISTER.md) and the documentary gate of Milestone 03.

**Decisions:**

1. **Replay-safe history and erasable evidence are separated.** The immutable envelope and the deterministic projection payload remain in the Event Journal; sensitive evidence may undergo cryptographic erasure and later physical expiration.
2. **Erasure is auditable history.** Authorized erasure adds receipt events and preserves a minimal tombstone. Reads return explicit unavailability and never fabricate content or success.
3. **Keys are mediated externally.** Core repositories depend on `KeyProvider`; PostgreSQL stores ciphertext and encapsulated data-encryption keys, never the key-encryption key.
4. **Local and production repositories have distinct execution models.** The full JSONL `EventStore` remains synchronous for offline CLI; PostgreSQL implements an object-safe asynchronous repository. Both share domain types, errors and conformance behavior.
5. **Legacy records receive no presumed context.** Historically, the previous Foundation envelope would only enter via `LegacyEventImporter` with an explicit `LegacyImportContext`. **Superseded by ADR-022:** this format was never published; the importer, context and runtime compatibility are removed rather than implemented.
6. **Tamper evidence combines chaining and authenticated anchors.** Streams form hash chains; checkpoints and backup manifests are authenticated by `KeyProvider`. Per-event public signatures depend on a future compatibility decision.
7. **Scope is enforced across three layers.** Typed scope, composed relational constraints and PostgreSQL RLS are mandatory; Runtime roles and administration roles are separated.
8. **Events contain only data necessary for replay.** Prompts, outputs, logs, raw tool results and sensitive blobs become encrypted evidence or an artifact reference. Replay remains complete even when referenced evidence is unavailable.
9. **PostgreSQL acceptance is real and isolated.** Concurrency, isolation, migration, backup and restore tests use an ephemeral PostgreSQL instance, with no Docker, production credentials or production infrastructure.
10. **Retention requires explicit authority.** Every expiration or erasure carries a versioned policy, typed scope, authority, reason, idempotency key and dry-run result. Legal hold blocks key destruction and physical deletion.

**Rejected alternatives:**

- **Unified, erasable payload:** deleting or overwriting Event Journal payloads would invalidate hash chains, deterministic replay and the append-only history guarantee.
- **Raw payload retained permanently:** widens the impact of compromise and prevents data minimization, limited retention and authorized erasure of sensitive evidence.
- **Universal content-addressed ledger:** mixes events, evidence and artifact metadata under incompatible semantics; public plaintext hashes could reveal correlation and do not resolve key revocation, legal hold or replay independent of removed content.

**Positive consequences:** replay and projections remain reconstructible; erasure leaves an auditable trail without retaining plaintext; scope, concurrency, integrity, backup and restore boundaries gain verifiable contracts; key and database adapters remain replaceable.

**Negative consequences:** Event Journal, Evidence Store, `KeyProvider`, revocation journal, tombstones, retention operations and authenticated checkpoints require additional states and tests. Erasure between PostgreSQL and `KeyProvider` is a crash-consistent state machine, not a distributed transaction. Database backup and key/revocation epoch custody require separate policies.

**Trust limitations:** RLS reduces the impact of repository/query defects, but does not authenticate that a caller is authorized to choose its own session scope; until Runtime authentication/RBAC exists, the asynchronous repository is a trusted-caller boundary. A compromised database without the `KeyProvider` exposes ciphertext and wrapped keys, but simultaneous control of PostgreSQL and the active `KeyProvider`/KEK defeats authenticated checkpoints. Public signatures and transparency logs remain future work.

**Relationship and supersession:** this ADR refines ADR-005, ADR-007 and ADR-011; it does not revoke them. D-035 remains normative. Any change that makes replay dependent on plaintext, reunites the Event Journal and sensitive evidence, changes erasure semantics, or replaces hash chains/authenticated checkpoints requires a new accepted ADR, an explicit update to the decision register and a compatibility/migration plan. Provisional wire identifiers `p50.dev` remain preserved until an accepted compatibility ADR.

## 27. ADR-022 — Safe persistence projection and pre-release reset

**Status:** accepted.

**Context:** the Foundation model is an authoring contract and contains instructions, objectives, purposes, textual completion contracts, diagnostic details and paths. Persisting a raw `GraphVersionRecord` in the Event Journal would violate ADR-021, prevent effective erasure and let free-form content cross an append-only boundary. Since no format has been published, preserving compatibility with the unsafe internal formats would create risk with no public benefit. This ADR formalizes [D-036](../DECISION_REGISTER.md) and accepts the [safe projection design](../superpowers/specs/2026-08-09-safe-persistence-projection-design.md).

**Decisions:**

1. **Authoring and persistence are distinct representations.** `AuthoringGraph` continues to accept inline content; `PersistedGraphVersion` is a durable, explicit and fallible projection, never a Serde alias of `GraphVersionRecord`.
2. **Externalization by the Governor is mandatory.** Only the Graph Governor classifies fields by typed paths, prepares encrypted Evidence, builds the projection and publishes the mutation. No public constructor allows embedding the authoring graph directly into a persisted event.
3. **Free-form content becomes Evidence.** Instructions, objectives, purposes, textual contracts, free-form explanations, diagnostic details and unregistered execution strings are externalized. The safe, bounded structural topology remains inline.
4. **Semantic identity uses content digests.** `topologyHash` covers the safe topology and typed positions; `semanticHash` covers that topology plus the ordered tuple of each slot and its `contentSha256`. Evidence ID, nonce, ciphertext, encapsulated key and encryption metadata do not affect identity.
5. **Erasure preserves history and blocks unsafe execution.** Replay reconstructs topology, hashes, slots and availability without plaintext. Erased or unavailable Evidence does not change historical hashes; if a required slot is unavailable, materialization fails before any model/tool call.
6. **The format fix is immediate.** JSONL and PostgreSQL read and write only the new, safe contract. There is no legacy compatibility layer (`no legacy compatibility layer`): previous internal formats fail with a generic diagnostic and are never accepted by heuristic.
7. **The pre-release baseline is rebuilt.** `LegacyEventEnvelope`, `LegacyStoredBatch`, `LegacyEventImporter`, contexts, commands, fixtures and fallback branches are removed. The intermediate `1.1.0` release is withdrawn and a single corrected `1.0.0` package becomes the first public baseline, with no migration package.
8. **Persisted diagnostics are safe.** `PersistedDiagnostic` contains only a stable code, severity, a closed `DiagnosticDomainPath`, a closed component and optional authorized references/digests. The domain path is a structural JSON Pointer whose entire sequence is validated against the closed union of registered shapes of GraphHelm contracts: structural fields are closed by position, arrays use canonical decimal indices with limits, and map keys use the exact nominal grammar of the owning contract (`OpaqueId`, `SafeKey` or an equivalent typed rule). There is no generic character whitelist or filename denylist. The empty root is used when there is no safe structural location. Converting a Foundation diagnostic is an explicit, fallible producer operation: `Diagnostic.path` and `Diagnostic.source` are never copied automatically, and unregistered detail is externalized or rejected.
9. **The persisted waiver is bounded.** The `policy-waiver` baseline requires non-empty typed IDs, a safe version, an optional reason omitted when absent and bounded when present, one to 64 bounded risks, a closed scope, canonical UTC timestamps with uppercase `T`/`Z`, a four-digit year and at most nine fractional digits, and no unknown fields. Offsets do not enter the persisted wire: normalizing them at the `0000`/`9999` year boundaries is not bijective with the four-digit profile, and fractions beyond nanoseconds would lose precision in the Rust type. This contract replaces the previous internal draft.
10. **Publication is atomic and fail-closed.** The Governor validates authoring, lint and policy, externalizes and seals content, builds topology/slots, verifies schema, invariants, scope, bijection, limits and hashes, and only then publishes Evidence, references and events. Any failure leaves no active candidate version.

**Rejected alternatives:**

- **Persisting a raw `GraphVersionRecord`:** retains free-form content in immutable history, conflicts with erasure and exposes paths/prose.
- **Graph DSL by references only:** shifts storage detail onto the authoring contract and reduces usability.
- **Fully encrypted snapshot:** makes inspection, replay and rebuild too dependent on content availability.
- **Compatibility with pre-release internal formats:** requires readers, importers and fallbacks for a contract that was never published and cannot be safely accepted by heuristic.

**Positive consequences:** authoring remains simple; journal and projections stay free of plaintext; re-encryption preserves identity; erasure does not falsify history; execution without required content fails closed; JSONL and PostgreSQL share a single wire contract; the first public `1.0.0` is born coherent.

**Negative consequences:** publication now requires deterministic externalization, exact bijection between slots and references, sealing before append, and separate executable materialization. Hashes and internal development data need to be recreated, with no runtime translation.

**Relationship and supersession:** this ADR refines ADR-003, ADR-005, ADR-011, ADR-014, ADR-015 and ADR-021 and makes D-036 normative. It specifically replaces the previous internal assumptions of raw `GraphVersionRecord` persistence, Foundation parity in the journal, persisted Foundation diagnostics, unbounded waiver, legacy import, the `1.1.0` release and runtime compatibility. The historical rationale remains documented, but does not authorize implementation incompatible with this ADR. Provisional wire identifiers `p50.dev` remain preserved except where the corrected representation requires a new document name; any future change to this boundary requires an accepted ADR and an explicit compatibility plan.

## 28. ADR-023 — Authoring paths as typed content positions

**Status:** accepted.

**Context:** Foundation allows path strings across three operational surfaces: context source-scope entries, permission scopes and isolation filesystem writable paths. ADR-022 requires that paths never cross the Event Journal in plaintext, but the closed eight-variant `ContentFieldKind` enum offered no true positions to externalize them. Reusing another field kind, fabricating a structural digest, or rejecting a valid Foundation graph would respectively break typing, executability, or parity between authoring and publication. This ADR formalizes [D-037](../DECISION_REGISTER.md).

**Decisions:**

1. `ContextPath` represents each path string of a registered context source-scope entry; `PermissionPath` represents each path string of a registered permission scope; `IsolationPath` represents each registered writable path string in the isolation filesystem.
2. The three positions are externalized as Evidence, have `restricted` sensitivity, are required for execution, preserve the order of their typed authoring position, and participate in the topology/semantic hashes exactly like other content slots. Evidence ID and cryptographic metadata remain outside identity.
3. None of this plaintext is copied into topology, events, logs, diagnostics or errors. Materialization only retrieves it from available, authorized Evidence; absence blocks execution before any effects.
4. The closed enum grows from eight to eleven variants with exact wire values `context_path`, `permission_path` and `isolation_path`. There is no catch-all, alias or translation.
5. Since no public package has been released, the root schema and the `schemas/releases/1.0.0` snapshot are corrected byte-for-byte in the same baseline. There is no new release, migration, compatibility or legacy branch.

**Rejected alternatives:** reusing `objective`, `instructions` or another semantically false type; persisting the paths inline or as reversible controls; treating paths as non-materializable digests; refusing surfaces accepted by the Foundation contract; creating an alias or intermediate release for a format that was never published.

**Consequences:** the safe boundary now faithfully represents all Foundation path surfaces and keeps replay independent of plaintext. Governor, relational validation and materialization need to record ownership, ordinals and exact bindings for the three positions before publishing these graphs.

**Relationship and supersession:** this ADR refines ADR-022 and specifically replaces any active normative list that limits `ContentFieldKind` to the previous eight variants. D-036 remains valid, D-037 is normative, and any new content surface requires an explicit decision and typed contract.
