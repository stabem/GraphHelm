# Milestone 05b — Gateway Slice Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The Universal Model Gateway's first slice: a pure route-manifest and policy crate (`core/gateway`), plus an impure adapter crate (`adapters/model-gateway`) holding the credential broker over the sealed key provider, BYOK Anthropic/OpenAI HTTP adapters, and Claude Code / Codex native-runtime CLI adapters — surfaced through `graphhelm gateway` CLI commands, with exhausted capacity mapping to `NeedsCapacity`.

**Architecture:** Two new workspace crates. `core/gateway` is pure (no I/O, no clock, no randomness): manifest types with validation, the error taxonomy, health states, the capacity→outcome mapping, and minimal candidate filtering. `adapters/model-gateway` is impure: the credential broker reuses `EvidenceProtector<SealedKeyProvider>` (no new cryptography), the BYOK adapters speak HTTP through a `HttpTransport` trait whose production implementation is `ureq` (ADR-025), and the native-runtime adapters spawn official CLIs as subprocesses with an environment that structurally cannot contain broker secrets. The CLI gains `gateway` subcommands and a `gateway_cli` gate stage.

**Tech Stack:** Rust 1.97.1 (pinned), serde/serde_json (already in workspace), `ureq` (new, exact-pinned by ADR-025, rustls only), `graphhelm-events` (`KeyProvider`, `EvidenceProtector`, `SecretBytes`), `graphhelm-sealed-key-provider`.

**Design sources:** `docs/superpowers/specs/2026-08-13-runtime-design.md` (§5 boundary, §6.3 credentials, §7 plan 05b), `docs/models/UNIVERSAL_MODEL_GATEWAY.md` (§2 route types, §4 manifest, §7 broker, §11 usage, §12 exhausted capacity, §17 errors, §18 health, §20 acceptance). Issue: #24.

---

## Binding process rules (every task, no exceptions)

1. **The code wins over the plan.** If a signature, type, or path in this plan disagrees with the repository, the repository is right: adapt, and report the discrepancy in your task summary. Never force the plan's version over compiling reality.
2. **This plan adds NO event kinds and touches NO frozen schema.** If you find yourself editing anything under `core/schema/`, the schema catalog, baselines, or `core/protocols` wire enums (except reading them), STOP and report. The gateway slice is manifest + adapters + CLI only.
3. **Workspace clippy is the bar:** `cargo +1.97.1 clippy --workspace --all-targets --all-features --locked -- -D warnings` must be clean at every commit, not just the touched crate.
4. **Every new guard must be observed failing once.** A validation, a refusal, a redaction — sabotage it (or feed it the bad input), watch the test fail the way you claimed, restore. If you back up a file to sabotage it, restore with `cp` of the backup, never `git checkout --`.
5. **New CLI test binaries must be added to `ci/gate.ps1` by name** (the CLI stage lists suites explicitly: currently `('cli_smoke','schema_cli','event_store_cli','execution_cli','api_http')`).
6. **Secrets never in argv, never in logs, never in error text.** Credential values enter via stdin or environment variable read-then-scrub, and every error path in the broker and adapters is redaction-safe. Tests plant a sentinel value and assert formatted errors and captured output never contain it.
7. **Run the fmt gate before every commit:** `cargo +1.97.1 fmt --all`.
8. Commit after every task with the repo's convention: `type(scope): description`, body explains the why, trailer `Co-Authored-By: Claude Code <noreply@anthropic.com>`.

## What already exists (do not rebuild)

- `core/protocols::simulation::NodeOutcome` — the outcome vocabulary; `NeedsCapacity` exists (`(Running, NeedsCapacity) → WaitingCapacity` in `core/execution/src/transition.rs:90`).
- `graphhelm_events::{KeyProvider, SecretBytes}` and `EvidenceProtector<K>` (`core/events/src/evidence.rs`) — sealing/opening with XChaCha20-Poly1305 over a wrapped key. `SealedEvidence::new(...)` is public and fully reconstructible from parts (reference, scope, media_type, sensitivity, retention_class, algorithm, nonce, ciphertext, wrapped_key) — the broker persists these parts as JSON and reconstructs; it invents no cryptography.
- `graphhelm_sealed_key_provider::SealedKeyProvider` — `create`/`open` against a keyring directory; implements `KeyProvider`. The CLI wiring precedent is `apps/cli/src/commands/events/config.rs` (keyring directory + key id + passphrase env `GRAPHHELM_EVENTS_KEY`, redaction-safe `Debug`).
- The CLI's async bridge: `super::runtime()?.block_on(async move { ... })` (see `apps/cli/src/commands/events/backup.rs:32`).
- The four-key JSON envelope and `Failure` type used by every command (`apps/cli/src/commands/`), failure codes `GHCLI001`–`GHCLI008` are taken; the gateway starts at `GHCLI009`.
- Test spawn pattern: `Command::new(assert_cmd::cargo::cargo_bin!("graphhelm"))` (`apps/cli/tests/execution_cli.rs:11`); local fake HTTP servers over `std::net::TcpListener` (`apps/cli/tests/api_http.rs`).

## File map

| Path | Responsibility |
|---|---|
| `core/gateway/Cargo.toml`, `src/lib.rs` | New pure crate `graphhelm-gateway` |
| `core/gateway/src/manifest.rs` | Route manifest types + validation |
| `core/gateway/src/taxonomy.rs` | `GatewayError` taxonomy, `RouteHealth`, capacity→`NodeOutcome` mapping |
| `core/gateway/src/eligibility.rs` | Minimal candidate filtering (§8.2 subset) |
| `core/gateway/tests/manifest_contract.rs` | Manifest validation properties |
| `core/gateway/tests/capacity_mapping.rs` | Mapping totality + codomain properties |
| `core/gateway/tests/source_invariants.rs` | Purity: pinned dependency table, no I/O/clock imports |
| `adapters/model-gateway/Cargo.toml`, `src/lib.rs` | New impure crate `graphhelm-model-gateway` |
| `adapters/model-gateway/src/broker.rs` | Credential broker over `EvidenceProtector<SealedKeyProvider>` |
| `adapters/model-gateway/src/transport.rs` | `HttpTransport` trait + `UreqTransport` (ADR-025) |
| `adapters/model-gateway/src/byok.rs` | Anthropic + OpenAI adapters, usage normalization, error mapping |
| `adapters/model-gateway/src/runtime.rs` | Native-runtime CLI adapters (Claude Code, Codex), env isolation, timeout |
| `adapters/model-gateway/src/bin/fake_runtime.rs` | Test-fixture binary imitating a host CLI (documented as such) |
| `adapters/model-gateway/tests/broker.rs` | Broker durability, lease scoping, redaction |
| `adapters/model-gateway/tests/byok_adapters.rs` | Fake-server adapter tests |
| `adapters/model-gateway/tests/runtime_adapters.rs` | Fake-runtime subprocess tests |
| `apps/cli/src/commands/gateway/mod.rs` (+ files per command) | `gateway routes\|probe\|credential-set\|credential-remove` |
| `apps/cli/tests/gateway_cli.rs` | CLI suite (new gate stage) |
| `docs/reference/REFERENCE_STACK_AND_ADRS.md` | ADR-025 (outbound HTTP client) |
| `docs/milestones/runtime.md`, `CHANGELOG.md` | 05b record |
| `Cargo.toml` (workspace), `ci/gate.ps1` | Members + gate stage |

Deferred, stated (do not build): aggregator/OpenAI-compatible/local-embedded transports; router scoring (§8.3) and work-profile benchmarks (§10); broker access-audit ledger (§7.1); health probes that spend quota (§18 forbids); YAML manifests (spec examples are YAML; the on-disk manifest is JSON in 05b — revisit with Studio); session management (§13 — stateless calls only); gateway-native tool calls (§14 — 05c).

---

### Task 1: `core/gateway` crate — manifest types and validation

**Files:**
- Create: `core/gateway/Cargo.toml`, `core/gateway/src/lib.rs`, `core/gateway/src/manifest.rs`
- Create: `core/gateway/tests/manifest_contract.rs`
- Modify: `Cargo.toml` (workspace members — add `"core/gateway"` after `"core/execution"`)

- [ ] **Step 1: Crate skeleton.** `core/gateway/Cargo.toml` mirrors `core/execution/Cargo.toml`'s shape (workspace package/lints inheritance). Dependencies: `graphhelm-protocols` (path), `serde` (workspace), `serde_json` (workspace), and whatever error-derive the sibling core crates use — inspect `core/execution/Cargo.toml` and match exactly; add nothing else.

- [ ] **Step 2: Write the failing contract tests** in `core/gateway/tests/manifest_contract.rs`:

```rust
use graphhelm_gateway::manifest::{
    Authentication, BillingMode, ManifestError, RouteManifest, Transport,
};

fn valid_manifest_json() -> serde_json::Value {
    serde_json::json!({
        "manifestVersion": 1,
        "routes": [
            {
                "id": "anthropic_byok",
                "provider": "anthropic",
                "transport": "direct_api",
                "authentication": "api_key",
                "billingMode": "per_token",
                "baseUrl": "https://api.anthropic.com",
                "model": "claude-sonnet-5",
                "credentialRef": "secret_anthropic_primary",
                "profiles": ["critical_reasoning"],
                "enabled": true
            },
            {
                "id": "claude_subscription",
                "provider": "anthropic",
                "transport": "native_runtime",
                "runtime": "claude_code",
                "authentication": "account_subscription",
                "billingMode": "subscription_quota",
                "command": { "program": "claude", "args": ["-p", "--output-format", "json"] },
                "profiles": ["software_execution"],
                "enabled": true
            }
        ]
    })
}

#[test]
fn a_valid_manifest_parses_and_reports_both_routes() {
    let manifest = RouteManifest::from_json(&valid_manifest_json().to_string()).unwrap();
    assert_eq!(manifest.routes().len(), 2);
    assert_eq!(manifest.routes()[0].billing_mode(), BillingMode::PerToken);
    assert_eq!(manifest.routes()[1].billing_mode(), BillingMode::SubscriptionQuota);
}

#[test]
fn billing_and_authentication_must_agree_with_the_transport() {
    // §20: BYOK and subscription are DISTINCT billing modes. A direct_api route claiming
    // subscription_quota, or a native_runtime route claiming per_token, is a category error.
    let mut bad = valid_manifest_json();
    bad["routes"][0]["billingMode"] = "subscription_quota".into();
    let err = RouteManifest::from_json(&bad.to_string()).unwrap_err();
    assert!(matches!(err, ManifestError::BillingTransportMismatch { .. }));
}

#[test]
fn direct_api_requires_credential_ref_and_native_runtime_forbids_it() {
    let mut missing = valid_manifest_json();
    missing["routes"][0].as_object_mut().unwrap().remove("credentialRef");
    assert!(RouteManifest::from_json(&missing.to_string()).is_err());

    let mut leaky = valid_manifest_json();
    leaky["routes"][1]["credentialRef"] = "secret_smuggled".into();
    // credential_export: false is structural — a native runtime owns its own auth and the
    // manifest cannot route a broker secret into it.
    assert!(RouteManifest::from_json(&leaky.to_string()).is_err());
}

#[test]
fn duplicate_route_ids_unknown_fields_and_oversize_are_refused() {
    let mut dup = valid_manifest_json();
    dup["routes"][1]["id"] = "anthropic_byok".into();
    assert!(matches!(
        RouteManifest::from_json(&dup.to_string()).unwrap_err(),
        ManifestError::DuplicateRouteId { .. }
    ));

    let mut unknown = valid_manifest_json();
    unknown["routes"][0]["extra"] = true.into();
    assert!(RouteManifest::from_json(&unknown.to_string()).is_err());

    let oversize = "x".repeat(graphhelm_gateway::manifest::MAX_MANIFEST_BYTES + 1);
    assert!(matches!(
        RouteManifest::from_json(&oversize).unwrap_err(),
        ManifestError::Oversize { .. }
    ));
}

#[test]
fn a_non_loopback_http_base_url_is_refused() {
    // TLS is the default posture: http:// is allowed only for loopback (local fakes and
    // OpenAI-compatible local endpoints later); a cleartext remote URL is a misconfiguration.
    let mut bad = valid_manifest_json();
    bad["routes"][0]["baseUrl"] = "http://api.anthropic.com".into();
    assert!(matches!(
        RouteManifest::from_json(&bad.to_string()).unwrap_err(),
        ManifestError::CleartextRemoteUrl { .. }
    ));
    let mut ok = valid_manifest_json();
    ok["routes"][0]["baseUrl"] = "http://127.0.0.1:9999".into();
    assert!(RouteManifest::from_json(&ok.to_string()).is_ok());
}
```

- [ ] **Step 3: Run to see them fail** (crate doesn't compile yet): `cargo +1.97.1 test -p graphhelm-gateway --locked` → compile error.

- [ ] **Step 4: Implement `manifest.rs`.** Shape (adapt names only if a collision forces it):

```rust
pub const MAX_MANIFEST_BYTES: usize = 256 * 1024;
pub const MAX_ROUTES: usize = 64;

#[derive(Clone, Copy, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Transport { DirectApi, NativeRuntime }

#[derive(Clone, Copy, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Authentication { ApiKey, AccountSubscription }

#[derive(Clone, Copy, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BillingMode { PerToken, SubscriptionQuota }

#[derive(Clone, Copy, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeKind { ClaudeCode, Codex }

#[derive(Clone, Copy, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkProfile {
    FastClassification, CheapExtraction, BalancedReasoning, CriticalReasoning,
    LongContextSynthesis, SoftwareExecution, VisionReasoning, CreativeGeneration,
    SourceGroundedResearch, LocalPrivate, HighReliabilityStructuredOutput,
}
```

`RouteManifest::from_json(&str) -> Result<Self, ManifestError>` enforces, in order: byte bound → serde with `deny_unknown_fields` → per-route structural rules → duplicate id scan. Structural rules: `DirectApi` requires `authentication: ApiKey`, `billing_mode: PerToken`, `base_url`, `model`, `credential_ref`, and no `runtime`/`command`; `NativeRuntime` requires `authentication: AccountSubscription`, `billing_mode: SubscriptionQuota`, `runtime`, `command` (non-empty program), and **no** `credential_ref`/`base_url`. Route ids: 1–64 chars of `[a-z0-9_]`. URL rule: `https://` always allowed; `http://` only when host parses as a loopback IP or is `localhost`. Keep URL parsing dependency-free: split scheme, take the authority up to the first `/`, strip port, test `localhost` / `Ipv4Addr::is_loopback` / bracketed `::1`. `ManifestError` is a field-carrying enum whose `Display` names the route id and rule, never file contents.

- [ ] **Step 5: Run to green:** `cargo +1.97.1 test -p graphhelm-gateway --locked` → all pass. Then the workspace bar: fmt + workspace clippy.

- [ ] **Step 6: Sabotage one guard** (rule 4): comment out the duplicate-id scan, watch `duplicate_route_ids_...` fail, restore.

- [ ] **Step 7: Commit** `feat(gateway): route manifest with structural validation`.

---

### Task 2: taxonomy, health, capacity mapping, eligibility — plus purity invariants

**Files:**
- Create: `core/gateway/src/taxonomy.rs`, `core/gateway/src/eligibility.rs`
- Create: `core/gateway/tests/capacity_mapping.rs`, `core/gateway/tests/source_invariants.rs`

- [ ] **Step 1: Failing tests** in `core/gateway/tests/capacity_mapping.rs`:

```rust
use graphhelm_gateway::taxonomy::{GatewayError, RouteHealth, outcome_for_error};
use graphhelm_protocols::simulation::NodeOutcome;

const EVERY_ERROR: &[GatewayError] = &[
    GatewayError::AuthRequired, GatewayError::AuthRevoked, GatewayError::QuotaExhausted,
    GatewayError::RateLimited, GatewayError::ProviderUnavailable, GatewayError::ModelRemoved,
    GatewayError::ContextTooLarge, GatewayError::MalformedOutput, GatewayError::ToolDenied,
    GatewayError::RuntimeCrashed, GatewayError::UnsupportedCapability, GatewayError::PolicyDenied,
    GatewayError::Cancelled, GatewayError::Timeout,
];

#[test]
fn the_mapping_is_total_and_lands_only_in_failure_shaped_outcomes() {
    // §12: capacity exhaustion pauses; nothing in the taxonomy may ever map to a
    // success-shaped or owner-decision-shaped outcome.
    for error in EVERY_ERROR {
        let outcome = outcome_for_error(*error);
        assert!(
            matches!(
                outcome,
                NodeOutcome::NeedsCapacity
                    | NodeOutcome::RetryableFailure
                    | NodeOutcome::TerminalFailure
                    | NodeOutcome::Cancelled
            ),
            "{error:?} escaped the failure codomain as {outcome:?}"
        );
    }
}

#[test]
fn capacity_class_errors_park_the_node_and_only_them() {
    use GatewayError as E;
    use NodeOutcome as O;
    // §12 step 1 names "limit/throttle/auth failure" as the pause triggers; wait, no
    // automatic paid fallback.
    for e in [E::QuotaExhausted, E::RateLimited, E::AuthRequired, E::AuthRevoked] {
        assert_eq!(outcome_for_error(e), O::NeedsCapacity);
    }
    for e in [E::ProviderUnavailable, E::Timeout, E::RuntimeCrashed, E::MalformedOutput] {
        assert_eq!(outcome_for_error(e), O::RetryableFailure);
    }
    for e in [E::ContextTooLarge, E::ModelRemoved, E::UnsupportedCapability, E::PolicyDenied, E::ToolDenied] {
        assert_eq!(outcome_for_error(e), O::TerminalFailure);
    }
    assert_eq!(outcome_for_error(E::Cancelled), O::Cancelled);
}

#[test]
fn health_for_error_updates_the_route_state_per_class() {
    use graphhelm_gateway::taxonomy::health_for_error;
    assert_eq!(health_for_error(GatewayError::QuotaExhausted), Some(RouteHealth::WaitingReset));
    assert_eq!(health_for_error(GatewayError::AuthRequired), Some(RouteHealth::AuthRequired));
    assert_eq!(health_for_error(GatewayError::AuthRevoked), Some(RouteHealth::AuthRequired));
    assert_eq!(health_for_error(GatewayError::ProviderUnavailable), Some(RouteHealth::Degraded));
    // A per-call condition says nothing about the route.
    assert_eq!(health_for_error(GatewayError::ContextTooLarge), None);
    assert_eq!(health_for_error(GatewayError::Cancelled), None);
}
```

- [ ] **Step 2: Eligibility tests** (same file or `eligibility.rs`-adjacent test file):

```rust
#[test]
fn eligibility_filters_disabled_unhealthy_wrong_profile_and_wrong_billing() {
    // Build a manifest with four routes varying one axis each; requirements ask for
    // {profile: CriticalReasoning, subscription_only: false}.
    // Expect: only the enabled+available+profile-matching route survives.
    // Health comes from a caller-supplied map (id → RouteHealth) — the pure crate holds
    // no registry; Available and Degraded pass, everything else is excluded (§8.2).
}
```

Write it concretely against `eligible_routes(&manifest, &health_map, &requirements) -> Vec<&ModelRoute>` with `Requirements { profile: WorkProfile, subscription_only: bool }`. `subscription_only: true` excludes `PerToken` routes (user control §19); scoring (§8.3) is deferred — the function documents that ordering is manifest order.

- [ ] **Step 3: Purity invariants** in `core/gateway/tests/source_invariants.rs` — copy the pattern from `core/execution/tests/source_invariants.rs` verbatim and adjust: the dependency table for `core/gateway/Cargo.toml` is exactly `["graphhelm-protocols", "serde", "serde_json"]` plus the error-derive crate if Task 1 matched one from siblings; `no_other_dependency_table_exists`; and a source scan asserting no `std::fs`, `std::net`, `std::time`, `std::process`, `rand` tokens anywhere under `core/gateway/src/`.

- [ ] **Step 4: Implement** `taxonomy.rs` (`GatewayError` as a fieldless `Copy` enum of the fourteen §17 kinds, `RouteHealth { Available, Degraded, WaitingReset, AuthRequired, Unavailable, Disabled }`, `outcome_for_error` as one exhaustive `match` — no wildcard arm, so a future taxonomy addition breaks compilation here on purpose; say so in a comment) and `eligibility.rs`. Run to green, fmt, workspace clippy.

- [ ] **Step 5: Sabotage:** make `outcome_for_error` return `Succeeded` for `RateLimited`; watch both mapping tests fail; restore.

- [ ] **Step 6: Commit** `feat(gateway): error taxonomy, health states, capacity mapping, eligibility`.

---

### Task 3: the credential broker over the sealed provider

**Files:**
- Create: `adapters/model-gateway/Cargo.toml`, `src/lib.rs`, `src/broker.rs`
- Create: `adapters/model-gateway/tests/broker.rs`
- Modify: `Cargo.toml` (workspace members — add `"adapters/model-gateway"`)

Dependencies for the new crate: `graphhelm-gateway`, `graphhelm-events`, `graphhelm-sealed-key-provider`, `serde`, `serde_json`, plus the async runtime the CLI already uses for `block_on` if the broker itself needs one — **prefer** exposing async fns and letting callers bridge, exactly as `EvidenceProtector` does. Inspect `core/events/src/evidence.rs` (`EvidenceInput`, `EvidenceProtector::seal/open` signatures) and `apps/cli/src/commands/events/config.rs` (provider construction) before writing a line — the code wins.

- [ ] **Step 1: Failing tests** in `adapters/model-gateway/tests/broker.rs`:

```rust
// All tests build the broker over a throwaway keyring directory (tempdir) with a fixed
// test passphrase, mirroring how sealed-provider tests bootstrap.

const SENTINEL: &str = "sk-ant-SENTINEL-0123456789abcdef";

#[test]
fn store_lease_roundtrip_is_durable_across_reopen() {
    // store(reference{id, provider, usable_by:["anthropic_byok"]}, SENTINEL bytes)
    // drop broker; reopen over the same directories; lease("secret_anthropic_primary",
    // "anthropic_byok") returns bytes == SENTINEL.
}

#[test]
fn lease_is_scoped_to_usable_by_routes() {
    // lease with route "openai_byok" (not in usable_by) → BrokerError::NotUsableByRoute,
    // and the error's Display names the route id but NEVER the credential value.
}

#[test]
fn a_revoked_credential_cannot_be_leased_and_survives_reopen() {
    // revoke(id) → lease fails with BrokerError::Revoked; reopen → still refuses.
}

#[test]
fn broker_errors_and_listings_never_carry_the_value() {
    // list() returns ids + providers only. Force every error variant this test can reach
    // (missing id, wrong route, revoked, tampered store file) and assert
    // format!("{e}") and format!("{e:?}") never contain SENTINEL or the passphrase.
}

#[test]
fn a_tampered_store_file_fails_closed() {
    // Flip one byte of the persisted ciphertext on disk; lease → BrokerError (decryption
    // authentication failure), never partial bytes.
}
```

- [ ] **Step 2: Implement `broker.rs`.**

```rust
pub struct SecretReference {
    pub id: String,          // 1..=64 of [a-z0-9_.-]
    pub provider: String,    // same charset
    pub usable_by: Vec<String>, // route ids; non-empty
}

pub struct CredentialBroker { /* directory handle, EvidenceProtector<SealedKeyProvider>, index */ }

impl CredentialBroker {
    pub async fn create(broker_dir: &Path, keyring_dir: &Path, key_id: &str, passphrase: SecretBytes) -> Result<Self, BrokerError>;
    pub async fn open(...) -> Result<Self, BrokerError>;
    pub async fn store(&mut self, reference: SecretReference, value: SecretBytes) -> Result<(), BrokerError>;
    pub async fn lease(&self, id: &str, route_id: &str) -> Result<SecretBytes, BrokerError>;
    pub async fn revoke(&mut self, id: &str) -> Result<(), BrokerError>;
    pub fn list(&self) -> Vec<SecretReferenceSummary>; // ids + providers + revoked flag, no values
}
```

Persistence: one JSON index file `credentials.json` inside `broker_dir`, atomic write (write `credentials.json.tmp`, then rename over). Each entry: the reference fields, `revoked: bool`, and the sealed parts of a `SealedEvidence` (reference id, scope, media type `application/octet-stream`, sensitivity, retention class, nonce/ciphertext base64 or hex, and the `WrappedKey` parts via `into_parts()`/reconstruction) — reconstructed with `SealedEvidence::new(...)` on read. Scope: a fixed gateway `RepositoryScope` (inspect its constructor; use stable ids like `gateway`/`credentials`). Lease = `EvidenceProtector::open` → `SecretBytes` (already zeroizing). §7.3: the value exists only in the returned `SecretBytes` for the caller's shortest use; the broker never caches plaintext.

- [ ] **Step 3: Run to green.** fmt + workspace clippy.
- [ ] **Step 4: Sabotage:** make `lease` skip the `usable_by` check; watch the scoping test fail; restore. Then plant `SENTINEL` into one error message; watch the redaction test fail; restore.
- [ ] **Step 5: Commit** `feat(gateway): credential broker over the sealed key provider`.

---

### Task 4: ADR-025, `HttpTransport`, and the BYOK Anthropic/OpenAI adapters

**Files:**
- Create: `adapters/model-gateway/src/transport.rs`, `src/byok.rs`
- Create: `adapters/model-gateway/tests/byok_adapters.rs`
- Modify: `adapters/model-gateway/Cargo.toml` (+`ureq`), workspace `Cargo.toml` if the dependency is workspace-managed (match how `axum` was pinned)
- Modify: `docs/reference/REFERENCE_STACK_AND_ADRS.md` (ADR-025)

- [ ] **Step 1: Pin `ureq` the ADR-024 way.** `cargo add ureq --dry-run -p graphhelm-model-gateway` to learn the newest version; pin exact `=X.Y.Z`; curate features: TLS must be rustls (no native-tls, no OpenSSL) and JSON helpers may be taken if the default set is clean — inspect the resolved feature graph (`cargo tree -p ureq -e features`) and write what you found into ADR-025. ADR-025 text: context (BYOK adapters need outbound HTTPS; the runtime API server chose axum in ADR-024 which brings no client), decision (`ureq`, exact pin, rustls-only, synchronous — matching the CLI's synchronous command layer; the async driver of 05d bridges with `spawn_blocking` when it arrives), consequences (one TLS stack in-tree via rustls; local tests run plain HTTP against loopback fakes, which the manifest's cleartext-loopback-only rule permits).

- [ ] **Step 2: `transport.rs`:**

```rust
pub struct TransportRequest {
    pub method: &'static str,       // "POST"
    pub url: String,
    pub headers: Vec<(String, String)>, // values may hold secrets: no Debug derive
    pub body: Vec<u8>,
    pub timeout: Duration,
}
pub struct TransportResponse { pub status: u16, pub body: Vec<u8> }

pub trait HttpTransport: Send + Sync {
    fn execute(&self, request: TransportRequest) -> Result<TransportResponse, TransportError>;
}
pub struct UreqTransport; // production impl; TransportError { Io, Timeout } — non-2xx is a
                          // RESPONSE, not a transport error (adapters map statuses).
```

Manual `Debug` impls redact headers. Verify how ureq surfaces non-2xx (its `Error::Status` behavior changed across majors — in ureq 3 configure the agent with `http_status_as_error(false)` or map the variant; the code wins).

- [ ] **Step 3: Failing adapter tests** in `tests/byok_adapters.rs`, following the `api_http.rs` fake-server pattern (a `TcpListener` on `127.0.0.1:0`, one thread per test, canned HTTP/1.1 responses; requests captured for assertion). Cover, for **Anthropic** (`POST {base}/v1/messages`, headers `x-api-key`, `anthropic-version: 2023-06-01`, body `{model, max_tokens, messages}`):

```text
success        → 200 {"content":[{"type":"text","text":"hello"}],
                      "usage":{"input_tokens":12,"output_tokens":5}, ...}
                 asserts: ModelReply.text == "hello", usage == Some(12)/Some(5),
                          request carried x-api-key: SENTINEL and the version header
rate limit     → 429 {"type":"error","error":{"type":"rate_limit_error",...}}   → GatewayError::RateLimited
auth           → 401 {"type":"error","error":{"type":"authentication_error"}}   → GatewayError::AuthRequired
overloaded     → 529 {"type":"error","error":{"type":"overloaded_error"}}       → GatewayError::ProviderUnavailable
garbage 200    → 200 "not json"                                                 → GatewayError::MalformedOutput
absent usage   → 200 without "usage"                                            → usage fields None (§11: never invent)
```

and for **OpenAI** (`POST {base}/v1/chat/completions`, header `Authorization: Bearer <key>`):

```text
success        → 200 {"choices":[{"message":{"role":"assistant","content":"hello"},...}],
                      "usage":{"prompt_tokens":12,"completion_tokens":5}}
quota vs rate  → 429 {"error":{"type":"insufficient_quota","code":"insufficient_quota"}} → QuotaExhausted
               → 429 {"error":{"code":"rate_limit_exceeded"}}                            → RateLimited
auth           → 401                                                                     → AuthRequired
```

Plus one shared test: `the_api_key_never_appears_in_errors_or_debug` — run the garbage-200 case with a SENTINEL key and assert `format!("{e}")`/`{e:?}` of every returned error and of the request's `Debug` never contain SENTINEL.

- [ ] **Step 4: Implement `byok.rs`.** `ByokAdapter::new(route: &ModelRoute, transport: Arc<dyn HttpTransport>)`; `call(&self, key: &SecretBytes, request: &ModelCall) -> Result<ModelReply, GatewayError>` where `ModelCall { prompt: String, max_tokens: u32 }` and `ModelReply { text: String, usage: Usage }`, `Usage { input_tokens: Option<u64>, output_tokens: Option<u64> }` (types live in `core/gateway` so 05d consumes them without touching the adapter crate — put them in a new `core/gateway/src/call.rs` with plain serde derives; extend the purity test's file list). Provider dispatch on `route.provider` (`"anthropic"` / `"openai"`); unknown provider on a `DirectApi` route is a `ManifestError` back in Task 1 — add that rule and a test if you reach here and it's missing (report it). Status mapping lives in one function per provider with the JSON error-body sniffing shown above. Fixed rules: recognized statuses map per the tables; `403` → `PolicyDenied`; any unrecognized ≥500 → `ProviderUnavailable`; any other unmapped status or unparseable body → `MalformedOutput`, with a code comment stating the reason: an unclassifiable reply must not park capacity (§12 is deliberate about what pauses), and `MalformedOutput → RetryableFailure` in the Task 2 table makes a mystery status a retryable defect rather than an invented meaning.

- [ ] **Step 5: Run to green, fmt, workspace clippy.**
- [ ] **Step 6: Sabotage:** invert the OpenAI quota-vs-rate sniff; watch that test fail; restore.
- [ ] **Step 7: Commit** `feat(gateway): BYOK Anthropic and OpenAI adapters over pinned ureq (ADR-025)`.

---

### Task 5: native-runtime adapters — Claude Code and Codex CLIs

**Files:**
- Create: `adapters/model-gateway/src/runtime.rs`, `src/bin/fake_runtime.rs`
- Create: `adapters/model-gateway/tests/runtime_adapters.rs`

- [ ] **Step 1: The fake runtime.** `src/bin/fake_runtime.rs` — a tiny binary, doc-commented as a test fixture (it ships with the crate because Cargo builds it for integration tests via `CARGO_BIN_EXE_fake_runtime`). Behavior driven by `FAKE_RUNTIME_MODE`:

```text
ok             → reads stdin to EOF, prints the configured happy JSON for FAKE_RUNTIME_SHAPE
                 (claude_code or codex), exit 0
quota          → prints a quota-exhausted shape, exit 1
crash          → prints half a JSON object, exit 137
hang           → sleeps 3600s (the adapter must kill it)
env-dump       → prints every environment variable as NAME=VALUE lines, exit 0
```

Happy shapes (these are this repo's documented understanding of the host CLIs' JSON output; 05e re-verifies against the live hosts and this is recorded as an honest limit in Task 7):

```json
claude_code: {"type":"result","subtype":"success","result":"hello from claude",
              "usage":{"input_tokens":12,"output_tokens":5}}
codex (JSONL, last line wins):
             {"msg":{"type":"agent_message","message":"hello from codex"}}
```

- [ ] **Step 2: Failing tests** in `tests/runtime_adapters.rs`:

```rust
// Adapter under test: RuntimeAdapter::new(route) where route.command.program is the
// fake_runtime path (env!("CARGO_BIN_EXE_fake_runtime")) and mode comes via the
// adapter's extra_env parameter — the ONLY env the adapter adds beyond the allowlist.

#[test] fn a_claude_shaped_reply_parses_text_and_usage() { /* mode=ok shape=claude_code */ }
#[test] fn a_codex_jsonl_reply_takes_the_last_agent_message() { /* mode=ok shape=codex */ }
#[test] fn quota_exhaustion_maps_to_needs_capacity() {
    // mode=quota → GatewayError::QuotaExhausted; outcome_for_error == NeedsCapacity
}
#[test] fn a_crashed_runtime_is_runtime_crashed_not_malformed() { /* exit 137 → RuntimeCrashed */ }
#[test] fn a_hung_runtime_is_killed_within_the_deadline() {
    // mode=hang, timeout 2s → GatewayError::Timeout in < 30s, child actually dead.
}
#[test] fn the_child_environment_is_an_allowlist_and_never_carries_broker_material() {
    // mode=env-dump with GRAPHHELM_GATEWAY_KEY and A_SENTINEL_SECRET set in the parent:
    // captured output contains PATH but neither of those names, nor the sentinel value.
}
#[test] fn the_prompt_travels_via_stdin_never_argv() {
    // fake_runtime mode=ok includes the first stdin line inside "result"; the test sends a
    // distinctive prompt and asserts it round-trips through the reply while the spawned
    // argv (which the fake also prints on stderr) never contains it.
}
```

- [ ] **Step 3: Implement `runtime.rs`.** `RuntimeAdapter::call(&self, request: &ModelCall) -> Result<ModelReply, GatewayError>`: spawn `route.command.program` with `route.command.args`, `Stdio::piped()` everywhere, **`env_clear()` then an explicit allowlist** copied from the parent: `PATH`, `SYSTEMROOT`, `SYSTEMDRIVE`, `COMSPEC`, `WINDIR`, `TEMP`, `TMP`, `USERPROFILE`, `HOME`, `APPDATA`, `LOCALAPPDATA`, `PROGRAMDATA`, `XDG_CONFIG_HOME`, `XDG_DATA_HOME` (the official CLIs need their own config dirs for their own auth — that is §6.3's separation: their credential, their store, never ours). Write prompt to stdin, drop stdin, then poll `try_wait` in a 50ms loop against the deadline (`route.timeout_seconds` manifest field — add it in Task 1 if missing with default 300; report the addition); on deadline: `kill()`, `wait()`, return `Timeout`. Parse stdout by `RuntimeKind`: `ClaudeCode` full-JSON (`result` string, optional `usage`), `Codex` JSONL scan keeping the last `agent_message`. Exit-code ≠ 0 with a recognizable quota shape (`"quota"`/`"rate limit"` markers in the parsed error JSON — define one marker list constant, comment it as heuristic) → `QuotaExhausted`; other nonzero exits → `RuntimeCrashed`; unparseable stdout on exit 0 → `MalformedOutput`. Usage absent (codex shape) → `None` — never invented (§11.2).
- [ ] **Step 4: Run to green** (these tests exercise real subprocesses — keep each under ~5s except the hang test's own deadline), fmt, workspace clippy.
- [ ] **Step 5: Sabotage:** remove `env_clear()`; watch the env allowlist test fail; restore.
- [ ] **Step 6: Commit** `feat(gateway): native runtime adapters with env isolation and deadlines`.

---

### Task 6: the CLI surface and the gate stage

**Files:**
- Create: `apps/cli/src/commands/gateway/mod.rs` (+ `routes.rs`, `probe.rs`, `credential.rs`)
- Modify: `apps/cli/src/commands/mod.rs` (register), `apps/cli/Cargo.toml` (add `graphhelm-gateway`, `graphhelm-model-gateway`)
- Create: `apps/cli/tests/gateway_cli.rs`
- Modify: `ci/gate.ps1` — CLI suites become `('cli_smoke','schema_cli','event_store_cli','execution_cli','api_http','gateway_cli')` (rule 5)

Commands (JSON four-key envelope like every sibling; new codes `GHCLI009_GATEWAY_INVALID`, `GHCLI010_GATEWAY_CREDENTIAL`, `GHCLI011_GATEWAY_PROBE`):

- `graphhelm gateway routes --manifest <file>` → validated route list: id, provider, transport, billing mode, profiles, enabled. Manifest errors → `GHCLI009` with the rule name, never file contents.
- `graphhelm gateway probe --manifest <file> --route <id> [--broker <dir> --keyring <dir> --key-id <id>]` → **quota-free** probe (§18): for `DirectApi`, the credential resolves from the broker (lease succeeds) — no network call; for `NativeRuntime`, the program spawns with `--version` under the Task 5 allowlist and exits 0 within 10s. Reports `{route, checks: [{name, ok}], health}` where health is `available` or `auth_required`/`unavailable` per failed check. Passphrase via `GRAPHHELM_GATEWAY_KEY` env, mirroring the events-config precedent.
- `graphhelm gateway credential set --broker <dir> --keyring <dir> --key-id <id> --ref <ref-id> --provider <p> --usable-by <route,route>` — value read **from stdin** (one trimmed line), never argv (rule 6). Prints the summary (id, provider, routes), never the value.
- `graphhelm gateway credential remove ... --ref <ref-id>` → revoke.

- [ ] **Step 1: Failing CLI tests** in `apps/cli/tests/gateway_cli.rs` (spawn pattern from `execution_cli.rs`): happy `routes`; invalid manifest → exit nonzero + `GHCLI009` + stderr/stdout free of the manifest's file contents; `credential set` via stdin then `probe` green for the BYOK route; `probe` on a native route whose `command.program` is the `graphhelm` binary itself (`assert_cmd::cargo::cargo_bin!("graphhelm")` — `CARGO_BIN_EXE_fake_runtime` is not visible across crates, and the probe only needs a program whose real `--version` exits 0). A `credential set` value must not appear in any output (SENTINEL assert). A revoked ref makes `probe` report `auth_required`.
- [ ] **Step 2: Implement**, run suite to green: `cargo +1.97.1 test -p graphhelm-cli --test gateway_cli --locked`.
- [ ] **Step 3: Add the gate stage** (rule 5) and run the CLI stage block locally to prove the stage name resolves.
- [ ] **Step 4: Sabotage:** point the suite list at a misspelled name and watch the gate stage fail to find it (proves the stage can go red), restore. fmt + workspace clippy.
- [ ] **Step 5: Commit** `feat(cli): gateway routes, probe, and credential commands with gate stage`.

---

### Task 7: documentation and the full gate

**Files:**
- Modify: `docs/milestones/runtime.md` (05b section), `CHANGELOG.md`, `docs/INDEX.md`/`DOCUMENTATION_MANIFEST.md` if they enumerate crates or ADRs (check), `docs/superpowers/specs/2026-08-13-runtime-design.md` status line for 05b.

- [ ] **Step 1: Write the 05b section of `docs/milestones/runtime.md` from the code as built** — not from this plan. Cover: the two crates and the purity boundary; billing-mode/transport agreement as a structural rule; the capacity table (which errors park, which retry, which are terminal) as actually implemented; the broker's reuse of `EvidenceProtector` (no new cryptography) and its lease scoping; ADR-025; the env allowlist and stdin-prompt rules; the quota-free probe. **Honest limits section** must name: JSON manifests (spec shows YAML), deferred scoring/audit/aggregator transports/session management, the host-CLI JSON shapes being fixtures re-verified live in 05e, and the quota-detection marker heuristic for runtime CLIs.
- [ ] **Step 2: Re-read the whole edited file top to bottom** (rule from five milestones of my own inversions): every claim must match a test or a line of code you can name.
- [ ] **Step 3: CHANGELOG entry** (mirror the 04f/05a entry style).
- [ ] **Step 4: Full gate:** `GRAPHHELM_PG_BIN` set, `./ci/gate.ps1` → GREEN, both PostgreSQL passes included. A red on the tracked flakes (#19) is re-run once with the flake noted; any other red is a defect to fix before closing.
- [ ] **Step 5: Commit** `docs(gateway): record milestone 05b as built`.

---

## Self-review notes (already applied)

- Spec coverage against the 05b bullet of `runtime-design.md` §7: manifest ✔ (T1), BYOK Anthropic/OpenAI ✔ (T4), Claude Code + Codex CLI adapters ✔ (T5), credential broker over sealed provider ✔ (T3), `NeedsCapacity` mapping ✔ (T2, exercised end-to-end in T5's quota test), usage normalization minimal ✔ (T4/T5 `Usage` with never-invent). §20 acceptance rows touched here: distinct billing modes (T1), no web password/cookie (nothing collects one; probe is quota-free), credentials never in the execution sandbox (T5 env allowlist — the sandbox itself is 05c, the allowlist is its precondition), no fixed brand rules (eligibility filters by capability only).
- Deliberate scope cuts are listed in the file map's "Deferred, stated" paragraph and must land in the milestone doc's honest-limits section.
- Type-consistency: `ModelCall`/`ModelReply`/`Usage` defined once in `core/gateway/src/call.rs` (T4 step 4) and consumed by both adapter families; `GatewayError` defined once in T2 and used by T4/T5/T6.
