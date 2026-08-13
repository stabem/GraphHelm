# Task 2 fix round 6 report

## Status

DONE — the user-authorized exceptional round resolves the three definitive-review Important
findings without starting Task 3 production work. The deferred XChaCha nonce Minor remains
unchanged.

- Base: `eff2148e8f0626d805dab7aafd45dd7bcbb76b7e`
- Branch/worktree: `issue-5-production-event-evidence-store`
- Scope: Task 2 event schema, matching `1.1.0` release/catalog copies, focused conformance tests,
  and this report only
- No push, PR, dependency, Cargo, protocol-production, runtime, migration, fixture-inventory, or
  public-manifest change was performed.

## Review verification and RED evidence

The findings were checked against the approved Foundation schemas, Task 9 atomic-import design,
and public `policy-waiver.schema.json` before implementation. Each received a behavioral test
before its schema change.

```powershell
cargo +1.97.1 test -p graphhelm-schema-evolution --test conformance event_safe_known_fields_match_the_official_foundation_corpus_bidirectionally --locked
cargo +1.97.1 test -p graphhelm-schema-evolution --test conformance legacy_import_receipt_shares_execution_scope_with_the_converted_batch --locked
cargo +1.97.1 test -p graphhelm-schema-evolution --test conformance embedded_policy_waiver_matches_the_normative_schema_exactly --locked
```

All three exited `101` for the expected missing behavior:

- the valid official agent fixture was accepted by Foundation but rejected by `eventSafeAgent`
  because its string `completionContract` was not representable;
- the execution-scoped `LegacyEventsImported` receipt could not join the converted execution
  event in Task 9's required single atomic batch;
- the official valid waiver with omitted `reason` was accepted by the public schema but rejected
  by the embedded copy.

Adversarial review then added a second parity matrix before broadening the implementation:

```powershell
cargo +1.97.1 test -p graphhelm-schema-evolution --test conformance event_safe_known_field_shapes_match_foundation_beyond_the_minimum_fixtures --locked
```

It exited `101` at `empty inputSchema` (`Foundation=true`, projection=`false`). The same table
also covers empty allowed-tool items, null evidence requirements, null permissions, empty agent
references, empty edge payload schemas, null edge conditions, empty `basedOn`, and empty
entrypoints. This prevented a three-literal patch from leaving adjacent known-field type/empty
semantics inconsistent.

A temporary test used `graphhelm_schema_evolution::schema_digest` to print the canonical digest.
Its first compile attempt used `Display` on `SchemaDigest`, which is intentionally unsupported;
the helper was corrected to `Debug`, run, and removed. `catalog_integrity.rs` has no final diff.

## Contract decisions and implementation

### Foundation/event-safe parity

- Official valid/invalid `graph`, `agent`, `node`, and `edge` fixtures now validate
  bidirectionally against their event-safe projections.
- `completionContract` preserves the Foundation union of string or object.
- `capabilities` preserves the Foundation non-empty and unique array contract.
- `policies` accepts only Foundation-valid strings or objects, so `[null]` is rejected.
- Known string/union behavior was aligned for input/output schemas, instructions, allowed tools,
  evidence requirements, agent refs, permissions, edge payload/condition, `basedOn`, and
  entrypoints.
- Forward-compatible objects remain closed through the bounded safe-key/value projection.
  Prompt, output, log, credential, authorization, environment, raw-tool-result, token, secret,
  password, API-key, private-key, request, and instruction families remain rejected through
  mixed case, concatenation, and inserted/repeated separators at every supported depth.
- Previously reviewed repository IDs, actor IDs, event/hash shapes, graph-size bounds, erasure
  receipts, artifact rules, and operational metadata behavior remain unchanged.

### Atomic legacy import scope

`legacy_events_imported` now requires `scopeWithExecution`. The focused test validates one
representative converted `graph_imported` event and its receipt under the exact same execution
scope, and rejects the formerly accepted project-scoped receipt. This matches Task 9's single
`append_atomic` contract without inventing a split batch.

### Normative PolicyWaiver reuse

The embedded definition is now a direct relative `$ref` to `policy-waiver.schema.json` instead of
a drift-prone duplicate. Parity covers the official valid/invalid fixtures plus omitted reason,
string reason, null reason, empty/non-empty risks, `createdAt`, string/null `expiresAt`, and unknown
fields. Consequently:

- `reason` is optional;
- a present `reason` must be a string and `null` is rejected;
- `acknowledgedRisks`, timestamps, and unknown fields follow the higher-precedence public schema
  exactly rather than an independently widened/narrowed copy.

**Task 3 producer requirement:** add
`#[serde(default, skip_serializing_if = "Option::is_none")]` to
`PolicyWaiver.reason`. `None` must be omitted, not serialized as `null`. This round records the
requirement in the event schema `$comment` and report but intentionally does not modify Task 3
production Rust.

## GREEN evidence

Focused GREEN:

```text
event_safe_known_fields_match_the_official_foundation_corpus_bidirectionally: pass
event_safe_known_field_shapes_match_foundation_beyond_the_minimum_fixtures: pass
legacy_import_receipt_shares_execution_scope_with_the_converted_batch: pass
embedded_policy_waiver_matches_the_normative_schema_exactly: pass
```

Complete focused suites:

```text
conformance:       35 passed
catalog_integrity: 30 passed
schema_cli:        27 passed
cli_smoke:          8 passed
```

The event-envelope canonical schema digest was calculated through the workspace API as:

```text
sha256:5d3425bd3a9801b098b9a98e12d0fe25f7a1ee0537401e1e1554f996873ae1a9
```

## Complete verification

All required commands exited `0` after the final implementation:

```powershell
cargo +1.97.1 fmt --all -- --check
cargo +1.97.1 clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo +1.97.1 test --workspace --all-features --locked
cargo +1.97.1 test -p graphhelm-cli --test cli_smoke --locked
cargo +1.97.1 test -p graphhelm-schema-evolution --test catalog_integrity --locked
cargo +1.97.1 test -p graphhelm-schema-evolution --test conformance --locked
cargo +1.97.1 test -p graphhelm-cli --test schema_cli --locked
cargo +1.97.1 run --locked -p graphhelm-cli -- schema catalog --catalog schemas/catalog.json
cargo +1.97.1 run --locked -p graphhelm-cli -- schema check --baseline schemas/releases/1.0.0/catalog.json --candidate schemas/catalog.json
cargo +1.97.1 run --locked -p graphhelm-cli -- schema conformance --catalog schemas/catalog.json --fixtures conformance/manifest.json
cargo +1.97.1 metadata --locked --no-deps --format-version 1
git diff --exit-code 7573170 -- schemas/releases/1.0.0
git diff --check
```

Verified outputs remain:

- aggregate catalog `14@1.1.0`;
- exact compatible `minor` impact with no migration;
- public conformance `48/48` and exact public inventory `50/50`;
- nested Serde envelope and all 17 closed event variants;
- producer-real GraphVersion coverage for all three official graph examples;
- preserved `p50.dev` identifiers;
- byte-identical root/release `1.1.0` event schemas with raw SHA-256
  `ACCA997C5631E0C8A98B5143455E97C5B4ADF648C793A725D23B8D75EB47CC71`;
- byte-identical immutable `schemas/releases/1.0.0` versus pre-Task-2 commit `7573170`.

## Adversarial self-review

- Property and required-set comparison found no missing or extra known property in agent, node,
  edge, graph root, or graph spec. Graph metadata has only the intentional `policyHash`
  operational extension.
- The official corpus and the additional known-field mutation table compare actual validator
  outcomes, not source text or mocks.
- The legacy test uses full envelopes and proves both positive same-scope validation and negative
  project-scope rejection.
- The waiver test compares the embedded definition and a full event against the public validator
  for every listed case; direct `$ref` removes duplicate validation logic.
- Safe-key generator, unsafe arbitrary-surface probes, erasure audit completeness, actor/opaque-ID
  bounds, artifact contract, hash formats, and producer-real graph examples all remained green.
- Final scope contains only the five Task 2 schema/test/catalog files plus this report. No
  Foundation schema, public fixture/manifest, Cargo file, lockfile, Task 3 type, runtime, or
  `1.0.0` release file changed.
- The deferred Minor remains unchanged: evidence nonce still permits 32..64 base64url characters
  instead of exactly the 32-character encoding of a 192-bit XChaCha nonce.

## Final commit

Planned subject: `fix(schema): align persistence integration contracts`

The final commit SHA is reported in the handoff because this report is part of that commit.
