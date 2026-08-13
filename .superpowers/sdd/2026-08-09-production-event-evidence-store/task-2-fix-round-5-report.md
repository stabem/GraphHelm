# Task 2 fix round 5 report

## Status

DONE — the five final Important findings are corrected within the Task 2 schema,
catalog, and contract-test scope. No Task 3+ production Rust source changed, the
public fixture inventory remains frozen, and the deferred nonce Minor remains
untouched.

## Review verification and RED evidence

Each finding received a focused behavioral test before its schema fix. The
observed REDs on base `94f0bcf61d06f4aca0d848282c81e3064d562a95` were:

- the deterministic safe-key generator found that `p.rompt` was accepted;
- a persisted GraphVersion accepted `prompt` under
  `/version/graph/spec/nodes/start`;
- the exact current `GraphVersion::publish` producer plus canonicalizer rejected
  safe operational metadata named `retryPolicy`;
- the audit-complete `evidence_erasure_requested` payload was rejected by the
  partial release schema;
- an embedded PolicyWaiver with `acknowledgedRisks: []` was accepted.

The focused RED commands were the individual tests named below:

```powershell
cargo +1.97.1 test -p graphhelm-schema-evolution --test conformance safe_keys_reject_every_separator_insertion_for_each_forbidden_family --locked
cargo +1.97.1 test -p graphhelm-schema-evolution --test conformance graph_version_record_is_event_safe_for_graph_and_semantic_node_extensions --locked
cargo +1.97.1 test -p graphhelm-graph --test canonical_hash current_graph_version_producer_validates_safe_operational_metadata_without_renaming --locked
cargo +1.97.1 test -p graphhelm-schema-evolution --test conformance erasure_events_are_audit_complete_closed_and_bounded --locked
cargo +1.97.1 test -p graphhelm-schema-evolution --test conformance embedded_policy_waiver_preserves_risk_and_timestamp_boundaries --locked
```

## Contract decisions and GREEN changes

- Safe-key classification is equivalent to lowercase followed by removal of
  `.`, `_`, and `-` before matching a forbidden family. The deterministic test
  covers every interior insertion position, each of the three separators,
  repeated separators, all 16 forbidden families, mixed case, root `x-*`
  extensions, labels, and nested objects.
- Persisted GraphVersion records use local bounded event-safe Graph, Node, Edge,
  Agent, completion, policy, and semantic projections. The event schema no
  longer references permissive `graph.schema.json` or `node.schema.json`, and
  forward-compatible arbitrary surfaces share the safe-key and bounded-value
  grammar.
- Known Serde fields remain explicit so every current canonical example
  validates. Safe non-reserved operational metadata such as `retryPolicy`
  round-trips in both the full graph and canonical semantic metadata without an
  `x-` rename. A producer-real test publishes and validates all three official
  graph examples.
- Erasure requested receipts require operation, evidence, key-handle,
  policy/version, authority/reason, prior state, and pending state. Completion
  receipts repeat that correlation and require ciphertext SHA-256, authenticated
  provider receipt ID, provider epoch, pending prior state, and erased final
  state. Envelope scope, occurrence time, actor, sensitivity, and idempotency
  remain authoritative. Payloads are closed and contain only bounded IDs,
  versions, enums, integers, and hashes.
- Embedded PolicyWaiver now requires 1..64 acknowledged risks with each risk
  bounded to 1..512 characters. `expiresAt` remains nullable for exact current
  Serde output and applies the same 20..35 date-time bound when present.
- Safe numeric metadata accepts bounded JSON numbers because real operational
  metadata is not restricted to integers. Nested values remain depth-, item-,
  property-, key-, and string-bounded.

Focused GREEN results were `conformance 32/32`, `canonical_hash 8/8`,
`catalog_integrity 30/30`, and `schema_cli 27/27`. The producer-real test covers
`software-feature.yaml`, `manual-override-deploy.yaml`, and
`research-to-publish.yaml`.

Root and `schemas/releases/1.1.0` event schemas are byte-identical. Both catalogs
use the canonical schema digest
`sha256:e0a66ace8f6ee018270ee394b5911064ba0a40065513cf1e4cfd34a44dab4b25`.

## Final verification

All required commands exited 0:

```powershell
cargo +1.97.1 fmt --all -- --check
cargo +1.97.1 clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo +1.97.1 test --workspace --all-features --locked
cargo +1.97.1 test -p graphhelm-cli --test cli_smoke --locked
cargo +1.97.1 test -p graphhelm-schema-evolution --test conformance --locked
cargo +1.97.1 test -p graphhelm-schema-evolution --test catalog_integrity --locked
cargo +1.97.1 test -p graphhelm-cli --test schema_cli --locked
cargo +1.97.1 run --locked -p graphhelm-cli -- schema catalog --catalog schemas/catalog.json
cargo +1.97.1 run --locked -p graphhelm-cli -- schema check --baseline schemas/releases/1.0.0/catalog.json --candidate schemas/catalog.json
cargo +1.97.1 run --locked -p graphhelm-cli -- schema conformance --catalog schemas/catalog.json --fixtures conformance/manifest.json
cargo +1.97.1 metadata --locked --no-deps --format-version 1
git diff --exit-code 7573170 -- schemas/releases/1.0.0
git diff --check
```

Verified outputs remain catalog `14@1.1.0`, compatible exact `minor` with no
migration, public conformance `48/48`, exact public inventory `50/50`, nested
Serde envelope and 17 closed event variants, preserved `p50.dev` identifiers,
and byte-identical immutable release `1.0.0`.

## Adversarial self-review and residuals

- Every arbitrary GraphVersion surface called out by the review was probed in
  both full and semantic projections: metadata, node extension, node completion,
  edge condition, policy, and graph completion.
- A repository search found no `graph.schema`, `node.schema`, permissive
  `additionalProperties: true`, placeholder, TODO, or deferred production stub
  in either final event schema.
- Erasure correlation was checked against the approved crash-consistent
  prepare/revoke/finalize design. Requested time, scope, actor, and idempotency
  are supplied by the containing event envelope; a per-target event needs no
  additional count. The completion receipt records the required digest, provider
  receipt identity, epoch, and state transition without evidence/source bytes.
- The round-five delta changes only the event schema, its immutable `1.1.0`
  release copy, matching catalog hashes, focused graph/schema-evolution tests,
  and this report. Cargo manifests, lockfile, protocol types, event/runtime
  implementations, public fixtures/manifest, and release `1.0.0` are unchanged.
- Task 5 remains responsible for secret-content scanning of otherwise permitted
  string values; Task 2 closes and bounds their wire structure.
- The exact 192-bit base64url nonce encoding remains the previously deferred
  Minor.

## Final commit

Commit subject: `fix(schema): finalize persistence wire contracts`

The commit SHA is reported in the handoff because this report is included in the
same commit.
