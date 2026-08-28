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
their complete producers exist, including prompt-cache components. Every token value is limited to
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
