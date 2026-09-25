# Reference stack and initial ADRs

## 1. Status

The normative architecture is language-independent. This document began as a pre-implementation
reference; later accepted ADRs record the implementation that now exists. The reference stack stays
coherent with security, portability, community, and extensibility.

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

**Temporary amendment (ADR-039, D-055):** the Milestone 05 loopback HTTP/bearer-token MVP may also use an SSH tunnel for operator API access. This narrowly suspends the SSH-only-for-maintenance restriction for that topology; it does not demonstrate mTLS or satisfy the remote-production identity contract. The exception ends when the mTLS Runtime API is implemented and verified, or an accepted superseding ADR replaces it. Normal remote-production operation still requires mTLS.

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

## 20. ADR-018 — MIT, single license

**Status:** accepted. Supersedes the earlier AGPLv3-plus-commercial-license decision.

**Decision:** MIT for the whole codebase, with no second tier and no CLA. The superseded design
existed to hold a commercial lever over network use; it was traded for adoption. The trade only
runs one way — code already published under MIT stays available under MIT to everyone who
received it.

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

**Context:** the Foundation model is an authoring contract and contains instructions, objectives, purposes, textual completion contracts, diagnostic details and paths. Persisting a raw `GraphVersionRecord` in the Event Journal would violate ADR-021, prevent effective erasure and let free-form content cross an append-only boundary. Since no format has been published, preserving compatibility with the unsafe internal formats would create risk with no public benefit. This ADR formalizes [D-036](../DECISION_REGISTER.md) and accepts the [safe projection design](../specs/2026-08-09-safe-persistence-projection-design.md).

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

### ADR-022 amendment A1 (2026-09-14): bounded declared text

**Status:** accepted. Deciding authority: the owner's standing orchestrator authority (global `CLAUDE.md`, "Orchestrator authority", owner order of 2026-09-07 — the orchestrator decides for the project and does not wait on the owner). Recorded on PR #1071 (issue #1063) as the ADR that AGENTS.md requires when a change contradicts a higher-precedence source instead of resolving it silently.

**Context:** promise 3 of #302 — continuity across harnesses — needs a second harness to read what an execution is FOR from the event stream alone, with no keyring and no Evidence store. The Studio's draft keeps the operator's sentence in the first entrypoint's `objective` (its `metadata.name` is a placeholder); a synthesized graph keeps the goal it was compiled from in `metadata.name`. Under ADR-022 §3 both are sealed Evidence, so the resume briefing could not name the goal without a credential it has no use for.

**Evidence — the contradiction, cited:**

- The code: `apps/cli/src/commands/execution/start.rs:287-305` records `name` (`graph.metadata.name`) and `objective` (the first entrypoint node's `objective`) inline and unsealed on `ExecutionFormDeclared`; `start.rs:440-452` (`declared_text`) bounds them through `graphhelm_protocols::bound_declared_text` (`core/protocols/src/event.rs:751-757`, `MAX_DECLARED_OBJECTIVE_CHARS = 2000` at `event.rs:744`, cut on a char boundary, whitespace-only becomes `None`) and passes them through `graphhelm_graph::validate_durable_content` (`core/graph/src/persistence.rs:446`), the same durable-content scan every append is held to (`core/events/src/integrity.rs:487-495`); text the scan refuses — a `secret://` reference, a token-shaped run — is left OFF the declaration and the start proceeds.
- The rules it contradicts: ADR-022 §3 above ("Instructions, objectives, purposes, textual contracts, free-form explanations, diagnostic details and unregistered execution strings are externalized. The safe, bounded structural topology remains inline.") and [D-036](../DECISION_REGISTER.md) ("the Governor deterministically externalizes free-form content as encrypted Evidence, keeps only safe topology inline").
- What the persisted version actually does: `metadata.name` is NOT inline in `PersistedGraphVersion`. `PersistedTopology` (`core/protocols/src/projection.rs:621-633`) carries `graphId`, `executionId`, `labels`, entrypoints, nodes, edges, budgets, policies and completion — no name — and the Governor registers `graph.metadata.name` as `ContentFieldKind::DisplayName` for sealing (`core/governor/src/externalize.rs:580-586`). The brief for this amendment assumed the name was already inline there; the check found the opposite, and this record says so rather than citing a precedent that does not exist.
- The precedent that DOES exist: `GateFinding.claim` and `GateFinding.remediation` (`core/protocols/src/event.rs:1305-1315`; `schemas/event-envelope.schema.json:731-733`, `maxLength: 2000`) are model-written free text carried inline in the journal, bounded to the same 2000 characters and admitted by the same append-time scan. Bounded, scanned prose in the journal is therefore not new with #1063; what is new is that this prose originates in an authoring field ADR-022 §3 names.

**Affected contracts:**

- `ExecutionFormDeclared` in `core/protocols/src/event.rs:711-739`: `name`, `objective` (both `Option<String>`, `skip_serializing_if`, so histories written before the fields existed re-serialize byte-for-byte and no hash chain moves), plus `executor`.
- `schemas/event-envelope.schema.json:361-376` (`executionFormDeclared`): `name` and `objective`, `minLength: 1`, `maxLength: 2000`, counted in code points as the Rust bound is.
- Readers: `core/execution/src/briefing.rs:179-180` copies the two fields into the `Briefing`; nothing else reads them.

**Alternatives considered:**

1. **Keep the objective sealed and require a keyring for the briefing.** Honours ADR-022 §3 verbatim, but defeats promise 3 of #302: the second harness is precisely the process that has no key, and a briefing that names the goal only when unsealing succeeds is a briefing that usually says "absent". Rejected.
2. **Store only a digest** (`contentSha256` of the objective, or the slot reference). Keeps the journal free of prose, but a digest tells a resuming harness nothing about WHAT to continue; it would still have to unseal to read the sentence, which is alternative 1 with an extra hop. Rejected.
3. **Record the text inline, bounded and scanned, as a label.** Accepted, below.

**Decision:**

1. `ExecutionFormDeclared.name` and `ExecutionFormDeclared.objective` are recorded inline, optional, bounded to 2000 characters (truncated on a char boundary, never refused), and admitted only when `validate_durable_content` accepts them; refused text is omitted, not stored and not fatal to the start.
2. The declared text is a **label, not evidence**. The sealed content slots of the `PersistedGraphVersion` remain the only place an objective lives AS EVIDENCE: identity (`semanticHash`), materialization and every rule that needs the objective's content keep reading the slot. Nothing reads the declared text as authoritative; a briefing that shows it shows what the operator declared at start, the way a status line shows a mode.
3. ADR-022 §3 is amended along this one axis: an authoring objective or display name may ALSO appear inline on the execution's declared form under the bound and the scan above. Every other clause of ADR-022, ADR-023 and D-036 stands; in particular, `PersistedGraphVersion` still carries no name, no objective and no free-form content inline.
4. Any further inline authoring text on a journal event requires its own amendment naming the field, the bound and the scan, following this record's shape.

**Risks:**

- Prose in the journal is exportable: an export of the stream carries the operator's sentence in plaintext, and erasure of the sealed Evidence does not erase this copy. This is the same class of exposure `GateFinding.claim`/`remediation` already carry; it is NOT the same as `PersistedGraphVersion`, which stays prose-free. The bound (2000 chars) and the scan (secret-shaped text refused) limit the surface; they do not remove it.
- Truncation is silent by design: a 2001-character objective is recorded cut, and the briefing does not flag it. The sealed slot keeps the full text.
- A harness that treated the label as the objective's identity would be wrong; §2 above is the rule, and `briefing.rs` is the only reader today.

**Relationship:** amends ADR-022 §3 and the inline-content clause of D-036 along one axis (bounded declared text on `ExecutionFormDeclared`); leaves ADR-022 §1, §2 and §4-§10, ADR-023 and the persisted-version contract unchanged; follows ADR-034/ADR-035's precedent for how a decision amendment is recorded.

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

## 29. ADR-024 — axum as the Public Runtime API's HTTP stack

**Status:** accepted.

**Context:** Milestone 05a (`docs/superpowers/plans/2026-08-13-public-runtime-api.md`) adds `graphhelm serve`, a loopback-only HTTP surface over the existing `execution` command layer, so several agents can operate one project concurrently through the events/signal/status substrate it exposes. §2's "Runtime core" named only an abstract "async runtime" and "gRPC/Connect-compatible API" in the language-independent reference stack; none of `tokio`, `hyper`, `axum` or `actix` appear anywhere earlier in this document. This ADR is the first concrete pin for an HTTP server crate now that code exists.

**Decision:** `axum`, exact-pinned in `[workspace.dependencies]` — `axum = "=0.8.9"` — to the newest version that builds on the pinned toolchain (Rust 1.97.1), verified by building the whole workspace. No newer version exists on crates.io as of this pin (`cargo add "axum@>0.8.9" --dry-run` reports no such version in the registry index). Default features are accepted rather than curated down with `default-features = false`: axum 0.8.9's defaults (`form`, `http1`, `json`, `matched-path`, `original-uri`, `query`, `tokio`, `tower-log`, `tracing`) pull in no TLS stack and no alternate backend — unlike `reqwest`, which the plan explicitly rules out for the test suite's own HTTP client for exactly that reason — so there is nothing here for the workspace's dependency-purity rules to prune.

**Rejected alternatives:**

- **Hand-rolled hyper.** `hyper` alone has no router, no extractor model and no middleware composition; building those by hand would mean re-implementing, and keeping in step with every endpoint this milestone still has to add (the read surface, attributed mutations, `If-Match` concurrency), a slice of what `axum` already provides as a thin layer over that same `hyper`. `axum` is authored by the `tokio` project itself, so this is not trading one dependency for a materially different trust boundary — it trades hand-maintained routing and middleware code for maintained routing and middleware code sitting on the identical HTTP implementation.
- **actix-web.** Built on its own actor runtime rather than composing with `tokio`, which is already this workspace's async runtime (pulled in by `sqlx`'s `runtime-tokio` feature and already driving the `events`/`execution` commands' PostgreSQL paths via `commands::events::runtime()`). Adopting actix would mean two async runtimes coexisting in one binary for no benefit this server needs.

**Rationale:**

1. **Tower ecosystem.** `axum::Router` composes `tower::Layer`/`tower::Service` directly, which is how the auth layer (this task) and every later cross-cutting concern (idempotency-key handling, `If-Match` version checks) are expected to be layered rather than threaded through each handler by hand.
2. **`tokio` already in the workspace.** `axum`'s `tokio` feature (default-on) integrates with the same runtime `commands::events::runtime()` already builds for the PostgreSQL-backed commands; `serve` reuses that exact helper rather than constructing a second runtime.
3. **The register's replaceability rules.** §24 of this document ("Dependency principles") requires database queries, sandbox backends and model/provider SDKs to sit behind interfaces so adapters stay replaceable; the same posture applies to the HTTP transport here even though no `docs/reference` section named it explicitly before this ADR. `graphhelm-cli`'s `serve` module is an adapter over the `execution` command layer (D-039's "never a second path": the HTTP surface calls the identical `execute()` functions the CLI calls), not a redesign of it — swapping `axum` for another tower-compatible server later would not touch `core/execution` or `core/events` at all.

**Constraint:** no handler may hold state beyond the shared `ServeState` passed through `axum::extract::State`. Handlers are free functions (or closures) over an injected, `Clone`-able state handle — in this task, the bearer token; later tasks extend `ServeState` itself rather than reaching for ambient process state — so the router stays testable in-process and stays behind the same D-039 "one path" discipline the rest of the command layer already holds to.

**Positive consequences:** the auth layer and every route this milestone still adds compose as ordinary `tower` middleware and extractors rather than bespoke request-parsing code; the server rides the same `tokio` runtime and `execution` command layer the CLI already exercises, so the CLI/API parity guard has one underlying implementation to hold identical, not two.

**Negative consequences:** `axum`/`tower`/`hyper` (and their own transitive dependencies — `http`, `bytes`, `pin-project-lite`, and similar) enter `graphhelm-cli`'s dependency graph for the first time; the workspace's dependency surface grows by a full HTTP stack where before this milestone the CLI had none. This is accepted because the milestone's own goal — several agents operating one project concurrently over HTTP — cannot be reached with zero HTTP server code, and the alternative (hand-rolled hyper) does not eliminate the dependency, only the router/middleware convenience layered on top of it.

**Relationship and supersession:** this ADR is the first concrete pin under §2's "Runtime core" (async runtime, gRPC/Connect-compatible API) for the Public Runtime API specifically; it does not revise §2's language-independent framing, only records the Rust implementation's actual choice now that code exists. Any change that replaces `axum`, moves the server off `tokio`, or lets a handler hold state outside `ServeState` requires a new accepted ADR.

## 30. ADR-025 — `ureq` as the BYOK adapters' outbound HTTP client

**Status:** accepted.

**Context:** Milestone 05b (`docs/superpowers/plans/2026-08-14-gateway-slice.md`) adds `adapters/model-gateway`, whose BYOK adapters (Task 4) place direct HTTPS calls against the Anthropic Messages API and the OpenAI Chat Completions API on the caller's behalf. ADR-024 pinned `axum` for the *server* side of Milestone 05a — an async, `tokio`-driven HTTP stack — but named no outbound HTTP *client*; nothing earlier in this document pins one. `apps/cli/tests/api_http.rs`'s own doc comment already records the workspace's standing objection to `reqwest` for exactly this role: "an HTTP client crate would drag dependencies (TLS stacks, in reqwest's case) this workspace does not want." This ADR is the first concrete pin for that role now that code exists.

**Decision:** `ureq`, exact-pinned in `[workspace.dependencies]` — `ureq = "=3.4.0"` — to the newest version that builds on the pinned toolchain (Rust 1.97.1), verified by building the whole workspace. No newer version exists on crates.io as of this pin (`cargo add "ureq@>3.4.0" --dry-run` reports no such version in the registry index). Default features are accepted rather than curated down with `default-features = false`: `cargo add ureq --dry-run` and `cargo tree -p ureq -e features` both show ureq 3.4.0's defaults resolve to exactly **rustls** (TLS) and **gzip** (response decompression) — no `native-tls`, no `json`/`cookies`/`multipart`/`socks-proxy`/`charset`/`platform-verifier` this crate does not use. The production transport (`adapters/model-gateway/src/transport.rs::UreqTransport`) builds its `Agent` with `Agent::config_builder().http_status_as_error(false).max_redirects(0).build()` so a non-2xx response comes back as an `Ok(TransportResponse)` rather than an `Err(Error::StatusCode(_))` — the real ureq 3 API name, confirmed by reading `ureq-3.4.0/src/config.rs` directly rather than trusting a remembered API shape (ureq's major-version HTTP-status-as-error behavior is exactly the kind of detail the plan flagged as "verify, don't trust"). `max_redirects(0)` was added after the milestone's final review found the default (10 hops, stripping only `Authorization`/`Cookie`/`Content-Length` per `ureq-proto`'s `Call<Redirect>::as_new_call`) would re-send Anthropic's `x-api-key`/OpenAI's `Authorization: Bearer` to whatever host a 302 `Location` named; with redirects disabled outright a 3xx still comes back as `Ok`, never a `TooManyRedirects` error, so this adds no new transport-level error case. Status interpretation belongs entirely to `byok.rs`'s per-provider mapping tables, never to the transport.

**Feature-graph finding:** `cargo tree -p ureq -e features` and `cargo tree -p rustls`/`cargo tree -i ring` (run against the actual resolved lockfile after adding the dependency) show the TLS stack is exactly rustls `0.23.43` + the `ring` `0.17.14` crypto provider + `rustls-webpki` `0.103.13`, with `rustls-webpki-roots` (the default-on root-certificate feature) bundling Mozilla's root store via `webpki-roots` `1.0.9` rather than reading the OS trust store. No `native-tls`, no `openssl`, no `openssl-sys` anywhere in the resolved graph. Critically, this is **not a second TLS implementation entering the tree**: `sqlx`'s existing `tls-rustls-ring-native-roots` feature (workspace `Cargo.toml`, pinned for the Postgres event store) already pulls in `rustls` + `ring`, and `cargo tree -i ring --locked` confirms both `sqlx-core` and `ureq` resolve to the identical `ring v0.17.14` — one crypto provider, one TLS implementation, shared. The only new dependency-graph mass this pin adds beyond the TLS stack itself is `ureq-proto` (the HTTP/1.1 wire-format layer) and the `gzip` feature's decompression chain (`flate2`, `miniz_oxide`, `crc32fast`, `adler2`, `simd-adler32`) — a compression codec, not a security-relevant addition.

**PR review addendum (Milestone 05b, SECONDARY b):** two more facts from re-running `cargo tree` against the finished implementation, recorded here rather than acted on — remediation, if any, is deferred to issue #35:

- **Two trust stores, not one, despite the shared TLS stack.** The feature-graph finding above already establishes that `ureq` and `sqlx` share the identical `rustls`/`ring` versions — but they do NOT share a root-certificate source. `sqlx`'s pinned feature is literally named `tls-rustls-ring-native-roots` (workspace `Cargo.toml`): it validates the Postgres event store's TLS certificate against the OS's own trust store. `ureq`'s default `rustls-webpki-roots` feature instead bundles Mozilla's root store via the `webpki-roots` crate (confirmed in `Cargo.lock`) — a certificate an OS administrator has explicitly distrusted (a corporate MITM proxy's own root, a compromised CA pulled from the OS store) would still be trusted by `UreqTransport` even though the identical scenario would already be rejected for the Postgres connection. One crypto engine, two independent trust decisions.
- **`base64` is present at two versions.** `cargo tree -i base64` resolves ambiguously: `0.22.1` (via `sqlx-core`/`sqlx-postgres`) and `0.23.1` (via `ureq`/`ureq-proto`). Neither this ADR's dependency addition nor any code in `adapters/model-gateway` chose this duplication directly — it falls out of `sqlx` and `ureq` each pinning their own `base64` majors — but it is a real fact about the resolved graph worth recording: two independently-maintained implementations of the same encoding, in the same binary, are two surfaces to keep patched instead of one.

**Rejected alternatives:**

- **`reqwest`.** Already rejected in this workspace for the identical reason `api_http.rs` states for the CLI's own test client: it drags in a TLS stack and an async runtime dependency the workspace does not want for a synchronous adapter crate that has no other reason to depend on `tokio` at all.
- **Hand-rolled HTTP over `std::net::TcpStream`/`rustls` directly**, the way `apps/cli/tests/api_http.rs` and `adapters/model-gateway/tests/byok_adapters.rs` do for their own *test-side* fake servers. Adequate for a test harness that only ever talks to a local loopback fake it fully controls; inadequate for a production client that must handle real TLS handshakes, redirects, and malformed-response edge cases against live third-party APIs. Re-implementing that correctly is the same "keeping in step with a maintained crate" tradeoff ADR-024 already made against hand-rolled `hyper`.
- **`native-tls`-backed `ureq`.** `ureq` supports both `rustls` and `native-tls` as alternate TLS backends, but `native-tls` is explicitly "never picked up as a default" per ureq's own feature documentation and would mean a second TLS implementation (system OpenSSL/Schannel/Secure Transport, depending on platform) alongside the `rustls` this workspace already carries via `sqlx`. Rejected for the same one-TLS-stack-in-tree reasoning as `native-tls`/OpenSSL generally.

**Rationale:**

1. **Synchronous matches this crate's synchronous boundary.** `adapters/model-gateway` has no other reason to depend on `tokio`: the credential broker (Task 3) exposes plain async fns bridged by a hand-rolled thread-parking executor in tests, not a real runtime, and the CLI's own command layer this crate will eventually be driven from (Task 6) is itself synchronous apart from the `events`/Postgres paths' own `tokio` bridge. `ureq`'s blocking model avoids pulling `tokio` into a crate that would otherwise need it for nothing but one HTTP client.
2. **One TLS stack, not two.** As the feature-graph finding above records, `ureq`'s `rustls` choice resolves to the exact `rustls`/`ring` versions `sqlx` already pinned for the Postgres event store — this pin adds an HTTP/1.1 client on top of infrastructure already in the tree, not a second cryptography implementation to audit and keep patched.
3. **The register's replaceability rules.** §24 of this document requires model/provider SDKs to sit behind interfaces so adapters stay replaceable. `byok.rs` never calls `ureq` directly — every call goes through the `HttpTransport` trait (`transport.rs`), whose only production implementation is `UreqTransport`; swapping the HTTP client later would touch `transport.rs` alone, not `byok.rs` or anything upstream of it. Tests already exercise this seam: `adapters/model-gateway/tests/byok_adapters.rs` never constructs a `ureq::Agent` directly either, only the same trait object production code uses, pointed at a local fake server.

**Constraint:** no code outside `adapters/model-gateway/src/transport.rs` may name the `ureq` crate directly. `byok.rs` and every future adapter in this crate speak `TransportRequest`/`TransportResponse`/`HttpTransport` only, so the interface — not a specific HTTP client — is what the rest of the gateway depends on.

**Positive consequences:** the BYOK adapters gain a maintained, actively-developed HTTP/1.1 client with correct TLS and per-call timeout handling without adding a second TLS implementation or an async runtime dependency. Redirect handling specifically is not merely "correct" but disabled outright (`max_redirects(0)`): `/v1/messages` and `/v1/chat/completions` never legitimately redirect, and ureq strips only `Authorization`/`Cookie`/`Content-Length` before re-sending a redirected request, which would leave a custom credential header like `x-api-key` to travel to whatever host a 302 `Location` named — refusing to follow at all closes that path structurally rather than relying on ureq's header-stripping to cover a header ureq does not know is a credential. The `HttpTransport` seam keeps `ureq` swappable and keeps `byok.rs`'s tests running against a local fake with no real network access, exactly like `apps/cli/tests/api_http.rs`'s own precedent for the CLI's HTTP surface.

**Negative consequences:** `ureq`, `ureq-proto`, and the `gzip` feature's decompression chain (`flate2`, `miniz_oxide`, `crc32fast`, `adler2`, `simd-adler32`) enter `graphhelm-model-gateway`'s dependency graph for the first time; `http`/`bytes` (already present transitively via `axum` in the CLI binary) are now also a direct part of this crate's own compile graph through `ureq`'s re-export. This is accepted because BYOK adapters cannot place an HTTPS call with zero HTTP client code, and every alternative considered either duplicates the TLS stack already in the tree (`native-tls`) or duplicates a maintained crate's correctness work by hand (raw `rustls`/`TcpStream`).

**Relationship and supersession:** this ADR is the first concrete pin for an outbound HTTP *client* in this workspace, complementing ADR-024's server-side `axum` pin — the two are deliberately different points on the sync/async spectrum because they sit on different sides of the process (`graphhelm serve`'s inbound API surface vs. `adapters/model-gateway`'s outbound provider calls) with no shared runtime requirement between them today. Any change that adds `native-tls`, moves this crate onto an async runtime, or lets code outside `transport.rs` name `ureq` directly requires a new accepted ADR.


## 31. ADR-026 — hand-rolled minimal JSON-RPC 2.0 + MCP handshake, no SDK

**Status:** accepted.

**Context:** Milestone 05e (`docs/superpowers/plans/2026-08-14-chat-surface.md`) adds the chat surface: `graphhelm mcp`, a stateless MCP server over stdio whose tools map 1:1 onto Public Runtime API requests (D-039; runtime-design §6.4; `CHAT_SURFACE_SPEC` §3). MCP's stdio transport is newline-delimited JSON-RPC 2.0 — the protocol surface this milestone actually needs is five methods (`initialize`, `notifications/initialized`, `ping`, `tools/list`, `tools/call`) and a closed ten-tool list. The decision is the protocol stack: the official Rust SDK, or a hand-rolled minimal layer.

**Decision:** hand-rolled minimal JSON-RPC 2.0 + MCP handshake — `apps/cli/src/commands/mcp/rpc.rs`, roughly 150 lines: newline-delimited framing with a bounded line reader (`MAX_LINE_BYTES` = 1 MiB, capped via `take()`-bounded reads so an oversized line is refused without ever being buffered whole, and the remainder discarded in bounded chunks so the loop resyncs on the next line), the four standard error codes (`-32700` parse, `-32600` invalid request, `-32601` method, `-32602` params), and one dispatch loop with the invariant the conformance tests pin: a request carrying an `id` gets exactly one reply with that `id` echoed verbatim (string or number), a notification never gets any.

**The declined footprint, measured** (the ADR-024 method — cite the real numbers, not a remembered impression; run 2026-08-16 on the pinned toolchain against this workspace's lockfile): `cargo add rmcp --dry-run -p graphhelm-cli` resolves **rmcp v3.1.2**, default features `base64` + `macros` + `schemars` + `server` + `transport-async-rw`. Actually adding it and diffing `Cargo.lock` shows the workspace grows from **328 to 342 packages — 14 new crates**: `rmcp`, `rmcp-macros`, `schemars`, `schemars_derive`, `serde_derive_internals`, `darling`, `darling_core`, `darling_macro`, `ident_case`, `dyn-clone`, `pastey`, `futures`, `futures-macro`, `tokio-util` (`cargo tree -p rmcp --edges normal` lists a 71-crate resolved closure, most already present via `axum`/`tokio`). The addition was reverted; nothing of it ships.

**Rejected alternative — `rmcp` (the official SDK):** an async-trait/tokio service stack whose value begins with MCP resources, streaming HTTP transports, sampling, and schema-derived tool declarations (`schemars`). 05e ships none of those: the transport is stdio only, the tool list is closed and hand-declared, and the server is stateless per session. Taking the SDK today means carrying two proc-macro stacks (`darling`, `schemars_derive`) and a futures-composition layer for five methods a bounded reader and one match dispatch cover — and it means the wire behavior this workspace most cares about (the notification-silence rule, the oversized-line refusal, id echo fidelity) would be the SDK's to change under us rather than ours to pin with conformance tests.

**Consequences:** we own protocol-revision pinning (`SUPPORTED_PROTOCOL_VERSION`, Task 2, verified against the official spec at implementation time) and the conformance tests (`rpc.rs`'s test module; promoted to black-box `mcp_stdio.rs` once the subcommand exists). The layer must stay minimal: it owns framing and the JSON-RPC envelope only; the method table — including `-32601` for unknown methods and `-32602` for refused params — belongs to the session handler above it.

**The revisit trigger, named:** the first milestone that needs MCP **resources**, **push notifications**, **`structuredContent` tool results** (hosts are moving toward schema-validated structured results — the reviewer-named second candidate), or a **non-stdio transport** adopts the SDK instead of growing this layer. Growing hand-rolled code toward any of those four is the wrong side of the ADR-024/025 "keeping in step with a maintained crate" tradeoff; five methods over stdio is the right side of it.

## 32. ADR-027 — Journey-Proven Development and one extension composition path

**Status:** accepted.

**Context:** repository guidance currently requires RED → GREEN → REFACTOR before every behavior
change. That is a useful local method but it is not equivalent to proving a user journey. It can
spend heavily on tests that observe the wrong boundary while a loading state, browser navigation,
recovery path, provider delivery, or cross-component failure remains unobserved. Issue #210 also
needs a first real skill ecosystem without creating a parallel plugin wrapper. DeepSeek Harness
reached the same packaging boundary by [removing its duplicate repository-plugin
format](https://github.com/deepseek-ai/deepseek-harness/blob/b150a551b8d465e31e418e1b2eaf5e79bbb7d28e/.agents/notes/implemented/simplification/2026-08-09-remove-repository-plugin.md)
and making ordinary Skill, MCP, and native contributions compose through one bundle path. Its
[skill architecture](https://github.com/deepseek-ai/deepseek-harness/blob/b150a551b8d465e31e418e1b2eaf5e79bbb7d28e/docs/subsystems/skills.md)
also separates the stable registry, providers, and lazy consumer instead of loading every skill
body into every task. ADHD's [isolated divergence and separate critic
pass](https://github.com/uditakhourii/adhd/blob/001a29d246fe140665a03ed387a7a30c56f089ed/README.md)
informs the council pattern: first-pass roles share the same bounded contract but not one another's
answers, then a distinct pass clusters claims, attacks traps, and deepens survivors. GraphHelm keeps
that mechanism while selecting role count from risk instead of fixing an arbitrary fan-out.

**Decision:** GraphHelm adopts [Journey-Proven Development](../harness/JOURNEY_PROVEN_DEVELOPMENT.md).
Every user promise is compiled into typed observation obligations. Deterministic risk policy selects
the smallest adequate mix of tests, observers, and independent agents. Missing proof capability
fails with `OBSERVER_MISSING`; a weaker proxy cannot satisfy a stronger fact. Retry history is
append-only and classifies first-pass, recovered, flaky, or unresolved outcomes without replacing
earlier evidence.

The JPD waiver profile is deliberately stricter than the base persisted waiver wire contract from
ADR-022: every newly authored JPD continuation waiver must carry a non-empty reason. The base schema
keeps `reason` optional only so legacy persisted waivers remain decodable; an old reasonless record
cannot be reused as authority for a new override.

The existing `Extension` manifest is the sole plugin envelope. A package composes ordinary skills,
MCP adapters, agents, observers, evaluators, policies, schemas, graphs, and fixtures through that
manifest. Installation and version resolution own source and lock state; the bundle owns explicit
composition. Host wrappers are thin and deletable. No plugin links privileged core internals, and a
skill calls GraphHelm only through public CLI, MCP, or HTTP. A skill may author a schema-bound local
advisory artifact, but that artifact creates no operational authority. Data packages may be validated
without activation; code extensions remain out of process under ADR-008.

**Rejected alternatives:** universal TDD as the development constitution; deleting focused tests;
agent voting as a quality gate; treating a later green retry as a clean first pass; one monolithic
JPD skill; an arbitrary target number of skills; a second `.graphhelm-plugin` wrapper alongside the
Extension manifest; loading every discovered directory; and in-process execution of untrusted
extension code.

**Consequences:** plans must describe the user journey, evidence strength, failure and recovery
states, and observer availability before selecting test methods. Skills stay small because
contributions are split by contracts, effects, permissions, or evidence—not by a numeric quota.
Plugin validation and task-local Skill Capsules can ship before a general installer. Browser proof
still requires a separately installed observer and must refuse honestly when absent. This ADR makes
D-041 normative and supersedes the universal-method wording in `AGENTS.md`; it does not weaken the
local gate or completion evidence requirements.

## 33. ADR-028 — Containment-safe code-index provider sessions

**Status:** accepted.

**Context:** issue #480 introduces the first consumer of codebase-memory-mcp's machine-readable
`structuredContent`. The provider keeps its active index under `CBM_CACHE_DIR`, and every retrieval
process must address the same canonical cache root. The current Tool Host deliberately clears the
environment, redirects home and temporary directories, and runs a `ShellAction` inside a Tier 1
ephemeral worktree. Passing the host cache into that sandbox would expose a writable path outside
containment; omitting it would silently select an empty or different index. Direct process launch,
automatic indexing, and a second ungoverned MCP path would all bypass the Tool Broker. ADR-026 also
named `structuredContent` as a trigger to adopt an MCP SDK instead of growing GraphHelm's hand-rolled
stdio server layer.

**Decision:** separate pure decoding from live provider access. A pure adapter may decode an already
captured MCP tool result and derive a transport receipt from an existing `ToolCallRecord`; this does
not create a session, transport, or protocol implementation and therefore does not extend the
hand-rolled ADR-026 server. It must fail closed on absent or malformed `structuredContent`, provider
errors, excessive bytes or nesting, inconsistent page metadata, repeated cursors or offsets, page
limits, and unfinished pagination. Provider `best_effort` coverage remains `Unknown`; it can never
be promoted to complete coverage. Receipts expose only facts present in `ToolCallRecord`; invocation
and executable identity remain explicitly unavailable.

Live wiring is deferred until the Tool Broker owns a bounded provider-session capability. That
capability must use a maintained MCP SDK, satisfying ADR-026's revisit trigger, and must verify the
provider executable before starting it. The broker copies a GraphHelm-owned, immutable provider
snapshot into Tier 1, pins and verifies its digest, and sets `CBM_CACHE_DIR` only to a path inside
that sandbox. Index production is a separate authorized operation and never occurs during
retrieval. The session returns recorded tool results to the pure decoder. Until this complete path
exists, the live boundary returns a typed unavailable result; no caller may fall back to a direct
command, host cache mount, network call, or automatic index mutation.

**Rejected alternatives:** passing the host `CBM_CACHE_DIR` through `ShellAction`; allowing provider
writes outside Tier 1; starting codebase-memory-mcp directly from Runtime or an adapter; adding
provider-specific environment escape hatches to Tool Host; treating a fresh empty cache as the
requested project; auto-indexing on retrieval; expanding GraphHelm's hand-rolled MCP server into a
client/session implementation; and inventing executable or invocation identity from stream digests.

**Consequences:** the first #480 slice is useful offline: it provides strict, reusable decoding and
honest receipts against real provider-shaped fixtures, but claims no live connectivity. The
follow-up broker capability must define snapshot production, digest verification, executable
verification, session lifetime, byte/page limits, and audit evidence before Runtime, CLI, API, or
MCP surfaces can consume it. Fixture capture and upstream-source provenance remain recorded beside
the [decoder fixtures](../../adapters/codebase-memory-mcp/tests/fixtures/README.md), so a shape
change requires evidence rather than an invented local contract. This makes D-042 normative,
refines ADR-008, ADR-013, ADR-026 and D-041, and does not weaken Tool Broker authorization or
containment.

## 34. ADR-029 — Closed execution accounting identity and interoperable token bounds

**Status:** accepted.

**Context:** issue #222's first durable accounting slice must distinguish measured usage from
missing observations. The executor's current `WorkSummary.input_tokens` is the provider-reported
input field, but the model gateway does not preserve every prompt-cache component. Calling that
number compiled input or a complete provider total would make cached work look falsely cheap.
Separately, the first receipt draft reused `ArtifactBinding` for execution identity. That generic
development binding requires repository and index snapshots, while a persisted execution event
owns neither. Filling those fields from an event hash would invent provenance. The pure
codebase-memory adapter accepted by ADR-028 decodes recorded provider results and transport facts;
it exposes no complete token-cost producer and therefore cannot fill these accounting categories.

**Decision:** the execution-accounting receipt is a closed, versioned Evidence wire contract. Its
`executionBinding` is a dedicated type with the constant kind `execution_started_event`, the exact
event-envelope schema identity and version, the persisted event ID and canonical event hash, the
exact execution-scoped `RepositoryScope`, and the complete persisted producer actor `{type,id}`.
Runtime construction accepts the verified `execution_started` envelope, recomputes its event hash,
and refuses a missing or mismatched execution scope. Deserialization requires that same journal
envelope and compares the decoded binding to the freshly derived one; a valid-looking hash or actor
is never trusted on shape alone. The actor type is identity material, so two actor classes sharing
an ID remain distinct. The receipt never accepts a generic `ArtifactBinding` and never synthesizes
`repoSnapshot` or `indexGeneration`.

`provider_reported_input_tokens` and `output_tokens` are measured only when `WorkSummary` reports
them. `provider_total_input_tokens` and `compiled_input_tokens` remain explicitly unavailable until
their complete producers exist, including prompt-cache components. Since #1065 `compiled_input_tokens`,
`eligible_candidate_tokens` and `tokens_saved` are produced per accounted attempt, `derived` under
`bytes-div-4/v1`, in the sealed `context-provenance@1` record beside each reply; the receipt's own
lines stay `unavailable` until the next frozen baseline admits the change. Every token value is limited to
9,007,199,254,740,991 before serialization and after deserialization, matching interoperable JSON
integer semantics. The registered schema is closed, fixes field order and provenance/value shapes,
and rejects the wrong binding kind, schema domain, digest shape, actor identity, extra data, and
unsafe integers.

**Rejected alternatives:** relabeling partial provider input as compiled input; summing a provider
total without cache-read and cache-creation usage; silently omitting unavailable categories; hashing
only actor ID; truncating actor IDs to fit `OpaqueId`; reusing `ArtifactBinding`; setting both
snapshot coordinates to the event hash; accepting arbitrary schema IDs or binding kinds; and
serializing a Rust-private structure without a registered schema and conformance fixtures.

**Consequences:** old pre-release receipt bytes from this unmerged branch are intentionally not
accepted. Durable receipts carry only real journal identity and honest observations. Context
Compiler, retrieval, formatting, index, and complete provider-total producers remain follow-up work
under #222; this slice does not close that issue and exposes no new public CLI, API, MCP, Studio, or
model-gateway surface. This makes D-043 normative, refines ADR-021, ADR-022 and ADR-028, and leaves
Tool Broker containment unchanged.

## 35. ADR-030 — Fail-closed retry policy for completed tool verdicts and conflicting causes

**Status:** accepted.

**Context:** a tool attempt can end with an honest completed verdict, fail before producing a
verdict, or time out without completing one. The historical executor plan treated every completed
non-zero exit and every `HostError` as retryable. That wording collapses distinct facts: a non-zero
exit is the tool's completed verdict, while the durable `HostError` record retains a stable code but
not the operating system `ErrorKind` needed to classify the failure as transient or permanent.
Separately, the Graph DSL permits an author to place the same cause in both `retryOn` and
`doNotRetryOn`. Choosing either list by precedence would make the result depend on implementation
order, while discovering the conflict only after scheduling can leave a node `Ready` and the
execution unfinished.

**Decision:** retry classification is fail-closed and verdict-aware. A completed tool attempt with a
non-zero exit is terminal by default. A graph author may opt in to retry that cause only with
`retryOn: [tool_exited_non_zero]`; `doNotRetryOn: [tool_exited_non_zero]` explicitly preserves the
terminal verdict. A tool timeout remains retryable because no tool verdict completed. Durable
`HostError` codes are terminal under the current record: the missing `ErrorKind` means GraphHelm has
no deterministic evidence for a transient/permanent split. GateCheck outcomes and retry semantics
are unchanged by this ADR.

The sets named by `retryOn` and `doNotRetryOn` must be disjoint. Any overlap is an invalid retry
policy, not a precedence rule. GraphHelm refuses the conflict deterministically before any node
effect, records stable diagnostic evidence, and settles the affected execution without silently
stranding the node in `Ready` or leaving the execution running. Attempt limits and backoff remain
additional bounds; they never convert a terminal cause into a retryable one.

**Rejected alternatives:** retrying every completed non-zero exit, which repeats a real verdict
without explicit author intent; treating all durable host errors as retryable, which invents a
transient classification after `ErrorKind` has been lost; letting `retryOn` or `doNotRetryOn` win by
precedence, which makes contradictory policy look valid; detecting the overlap only after a node
effect; and changing GateCheck behavior as part of a tool-outcome decision.

**Compatibility and supersession:** this ADR supersedes only the contradictory retry examples in
`docs/superpowers/plans/2026-08-14-real-executor.md` that map completed non-zero exits and
`HostError` to retryable failure. That file remains an immutable historical implementation plan;
its other design statements are unaffected. Existing graphs with disjoint retry sets retain their
declared behavior except at the two causes decided here: a completed non-zero exit is retryable only
through its explicit `retryOn` opt-in, while a durable `HostError` remains terminal under the current
record regardless of retry declarations. A graph whose lists overlap was ambiguous rather than
valid and is now refused before effects.

**Consequences:** authors must opt in before GraphHelm retries a completed non-zero tool verdict.
Timeouts remain recoverable, durable host errors remain honest terminal evidence, and contradictory
retry declarations cannot hang an execution at `Ready`. Implementations must preserve the stable
refusal evidence and terminal settlement across replay. This makes D-044 normative, refines ADR-003
and ADR-005, and does not change GateCheck semantics, schemas, or any public Studio surface.
## 36. ADR-031 — Immutable retrieval coverage sidecar and fake provider boundary

**Status:** accepted; superseded on the source-fallback clause by ADR-035 (accepted 2026-09-02) — the "source fallback unavailable" statement below is no longer the standing decision on that one axis.

**Context:** `RetrievalPlan` is immutable pre-execution intent. The first #219 implementation
classified a caller-constructed `IndexResponse`, so no producer path emitted durable evidence that
another journey could join to the exact plan, step, query, project, repository snapshot, index
generation, provider capability, or Tool Broker call. Adding those observed fields to
`RetrievalPlan` would mutate intent after execution. Adding another `DevelopmentKind` would also
expand the approved artifact vocabulary for evidence that belongs beside, not inside, that plan.
ADR-028 permits strict decoding of recorded provider output but explicitly does not authorize a
live MCP session.

**Decision:** add the closed `RetrievalCoverageReceipt@1` sidecar. Runtime alone constructs it after
calling a brand-neutral `StructuralCodeIndex` port and independently checking the provider's echoed
plan binding, step and query digests, D-027 scope, content-derived repository snapshot, separate
index generation, provider/capability version, and the digest of the exact durable Tool Broker
record. The receipt also carries typed path and negative-scope coverage entries with gap ranges,
terminal non-looping pagination, declared and Runtime-observed result/page/byte/token limits,
canonical repository-relative hits, typed source/reindex fallback outcomes, and a canonical SHA-256
digest. Deserialization recomputes those facts against the expected request and Tool Broker record.

The first implementation uses only a deterministic fake port. A `best_effort` provider result can
never become `complete`. Empty `complete` licenses absence only when every requested path and every
bounded negative scope has exact complete coverage, terminal pagination, and no gap. Missing,
partial, skipped, excluded, extraction-gap, unknown, unresolved, or uncovered evidence remains
`negative_claim_unverified`. Stale generations refuse before receipt publication. Source fallback
is explicitly `unavailable`; there is no successful fallback spelling until a real producer exists.

**Rejected alternatives:** continuing to accept caller-built `IndexResponse` as producer evidence;
mutating `RetrievalPlan`; adding a tenth `DevelopmentKind`; trusting provider totals, summaries, or
coverage confidence; storing a bare Tool Broker stream digest without binding the complete record;
inventing a successful fallback outcome; opening a direct MCP client; mounting the host provider
cache; auto-indexing during retrieval; or using a native index before its own contract exists.

**Consequences:** #211 and later journeys can join immutable retrieval evidence to an exact plan,
but this slice does not establish live provider connectivity or complete repository coverage.
The current pure codebase-memory decoder remains useful input to a future broker-owned adapter, not
authority for this fake. Live MCP still requires ADR-028's contained, digest-pinned Tool Broker
session. No CLI, API, MCP, Studio, network, native-index, or source-reader surface is added. This
makes D-045 normative, refines ADR-028, and leaves #219 open for the real producer and fallback path.

## 37. ADR-032 — Two-axis memory lifecycle: semantic validity separate from publication state

**Status:** accepted.

**Context:** #220 (task-004) shipped the safe `MemoryAdmissionRefused` slice (#488): a durable
refusal event that never carries rejected content, gated on explicit per-project opt-in, with the
disabled and incoherent-input arms returning before any repository access. That slice deliberately
excluded any lifecycle decision. Ahead of it, task-004 had already built `MemoryState`
(`provisional | published | superseded | withdrawn`) and `MemoryTransition`
(`publish | supersede | withdraw`) as a single closed axis (`core/governor/src/memory.rs`,
`policies/memory-transition.yaml`, `schemas/memory-transition.schema.json`), with `apply_transition`
enforcing exactly three legal tuples.

A current-main audit (`cdf1fc4`) found this one-axis model conflicts with a normative contract that
already exists: [Data & Protocols](../architecture/DATA_AND_PROTOCOLS.md) §16 declares
`memory_record.status` as `candidate | validated | deprecated | contradicted | expired` — the same
five-value semantic vocabulary §15 already uses for `claim.status`, with `claim.relationships.supersedes`
as a reference to another claim, never a state name. Task-004's axis is a different, narrower set that
conflates two questions a memory record's caller can face independently: *is this content still
believed* (semantic) and *is this content visible in the default read* (publication). Concretely,
`superseded` — as shipped — is a STATE a record enters, collapsing three distinct reasons a caller
must be able to tell apart: a predecessor DEPRECATED by policy, one CONTRADICTED by new evidence, and
one merely superseded by a better observation with no fault in the original. One state name for all
three erases which repair an operator should make. The issue's own audit trail records that "full
lifecycle work must wait for an accepted ADR plus Decision Register entry; an RFC alone is not
authority" before this contradiction may be frozen into a wire contract — this ADR is that decision,
formalizing D-046. It is documentation only: no code, schema, or test changes accompany it, and #220
stays open after it lands.

**Decisions:**

1. **Two independent axes replace the single `MemoryState` axis.** Semantic:
   `candidate | validated | contradicted | deprecated | expired`, wire-identical to
   `memory_record.status` in Data & Protocols §16 — this ADR does not change that document, it makes
   an implementation obligated to honor the field that was already normative there. Publication:
   `unpublished | proposed | published | withdrawn`. A record's full state is the ordered PAIR of both
   axes; no implementation may flatten them back into one enum for storage, wire, or comparison.
2. **`superseded` is removed as a state.** A record that supersedes an earlier one carries a
   `supersedes` relationship to that predecessor's immutable identity, mirroring `claim.relationships`
   in Data & Protocols §15. The predecessor's own semantic axis moves to `deprecated` or `contradicted`
   according to the caller's stated reason — decided by the caller at the transition site, never
   inferred by the mechanism from the fact that a successor exists.
3. **Withdrawal moves only the publication axis, never the semantic axis, and never deletes or
   rewrites the journal.** Withdrawn content stays fully replayable and auditable, satisfying #220's
   own acceptance criterion ("expired/stale/withdrawn memory is excluded by default but remains
   auditable") without tension: exclusion from the default view is a publication-axis fact, not an
   erasure. Opt-in governs CAPTURE; withdrawal governs the VIEW. These are different clocks and neither
   is read against the other.
4. **Erasure is a distinct operation from withdrawal and is not reinvented here.** A memory record's
   evidence may be erased through the cryptographic-erasure mechanism ADR-021 (D-035) already
   establishes for the Event/Evidence Store (`EvidenceErasureRequested` /
   `EvidenceErasureCompleted` / `EvidenceCiphertextDeleted`): the decryption key is destroyed, the
   ciphertext and audit trail remain, and replay reports unavailability rather than fabricating
   content or silently succeeding. Erasure is orthogonal to both lifecycle axes — a record in any
   semantic/publication pair may have its evidence erased without changing either axis's value.
5. **A handoff into a scope without capture opt-in is refused by name at the transition site.** The
   refusal carries a closed code (e.g. `handoff_target_not_opted_in`) rather than silently completing
   the transition or silently dropping it.
6. **A refusal record carries only a closed code and a closed location, never the content, excerpt, or
   a content-derived digest that caused it.** `MemoryAdmissionRefused` (#488) already implements this;
   this decision extends the same constraint to every refusal-shaped event the lifecycle implementation
   adds under this ADR, so the guard is a standing rule rather than a property of one shipped event
   type.
7. **General rule for a frozen `1.0.0` artifact found wrong: authored content in a frozen release
   never changes; derived metadata that was derived wrong is re-derived in place, with the guard
   updated in the same commit and provenance recorded — and the correction must EXHIBIT a control
   proving it touched only derived metadata, never assert it.** This answers the question #220's own
   audit trail named as this ADR's to decide ("whether the checked-in pre-release `1.0.0` artifacts
   are corrected in place or versioned"), and it is grounded in the first real instance rather than
   argued in the abstract: `#508`/PR `#511` found the `event-envelope` catalog digest recorded in
   BOTH the live catalog and the frozen `releases/1.0.0/` snapshot never matched the schema's own
   canonical digest. The digest field is a derivation over already-shipped, byte-frozen schema
   content, not authored content itself, and re-deriving it corrects the record to describe the bytes
   it always claimed to describe without touching a single authored byte. The fix's own diff is the
   control that makes this checkable rather than asserted: two one-line catalog entries plus a guard
   comment recording why the correction is legal, **zero `*.schema.json` files touched** — the
   empty set over authored files is the evidence, not the claim, that only a derivation moved. The
   rejected alternative (a versioned `1.0.1`) would leave `1.0.0` self-inconsistent forever and
   require a permanent exemption cell in the very integrity guard that caught the error — a standing
   lie with a standing waiver, for a value nobody outside the tree ever depended on being that
   specific wrong number.
8. **The memory-transition schema/policy pair is a narrower instance of decision 7's rule, one step
   easier.** Neither `memory-transition.schema.json` nor `memory-transition.yaml` is part of any
   snapshot under `schemas/releases/1.0.0/` at all (verified against `origin/main` at `1a5ff3b`: that
   directory has no `memory-*` entry) — nothing has published these wire spellings yet, so there is no
   frozen release to preserve and no derived-vs-authored question to answer. ADR-022's "no legacy
   compatibility layer" precedent applies directly: the two-axis shape replaces the one-axis shape
   byte-for-byte in the same files, with no migration, alias, or intermediate release, and no control
   diff is required because nothing shipped is being corrected.

**Rejected alternatives:**

- **Keep the one-axis model and special-case the three collapsed reasons inside `superseded`'s own
  handling.** This is the flattening pattern the audit already named: the state name stops
  distinguishing what an operator must respond to differently, and the distinction has to be
  reconstructed from context every time instead of being carried on the record.
- **Add a `published: bool` beside the existing one-axis enum instead of a true second axis.** Does
  not resolve the semantic/publication conflation this ADR exists to fix, and cannot express states a
  real correction workflow needs — e.g. content already CONTRADICTED by new evidence that a caller has
  not yet unpublished, a transient and legitimate combination a boolean bolted onto one enum cannot
  represent without becoming a second enum in practice.
- **Defer the decision until Studio ships a lifecycle UI.** Rejected: this is a data-model question
  independent of any UI, #220 has stayed open on this exact blocker, and every day of deferral is a day
  the shipped one-axis model can gain more callers to migrate later.

**Consequences:** #220's full lifecycle implementation is unblocked and must consume this ADR's two
axes rather than extend the one-axis model. `MemoryState`, `MemoryTransition`, `apply_transition`,
`MemoryRecord` (`core/governor/src/memory.rs`), the shipped policy/schema pair, and every test binding
them (`core/governor/tests/memory.rs`, `apps/cli/tests/development_cli.rs`) must be rewritten for the
two-axis shape as follow-up implementation work — none of that is done by this ADR itself. The
`wire_vocabulary!`/`closed_vocabulary!` macro machinery already in place for these types (#362, #381)
is reused as the spelling generator for both new axes; it is a generator, not the axis count, so
nothing about it constrains this decision. `MemoryAdmissionRefused` (#488) needs no change: it never
encoded the one-axis model and satisfies decision 6 already. Decision 7's rule is standing beyond
memory: it governs any future correction to a checked-in `1.0.0` artifact anywhere in this
repository, not only the memory schemas, and any such correction must cite decision 7 and exhibit
the same kind of control (`git diff --stat` naming zero authored files touched, or the equivalent for
the artifact class in question) rather than assert derivation.

**What this ADR does not establish**, named so nobody reads more into it than is decided: it does not
implement the two-axis types, the transition matrix, event persistence for lifecycle transitions, or
any schema/policy edit — those remain #220's open implementation work. It does not decide which
actor/authority may move which axis, the exact set of legal (semantic, publication) pairs, or how
`expired` interacts with a caller-chosen retention window — those are implementation-time decisions
within the two-axis frame this ADR fixes, not decisions this ADR is making for them.

**Relationship and supersession:** this ADR refines ADR-021 (D-035) by routing memory-record erasure
through the identical mechanism rather than inventing a second one, and is the first ADR to make
Data & Protocols §16's `memory_record.status` field normative for an implementation rather than a
descriptive reference. Decision 7 answers, for the whole repository and not only for memory, the
"corrected in place or versioned" question `#220`'s own audit trail named as this ADR's to decide;
it is grounded in `#508`/PR `#511` — the `event-envelope` catalog digest recorded wrong in both the
live catalog and the frozen `releases/1.0.0/` snapshot, fixed in place with a two-file,
zero-`*.schema.json` diff as the exhibited control — rather than argued from a hypothetical. This ADR
supersedes no prior ADR — no earlier ADR fixed the memory lifecycle's axis count or the
frozen-artifact correction rule — but any future change to the two memory axes, the
supersedes-as-relationship rule, the withdrawal/erasure boundary, or decision 7's authored-vs-derived
rule and its control requirement requires a new accepted ADR. This makes D-046 normative.

## 38. ADR-033 — Countersign clearance gains a signature: signed now while the wire is empty, verified at append, never in the fold

**Status:** accepted (2026-08-31 — review by L, both Codex P1 threads triaged with verdicts in the
PR; the merge of #581 is the acceptance act, matching how ADR-032 landed. The register row D-047
and this status move together: a proposed ADR must not sit behind a normative register entry, so
either both land accepted or neither lands).

**Context:** #529 (J's measurement at `e8f5c958`): `ClearanceVerifier::Countersign` carries
`{identity, key_fingerprint}` and **no signature**. Both fields the fold checks are journal-readable,
so a countersignature currently adds no authority beyond append access — the mechanism does not do
the one thing a countersignature is for, which is being a *second* authority independent of whoever
did the work. This is not remotely exploitable (append requires the local bearer token or filesystem
access; there is no generic event-append endpoint), and `MachineReplay` is unaffected (verified
against the journaled evidence digest since #521). Three further measured facts shape the decision:
**no Countersign event has ever been journaled** — no CLI, HTTP, or MCP surface emits clearance
commands at all (the append path is #159's pending surface work); `signature_unverifiable` is the
ninth entry of `REFUSAL_REASON_CODES` (`core/protocols/src/event.rs`), a vocabulary deliberately
frozen ahead of its producers by four-lane agreement; and PR #527 holds the line with a trap — no
production surface in `apps/cli` may construct a `Countersign` clearance, so the first producer goes
red until verification (or this ADR's refusal) lands in the same change. #541 records the trap's
reach limits. The custody shape was already named in #161's close-out: per-identity public keys as
verified children of the anchored keyring directory, reusing `open_and_verify` /
`verify_child_identity` in `adapters/sealed-key-provider` — not a widened `KeyringDocument`. The
question this ADR answers is the one #161's close-out deferred to #529: does `Countersign` gain a
signature field (and `signature_unverifiable` gain producers), or is the refusal retired as
unrepresentable-by-type?

**Decisions:**

1. **`Countersign` gains a mandatory signature field, decided now while the change is free.** The
   deciding fact is emptiness: zero Countersign events exist in any journal, because zero producers
   exist. A wire-format change today costs no migration, no compatibility arm, and no versioned
   variant; every day after the first producer ships, the same change buys a journal migration. The
   two options are not symmetric in reversibility — retiring the refusal now and adding the
   signature later pays the migration; adding the field now and (if custody never lands) never
   verifying it pays nothing.
2. **The signature binds to the claim instance, not just to the signer.** It must cover, at
   minimum, the stream identity, the `claim_seq` being cleared, and the claim's journaled evidence
   digest — so a valid signature cannot be replayed onto a different claim, stream, or bundle. The
   exact canonical byte form is specified where the schema lives when the producer is implemented,
   not restated in prose here: a second producer of a canonical form is how two serializations
   drift while both look correct.
3. **Verification happens at append time, in the command layer — never in the fold.** The append
   surface (#159's pending work) verifies the signature against the identity's registered public
   key and refuses with `signature_unverifiable` on failure — the ninth code gains its producer at
   the same moment the first Countersign producer exists, which is exactly the sequencing the #527
   trap enforces by going red on any earlier producer. The fold's check stays journaled-against-
   journaled (`identity` + `key_fingerprint` against `clearance_registry`): no key material, no
   I/O, no cryptography in replay, so replay remains a pure function of the log and replays
   identically forever.
4. **Custody follows #161's named shape**: per-identity public keys as verified children of the
   anchored keyring directory, through the sealed-key-provider's existing `open_and_verify` /
   `verify_child_identity` — not a widened `KeyringDocument`. This ADR inherits that decision
   rather than reopening it.

**Rejected alternative — retire `signature_unverifiable` as unrepresentable-by-type**, accepting
that `Countersign` is a journaled attestation trusted at the authenticated append boundary. Its
costs, named: (a) it forfeits the mechanism's sole purpose — under one shared bearer token, anyone
who can append can clear as any registered identity, so the identity sets #159's human-judgment
nodes declare become unverifiable prose; (b) it saves nothing today — the #527 trap already
prevents accidental producers at zero runtime cost, so there is no burden the retirement would
lift; (c) removing the ninth code from a vocabulary frozen by four-lane agreement is its own
coordination cost; and (d) it takes the irreversible branch of the asymmetry in decision 1.

**Consequences:** #159's clearance surface implements sign-and-verify-or-refuse when it builds the
append path — the signature field, the append-time verification, and the `signature_unverifiable`
producer land in that lane, red-first, with this ADR as their authority. The #527 trap stands until
that producer lands and then narrows per #541, whose reach work becomes the guard that keeps every
producer inside the verified door. `Countersign`'s doc comment stops promising a verification that
does not exist and points here instead.

**What this ADR does not establish:** the exact signature algorithm and encoding — deferred to the
implementing lane **under one named constraint that is not deferred: the primitive must be
asymmetric**, signing capability strictly separate from verifying capability (found by Codex
review). Measured: the sealed-key-provider today offers only XChaCha20-Poly1305 and HMAC-SHA-256,
both symmetric, and no signature crate sits in its dependencies — so its current primitives do
NOT qualify as the signature primitive, though the provider remains the custody home for the keys
per decision 4. A shared-secret MAC countersign would collapse into the rejected alternative one
layer down: anyone who can verify can forge, and the registry's journal-readable "fingerprint"
would name a secret rather than a public key. Also not established: key
rotation and revocation beyond the `clearance_identity_registered` / `clearance_identity_revoked`
events that already exist; who may be a countersigning identity for a given node — that is
graph-definition vocabulary, owned by #159; and **the released-schema question, deferred
explicitly rather than left looking resolved** (found by L reviewing this ADR): the frozen
`schemas/releases/1.0.0/event-envelope.schema.json` declares the countersign branch CLOSED —
`additionalProperties: false` over exactly `type`/`identity`/`keyFingerprint` — so a clearance
carrying the new field is *refused* by a 1.0.0 validator, not treated as unknown. Whether that
costs a release bump or is absorbed by the current-schema evolution rules is the implementing
lane's decision, made when the producer lands. Until it is made, the #527 trap performs a second
job it is not credited for: keeping any producer from journaling an event the released schema
would refuse. The emptiness argument of decision 1 covers this axis too, measured: the countersign
wire word appears in four schema files and nowhere else on the wire, and exactly one constructor
exists outside the enum's declaration — a test fixture — so the schema, the code, and the
journals are all still on the cheap side of the change.

**Relationship and supersession:** implements the half of #161's close-out that was measured
not-implementable-as-written and deferred to #529; consumes #161's custody decision unchanged;
constrains #159's surface lane and #541's trap-reach lane. Pairs with D-047. It does not modify
ADR-032 or any earlier ADR.

## 39. ADR-034 — The contained producer's framing: the revisit trigger governs layer GROWTH, and the layer did not grow

**Status:** proposed.

**Context:** D-042 and ADR-028 require live provider retrieval to run over a **maintained MCP SDK
session**; ADR-028's rejected alternatives include *"expanding GraphHelm's hand-rolled MCP server
into a client/session implementation"*. #576 shipped the contained producer without an SDK, and the
clause passed every reviewer including me — I reviewed and approved that PR, and the framing is not
something I raised. Codex found it. This ADR exists because a standing decision was violated in fact
before anyone noticed it applied, which is itself the strongest argument for restating it in terms a
reader can check.

**What actually ships, measured at `origin/main` rather than described:**

```
adapters/codebase-memory-mcp/src/provider.rs
  messages written to the child                     3, in a fixed array:
                                                      initialize | notifications/initialized | tools/call
  capabilities ADVERTISED by the client             "capabilities": {}  -- empty
  capabilities advertised by the SERVER             NEVER READ. `retrieve` consults only the
                                                      reply bearing id 2; the initialize result is
                                                      not parsed, so no server capability can enter
                                                      this client's behaviour
  process lifetime                                  one call, one process, stdin closed
  the reply                                         decoded by decode_search_graph, which REQUIRES
                                                      structuredContent (lib.rs:651,
                                                      DecodeError::StructuredContentMissing)
  a reply that does not decode                      StructuralCodeIndexError::Unavailable
```

**One thing this table deliberately does NOT claim, because an earlier draft claimed it and K was
right to strike it.** That draft opened with *"imports from the hand-rolled MCP server: NONE"* as
evidence of restraint. It is not evidence of anything: `apps/cli` declares `[[bin]]` and no `[lib]`,
so **no crate in this workspace can depend on it** — the count is zero by topology, and would read
zero for a producer that would gladly have reused the server if the manifest allowed it. A number
that cannot come out otherwise measures nothing. The shape clause of ADR-028 is answered below by
what the framing *is*, not by an import count that was never free to differ.

**The governing text is not the one the case opened on.** ADR-028's rejected line names a shape —
*expanding the server into a client/session*. Measured above, that shape did not occur: no server
code is reused and nothing persists between calls. But answering the shape clause is not answering
the decision, because the operative rule is ADR-026's revisit trigger:

> *"the first milestone that needs MCP resources, push notifications, `structuredContent` tool
> results (hosts are moving toward schema-validated structured results — the reviewer-named second
> candidate), or a non-stdio transport adopts the SDK instead of growing this layer. **Growing
> hand-rolled code toward any of those four** is the wrong side of the ADR-024/025 'keeping in step
> with a maintained crate' tradeoff; five methods over stdio is the right side of it."*

**One of the four named capabilities is in the live path, and is mandatory there.** So a defence
resting on "a one-shot exchange is not a session" fails: the trigger fires on capability, not on
session shape, and ADR-028 records this exact capability as the thing that fired it.

**Decisions:**

1. **The trigger governs GROWTH OF THE LAYER, not consumption of a payload, and that distinction is
   the whole decision.** Its own words are *"growing hand-rolled code toward any of those four"*, and
   the reason ADR-024/025 give is that a hand-rolled protocol layer rots as it accretes cases. This
   framing accreted nothing: three fixed messages, no capability advertised and none read, no server-initiated
   traffic, no state between calls. `structuredContent` arrives in the **reply body** and is read by
   the strict bounded decoder ADR-028 explicitly permits — the half of #480 that ADR-028 blessed.
   The layer stayed at its minimum while the payload got richer, and the trigger is about the layer.

2. **That reading is only honest if the layer is BOUND, so this decision ships with a mechanism
   rather than an intention.** A guard pins the request set at exactly those three methods with empty
   capabilities. A fourth message, a capability advertised or consulted, a second tool, a resources or
   notifications method, or a non-stdio transport breaks the build and reopens this ADR. Without the
   guard, decision 1 is a promise about future restraint — which is the thing ADR-026's trigger
   exists because we cannot keep.

3. **The SDK requirement in D-042 and ADR-028 is AMENDED, not waived**, and only along this axis:
   live retrieval may use hand-rolled framing **while that framing is bounded by decision 2**. Every
   other D-042 clause stands unchanged — the broker-owned session, the pinned snapshot, the verified
   executable, the confined cache. Those four were proven adversarially end to end and nothing here
   touches them.

4. **A deviation refuses rather than degrades.** Any reply the bounded decoder cannot read becomes
   `Unavailable`, a typed refusal, never a partial belief. This is what makes rot in the layer
   *visible* instead of silent, and it is the property that would be lost first if the framing grew.

**Scope — which speakers this ceiling binds, and the split is measured, not asserted.** The
ceiling above was written with one artifact in view, and a second hand-rolled MCP client exists. An
unnamed subject is the same defect as an unenforced ceiling, so the census is recorded here:

```
adapters/codebase-memory-mcp/src/provider.rs      ships; the operator's live retrieval path
  messages 3 (fixed)  tools 1  methods 3  capabilities {}  transport stdio

tools/development-benchmark/src/bin/generate-retrieval.rs   dev-dependency of apps/cli; run by hand
  messages 2 + one per corpus case -- UNBOUNDED BY CONSTRUCTION
  tools 2 (index_status, search_graph)   methods 3   capabilities {}   transport stdio
```

**The clauses split, and they do not all split the same way:**

1. **Layer-wide — binding on every hand-rolled MCP client in this repository:** the three methods
   `initialize` / `notifications/initialized` / `tools/call`, **no capability advertised and none
   read**, and
   **stdio only**. These are the ADR-026 trigger's own named capabilities, and nothing about shipping
   or not shipping changes whether growing toward them is the wrong side of the ADR-024/025 tradeoff.
   Both speakers satisfy all three today, measured — so this clause records a fact and installs a
   guard against its changing, rather than promising future restraint.

2. **Path-specific — binding only on what ships on the operator's retrieval path:** **exactly three
   messages** and **exactly one tool**. The bench speaker meets neither, and both divergences are
   legitimate rather than tolerated: its message count scales with the corpus by design, and
   `index_status` is a provenance control that exists *because* its output freezes as committed
   evidence — a store built from another revision must not silently supply rows read against this
   one. The shipped path has no such need, and a refusal there is cheaper than a check.

**Why this is not the convenient answer.** The reading that exempts the bench speaker entirely is the
one that leaves this ADR's decision 1 unfalsified, and it should be distrusted for that reason. It
also fails on the merits: ADR-024/025's stated reason for the trigger is that hand-rolled protocol
code **rots as it accretes cases**, and the bench speaker is precisely the artifact that accretes
cases — one message per corpus entry, growing whenever the corpus grows. The rot mechanism applies to
it more than to the producer, not less. What does not apply is the *count*, because a count is the
wrong unit for a loop. So the bench speaker is bound by form and not by volume, and that is a
different claim from being unbound.

**The guard must derive its population, not name it.** The guard implementing decision 2 reads a
single hard-coded path. That measures the form exactly and leaves the population a hand-chosen guess:
a third speaker appears unguarded, and nothing turns red. A guard has two chosen parameters and this
one has measured only the first. Before this ADR is accepted, the layer-wide clauses in (1) must be
enforced over an **enumerated** population — every hand-rolled MCP client in the workspace, derived
from the tree — so that a speaker nobody remembered to add fails by name.

**Rejected alternative — adopt a maintained MCP SDK now, costed:** (a) a new dependency under the
M06 freeze, which is a decision with its own owner and its own review rather than a side effect of
this one; (b) an audit of the SDK's stdio and process handling against the Tier 1 containment the
four D-042 clauses establish — the SDK would run inside that sandbox and inherit its guarantees, so
its behaviour must be re-verified rather than assumed; (c) re-verification of the whole D-042 chain,
proven adversarially end to end across #544, #551, #553 and #576 — replacing the framing invalidates
the composition cell that ties program, session, snapshot and receipt together, and that cell is the
strongest artifact the containment work produced; and (d) it buys protocol surface this producer does
not use, today: three fixed messages against a provider whose reply is already strictly decoded. The
cost is present and the benefit is future-shaped, which is exactly why decision 2 exists — so the
benefit can be bought on the day it is needed rather than argued about now.

**Rejected alternative — amend on the grounds that a one-shot exchange is not a session
implementation:** the argument this case opened on, and it does not survive the trigger. It answers
ADR-028's shape clause and leaves ADR-026's capability clause untouched, and the capability clause is
the one with the reason attached. Recorded as rejected rather than omitted, because it is the reading
a future reader reaches for first.

**Consequences:** the guard in decision 2 lands before this ADR is accepted, red-first, with the
current three messages as its subject and with the layer-wide clauses enforced over a population
derived from the tree rather than a hard-coded path. `provider.rs`'s framing comment stops describing the exchange
and starts naming the bound. D-042's SDK clause gains a pointer here, so a reader of the decision
register meets the amendment where the requirement is. If the guard ever fires, the correct response
is to adopt the SDK, not to widen the guard — and that sentence belongs in the guard's own failure
message, where the person holding the failing build will read it.

**What this ADR does not establish** — and this list is meant to be complete, because an entry
missing from it is the failure mode this project has now recorded five times:

- **which SDK**, its licence, its maintenance posture or its transport support — the adopt-now option
  remains live and unspecified, and choosing it is a separate decision under the M06 freeze;
- **the guard's exact form** — whether it pins serialized bytes, method names or the message count is
  the implementing lane's call, provided a fourth message cannot pass and provided the layer-wide
  clauses resolve their population by enumeration;
- **anything about GraphHelm's own hand-rolled MCP SERVER**, a different artifact under ADR-026 and
  untouched here;
- **pagination, a second tool, or a second provider** — each is growth under decision 2, and each
  reopens this ADR by construction rather than by anyone remembering to;
- **whether `structuredContent` decoding should move behind an SDK later** — decision 1 says the
  decoder is not the layer, not that the decoder is permanent;
- **the review that let the clause pass unnoticed through #576** — a real gap, mine among others, and
  a process question rather than an architectural one.

**Relationship:** amends D-042 and ADR-028 along one axis; consumes ADR-026's revisit trigger and
narrows its reading to layer growth, with a mechanism attached; relies on ADR-024/025's tradeoff and
does not modify it. Pairs with D-048.

## 40. ADR-035 — Bounded source fallback becomes available at the compile layer, its real producer being the workspace channel itself

**Status:** accepted 2026-09-02 by the Orchestrator on the owner's delegated authority; technical gate: K's review of #608 (pin on `29ded95b`); the composed path's production consumer is tracked in #724 (#223/#224 closed without it, see #722).

**Context:** ADR-031 and D-045 both declared source fallback `unavailable`, each with the same
condition: *"there is no successful fallback spelling until a real producer exists."* #219's
composed selector adds the compile layer that, when a claim's coverage is non-complete, consults a
bounded `BoundedSourceSearch` channel SUPPLIED BY THE CALLER (`core/runtime/src/ports.rs`), whose
paths join the claim — the only entry point, `compile_plan_composed_against<R: SourceReader>`, takes
the channel as an INJECTED parameter, so `source_fallback_available()` is `true` here because the
PATH exists in the runtime, not because a producer is present. The workspace-backed implementation
of the port (`WorkspaceSourceChannel`) is delivered by #622; the production CONSUMER that calls
`compile_plan*` in production is tracked in #724 (#223/#224 closed without it, see #722). The base plan compiler
(`compile_plan`/`compile_plan_within`) already has a non-test consumer — `tools/development-benchmark`
(`src/lib.rs:845`); it is only the COMPOSED/fallback path (`compile_plan_composed_against`) and its
production channel that await #724. The register row for D-045 records this. Codex was right
that an
amendment to the register that contradicts an accepted ADR is procedurally incomplete without an
ADR/RFC recording the alternatives and the recommendation — AGENTS.md requires exactly that, and
ADR-034 set the precedent this milestone. This ADR is that record.

**The measured finding the amendment rests on, not a preference:**

```
StructuralCodeIndex over the shipped index:
  required evidence in non-code files (JSON schemas, changelogs, .sql, .md)   UNREACHABLE
  reason                                                                       the provider filters
                                                                               non-construct nodes
                                                                               BY DESIGN, at any query
```

So the compiler grew a second bounded channel rather than the corpus getting easier questions. The
"real producer" ADR-031 awaited is, on the source axis, the `WorkspaceSourceChannel` itself: a
deterministic, in-process, bounded producer of source-file candidates — not a fake, and not an MCP
client. Its workspace-backed implementation is **delivered by #622**
(`adapters/tool-host/src/source_channel.rs`, `impl BoundedSourceSearch`), and the production
CONSUMER that calls `compile_plan*` in production is tracked in **#724** (#223/#224 closed without it, see #722).
#219 delivers the compile-layer path and the port; the flag is `true` because that path exists and
the caller injects the channel — no code between the merges claims a producer that is not there.

**What this decides, and the boundary it does NOT cross:**

- Bounded source fallback is available at the RETRIEVAL COMPILE LAYER, through
  `compile_plan_composed_against`, licensed exclusively by a non-complete coverage state
  (`Complete` never consults, so no workspace walk enters an ordinary compile). The channel's
  paths pass the same escape check, declared limits, snapshot-freshness re-check and canonical
  form as the index's own hits; the channel's typed failure returns beside the outcome; coverage
  is never promoted.
- The RECEIPT keeps no successful-fallback spelling. `validate_response` still records the
  fallback as `unavailable`, and the closed coverage vocabulary gains no success state here.
  ADR-031's receipt-level claim therefore STANDS unchanged: a composed claim is compile-layer
  evidence, never receipt evidence. Integrating composition at the validated-receipt boundary
  (scope binding of the channel's results, and an outcome the receipt can record) is a
  frozen-schema change tracked as its own slice in #655.
- The REINDEX fallback stays `unavailable`. This ADR moves one axis, not both.

**Rejected alternatives:** keep source fallback unavailable and leave non-code required evidence
permanently unreachable (rejected by the measured finding — it makes the benchmark's own questions
unanswerable for a real class of evidence); add the receipt's success spelling now (rejected here
only to keep the frozen-schema change its own reviewable slice, #655 — not rejected in principle);
open a direct MCP client or a native index for the source axis (rejected by D-042, unchanged);
relabel or regenerate the frozen corpus to dodge the question (that is #637, a different defect
about tree provenance, not about whether fallback exists).

**Consequences:** D-045's source-fallback clause is amended in the register with a pointer here;
ADR-031's receipt-level and reindex-axis claims are untouched; #655 carries the receipt-boundary
integration. A reader who follows ADR-031 to "fallback unavailable" now finds the transition
recorded rather than a silent contradiction in the higher-precedence register.

**Relationship:** amends D-045 and the source-fallback clause of ADR-031 along one axis; leaves
ADR-031's receipt vocabulary and reindex axis unchanged; depends on nothing in ADR-034 but follows
its precedent for how a decision amendment is recorded.

## 41. ADR-036 — `cancel` becomes a Studio site tool, behind two confirmations

**Status:** accepted (product decision delegated by the owner, 2026-09-02; recorded on PR #662).

**Context:** `docs/ux/STUDIO_MVP.md` §6 listed `cancel` as a deliberate omission from the WebMCP
site tools — "the destructive verb, and this journey does not need it" — and the adapter carried a
guard test that failed the day anyone added it, so that the addition would have to be argued. Phase 2
of #105 orders the Studio to OPERATE: every execution verb the Public Runtime API exposes for the
execution screen becomes an operator button, with WebMCP parity so an agent can do what the operator
can. `cancel` is on that screen. The bot's review of #662 (adapter.ts:595) named the contradiction
precisely: the higher-precedence subsystem specification still said "not a site tool" while the PR
registered the tool and updated only the README. This ADR is the required record of that contract
change; the guard test was rewritten with this argument, as its own comment demanded.

**Decision:** `cancel` IS a Studio site tool (`graphhelm_cancel_execution`), under these guarantees:

1. **Two confirmations, one per surface.** On the page, the first press only ASKS — the question and
   its answer live in the page (no browser `confirm()`), and "keep running" backs out with zero calls.
   Through WebMCP, the host's own tool-call confirmation prompt applies to every write tool, this one
   included; parity does not skip the consent the page requires.
2. **The tool says DESTRUCTIVE in its first word**, states that every unfinished node is recorded
   Cancelled and that an append-only log has no undo, and instructs the agent to confirm with the
   person unless they explicitly asked for the cancellation.
3. **Attribution is preserved.** A cancel through the tool is recorded as `agent` /
   `studio-webmcp-adapter`, never as the operator; the host's confirmation is consent, not authorship
   (STUDIO_MVP §5).
4. **The Runtime still refuses what it always refused.** `cancel.rs:62` rejects a run that is
   already `completed`, `failed` or `cancelled`; the page renders the button disabled with that reason
   (`components/legality.ts`, closed over the Runtime's seven states) and the tool relays the refusal
   as `refused` with the diagnostic. Nothing here widens what the API accepts.
5. **"delete" stays forbidden** — nothing on this API erases, and the adapter's guard test now pins
   that no tool name may imply it.

**Consequences:** STUDIO_MVP.md §6 no longer lists `cancel` as an omission and points here; the
README's tool table carries the tool with its destructive marking. A future verb that is destructive
in the same sense (there is none today) follows this ADR's shape or reopens it.

**What this ADR does not establish:** any change to the Runtime's cancel semantics; any bulk or
multi-run cancel; any tool that erases evidence, events or executions.

**Relationship:** amends STUDIO_MVP.md §6 (Deliberate omissions) along one axis; consistent with
D-039 (chat-first operation is an adapter over the Public Runtime API, never a second operational
path) and with STUDIO_MVP §5 (agent actions are recorded as agents).

## 42. ADR-037 — The systemd VPS install and update path is an exception to D-002, recorded before it ships

**Status:** accepted on the merge of #595.

**The status names the event and not a calendar date, which is a deliberate departure from this ADR's own `accepted <date>` instruction and is recorded here so it does not read as an oversight.** This edit rides in #595's squash, so it is written before the button is pressed: any date put here would be a prediction about when that happens, and a document whose entire purpose is that the register never contradicts the tree should not rest on a prediction. The event is exact and verifiable from the merge itself. The calendar date belongs in #595's merge comment, which is where the Consequences section below already sends it.

**Context:** `docs/DECISION_REGISTER.md` D-002 is normative for version 0.1 and says *"Existing VPS connected via SSH; installation and updates via Docker."* PR #595 (owner's lane) ships `deploy/upgrade-vps.sh`, `deploy/backup-vps.sh`, `deploy/restore-vps.sh`, `deploy/graphhelm-backup.service`, `deploy/graphhelm-backup.timer` and `docs/operations/VPS_UPGRADE_BACKUP_RESTORE.md`: a **systemd**-managed install, update, backup and restore path with no container in it. #663 measured that nothing in `docs/adr/`, `docs/rfc/` or this file records that divergence, so the day #595 merges the highest-precedence document describes a system the repository does not ship. This ADR is the record #663 asks for, written while the path is still on a branch so the register never contradicts the tree for even one commit.

**Measured at `origin/main` `0576153b`, rather than described:**

```
git grep -icE '\bsystemd\b|\bsystemctl\b' origin/main -- deploy docs   0 (word-bounded: the unbounded -i form returns 3, all SYSTEMDRIVE/SYSTEMROOT in docs/, not systemd)
gh pr view 595 --json files                                      the six deploy/ files and the runbook above, OPEN, head a67ab257
docs/DECISION_REGISTER.md:8                                      D-002 as quoted
```

So today D-002 is true of the tree; it stops being true at the merge of #595, and only then.

**Decision:** the VPS install and update path MAY be managed by systemd units and shell scripts instead of Docker, as an exception to D-002's "installation and updates via Docker" clause and along that axis only. The SSH connection clause of D-002 stands. Docker remains the packaging for the runtime image (`Dockerfile`, `docker-compose.yml` on main are unchanged by #595); the exception covers the *host-side* lifecycle — install, upgrade, backup timer, restore — which #595 implements as units and scripts because a timer-driven backup and an in-place upgrade need the host's init system, not a container's.

**Affected contracts:** D-002 (amended along one axis by this ADR, annotated in the register); `docs/operations/VPS_UPGRADE_BACKUP_RESTORE.md` (the runbook this ADR authorises; until acceptance it must say the exception is pending, per #663); the install story in #330 and the restore privilege fix in #786, both of which assume the systemd path.

**Alternatives considered:** (a) keep D-002 as written and re-do #595 as a Docker-managed lifecycle — rejected: the backup timer and the in-place upgrade would then run inside a container that has to manage its own host, which is the shape the runbook exists to avoid, and it discards measured, reviewed work for a sentence; (b) patch the D-002 row silently in the deploy PR — rejected by #663's own reasoning: a governance change carried inside a deploy diff has no evidence, alternatives or recommendation of its own and is invisible to the next reader; (c) this ADR, proposed now and accepted when #595 merges — chosen because it is the only option under which the register and the tree never disagree.

**Consequences:** D-002 gains an annotation pointing here. The runbook keeps its "pending" wording until this ADR's status changes to accepted. **Who performs that transition, named, because a conditional status otherwise depends on someone remembering an obligation attached to an event they may not attend:** the presser of #595 flips this ADR to `accepted <date>` and removes the D-002 annotation's "not in effect" clause in the same squash or in the commit immediately after, and says so in #595's merge comment. `.factory/MERGE-CHECKLIST.md` at the time of writing has no step that sends a presser to an ADR (measured by the first pass on PR #957: zero real mentions), so the obligation is written in #595's own thread rather than assumed from the checklist; a generic checklist item ("does this PR's body name an ADR whose status is conditional on this merge?") is requested from the checklist's owner. No cell reddens main on this: a docs status must not be the reason no PR can go green. No code changes. No schema changes.

**Relationship:** amends D-002 along the install/update axis; records the decision behind #595, #330 and #786; closes #663 as the governance record it asked for.

## 43. ADR-038 — Two Runtime read contracts reversed by the integrated MVP verification: an unknown execution id is a 404, and a fixture-only route listing is a 200

**Status:** accepted with PR #1091 (issue #1083). Recorded because two lower-precedence records — the 05a milestone note in `docs/milestones/runtime.md` ("an unknown execution id reads as empty, not 404") and the reconciled Task 0 decision in `docs/superpowers/plans/2026-08-14-chat-surface.md` ("a fixture-only server requires the query param or answers 400") — described the opposite of what #1091 ships, and `AGENTS.md` refuses a silent resolution.

**Context, measured by the blind integrated verification of the MVP (#1082, #1083):**

- F1: `GET /v1/executions/{id}` and `/briefing` for a well-formed id that names no stream answered `200` with every field null and `attention: can_sleep`. The Studio rendered a calm, empty run for a typo. The CLI `status`/`briefing` did the same. A read that cannot distinguish "nothing happened" from "this run does not exist" is not an answer an operator can act on.
- F6: `GET /v1/gateway/routes` on a fixture-only Runtime (no `--manifest`), asked without the `manifest` query, answered `400`. Every Studio session logged that as a red console error, for the true answer "no routes are configured".

**Decision:**

1. A well-formed execution id that names no stream is `GHCLI028_EXECUTION_NOT_FOUND` at `/execution`: HTTP `404` on status and briefing, the same diagnostic from the CLI, `isError` from the MCP `status`/`briefing` tools, and the evidence route once its sealing check passes. The guard lives in the command layer (`apps/cli/src/commands/execution/status.rs`, `briefing.rs`), which both surfaces call, so CLI/HTTP parity is kept by construction. Unchanged: the events tail (`/events`) answers an empty page for an unknown id, and the render a mutation replies with after it commits never refuses. `execution status --html` applies the same check before writing a snapshot, so a refused command has no file side effect.
2. `routes` on a fixture-only Runtime without the query answers `200` with `{"configured": false, "routes": [], "reason": "no gateway manifest is configured on this server"}`. `probe`, which needs a manifest to act on, keeps the `400` naming the parameter. A manifest that is named (flag or query) but cannot be loaded or validated is still refused on both.

**Affected contracts:** `docs/milestones/runtime.md` (05a and 05d notes, updated in the same PR), `docs/superpowers/plans/2026-08-14-chat-surface.md` Step 2 (annotated), `apps/cli/src/error_codes.rs` (new code), `apps/cli/tests/api_http.rs`, `execution_cli.rs`, `mcp_stdio.rs` (pinned). Studio: reads `configured`, keeps the `400` fallback for older Runtimes, and treats the 404 on start as "the run does not exist yet".

**Alternatives rejected:** a store-layer not-found signal (deferred in 05a) — it would change the event store contract for a distinction the command layer already has all the facts for; keeping `400` on the listing and silencing the browser console — hides the true answer behind client code.

**Precedence note:** no entry in `docs/DECISION_REGISTER.md`, no accepted ADR and no schema under `schemas/` states either old behaviour; both records were milestone and plan notes. This ADR is the record `AGENTS.md` asks for so the reversal is not silent.
## 44. ADR-039 — A container keeps the loopback-only bind and joins the host's network; no flag relaxes the guarantee

**Status:** accepted. The decision was already in effect in the tree when it was recorded: the shipped `docker-compose.yml` (PR #327, `0339862d`) runs the container with `network_mode: host` and `GRAPHHELM_BIND: "127.0.0.1:8080"`, and `install/VPS_REHEARSAL.md` describes how to check that path against `http://127.0.0.1:8080/health` on the host. The rehearsal has not been executed, so this is a configured topology and an unexecuted validation procedure, not release evidence. This ADR writes down the choice that file made, so the next author of a container path does not make the security call implicitly (#128).

**Context:** `serve --bind` is validated by `parse_loopback_bind` (`apps/cli/src/commands/serve/mod.rs:1765` at `95a7ad9d`), which refuses any non-loopback address unconditionally — *"the Public Runtime API is never exposed beyond localhost"* — and the refusal is proven able to fail by `a_non_loopback_bind_is_refused_fail_closed_before_anything_is_opened` (`apps/cli/tests/api_http.rs:1432`). There is no flag that relaxes it. D-002 says installation and updates happen via Docker, and a container on Docker's default bridge network has its own loopback: a process bound to `127.0.0.1` inside it is unreachable from the host regardless of `-p`, so D-002 and the guard are in tension the moment a container path is built on the bridge (#128, raised while drafting #106). The two ways out named in #128: run the container in the host's network namespace, or add an explicit off-by-default flag that relaxes the guard for containers.

**Source configuration inspected at `origin/main` `95a7ad9d`; the VPS rehearsal remains unexecuted:**

```
docker-compose.yml          network_mode: host · GRAPHHELM_BIND: "127.0.0.1:8080" · read_only · cap_drop ALL · no-new-privileges
Dockerfile                  ENV GRAPHHELM_BIND=127.0.0.1:8080 · USER 10001:10001
install/VPS_REHEARSAL.md    unexecuted curl procedure for http://127.0.0.1:8080 on the host (lines 68–90); no VPS result
git grep -n 'is_loopback' origin/main -- apps/cli/src/commands/serve   one site, the guard above; no override flag anywhere
```

**Decision:** a containerised deployment runs in the **host's network namespace** (`network_mode: host` / `--network=host`) and binds loopback, exactly as a bare-metal or systemd deployment does. The loopback guarantee in `parse_loopback_bind` stays **unconditional**: no flag, environment variable, build feature or container-specific branch relaxes it. Remote access to the current Milestone 05 API is by a temporary SSH tunnel to the host's loopback, which is what the loopback constraint already wants (#106's blueprint). This is operator reachability for today's plain HTTP and bearer-token MVP; it is not an mTLS implementation, an identity waiver, or a remote-production endpoint. The bridge-network path with `-p` is not supported and is not to be documented as if it were.

**The guardrail, written now rather than after:** if a future need makes a non-loopback bind necessary, that is a **new ADR** superseding this one, and it is not acceptable without all three of: TLS on the HTTP surface (today `sqlx`'s `tls-rustls-ring-native-roots` secures only the Postgres connection, not this server); a two-key arming — an explicit flag **and** a matching environment variable, so a copied command line can never produce a non-loopback bind by itself; and the guard's test extended so the refusal is still proven able to fail with the flag absent. Any one of the three without the others is the implicit security call this ADR exists to forbid.

**Affected contracts:** ADR-002 (temporarily amended to permit operator API access through SSH for the Milestone 05 loopback MVP, beyond diagnostics and installation); D-002 (unchanged; this ADR fixes how its Docker path reaches the host); D-055 in `docs/DECISION_REGISTER.md` (the row for this decision); `docs/milestones/runtime.md` ("The server": one sentence pointing here); `docker-compose.yml` and `Dockerfile` (already conform; a change to `network_mode` or to `GRAPHHELM_BIND` away from loopback is a change to this ADR). `docs/architecture/SYSTEM_ARCHITECTURE.md` remains unchanged: its mTLS, Runtime identity, and remote-production endpoint requirements are preserved and deferred until their implementation and observer exist.

**Alternatives considered:** (a) an off-by-default `--allow-non-loopback` flag — rejected: it reopens a guarantee the code makes unconditionally, for a deployment shape (bridge + `-p`) that host networking already serves with zero code, and the guardrail it would need (TLS, two-key arming) does not exist yet; (b) binding `0.0.0.0` inside the container and relying on Docker's port mapping as the perimeter — rejected: the perimeter would then be Docker's iptables rules rather than the Runtime's own refusal, and a `-p 0.0.0.0:8080:8080` typo publishes the API to the internet with the bearer token as the only defence; (c) leave it unrecorded because the compose file already does the right thing — rejected: that is exactly the implicit security call #128 asked not to make.

**Consequences:** host networking couples the container to the host's port space — `8080` must be free on the host, which `install/VPS_REHEARSAL.md` already checks ("Nothing listening on `127.0.0.1:8080`"). `graphhelm doctor` (#127), when it exists, checks that a running container is in host network mode rather than asking whether the bind is loopback — the bind is always loopback. Closes #128 as the governance record it asked for.

**Relationship:** temporarily amends ADR-002's SSH usage restriction only for operator access to the Milestone 05 loopback HTTP/bearer-token MVP. This exception ends when the mTLS Runtime API is implemented and verified, or an accepted superseding ADR replaces it. It is consistent with D-002 and ADR-037's systemd path (both reach the same loopback-bound server), records the choice PR #327 shipped, and preserves the architecture's mTLS and identity requirements for remote-production operation. An SSH tunnel is not evidence of that contract; a future non-loopback bind still requires a superseding ADR under the guardrail above.

