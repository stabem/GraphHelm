# Task 2 fix round 3 report

## Status

DONE — the remaining Important metadata-key bypass is corrected within the Task 2 schema, catalog, and contract-test scope. No Task 3+ production Rust source was changed, and the deferred nonce Minor remains untouched.

## Review verification and RED evidence

The review finding was verified against `2e349c655cc6c9525d1965e73d5905b321b6a7d1`, not accepted blindly. Repository inspection established that the canonicalizer always emits `labels`, preserves operational `GraphMetadata.properties`, and has one explicit operational-property test for `policyHash`; the checked-in graph examples currently emit `origin` and `mode` labels.

The regression test was added before the schema edit and run with:

```powershell
cargo +1.97.1 test -p graphhelm-schema-evolution --test conformance semantic_metadata_rejects_secret_bearing_keys_case_insensitively_at_any_depth --locked
```

Observed result: one focused test failed because uppercase `PROMPT` metadata was accepted. The expanded table also covers `Prompt`, mixed-case prompt, output, log, credential, authorization, environment, raw tool result, `token`, `secret`, `password`, `apiKey`, `privateKey`, `request`, `instruction`, label keys, and nested occurrences.

## GREEN design and changes

- Closed semantic metadata at the root to `labels`, `policyHash`, and explicit `x-*` operational extensions.
- Required extension and label keys to use a lowercase normalized grammar. This removes casing ambiguity before applying the bounded forbidden-family rule at every supported object depth.
- Extended the forbidden families to prompt/output/log/credential/authorization/environment/raw-tool-result plus token/secret/password/API-key/private-key/request/instruction forms.
- Kept forward compatibility through an explicit `x-*` namespace that the current canonicalizer already preserves as an operational `GraphMetadata.properties` key. Unnamespaced arbitrary top-level metadata is no longer accepted.
- Typed `policyHash` as the existing `wireHash` contract and preserved bounded label strings, property counts, extension arrays, scalars, and finite nesting.
- Added a schema `$comment` making the Task 5 content scanner mandatory for every permitted label/extension string before append. The schema constrains shape and keys; it does not claim to detect secret text inside an otherwise permitted string.
- Published byte-identical root and `1.1.0` event-envelope schemas and refreshed both catalogs with the canonical Rust digest `sha256:7a53e693699470753e59f3d81080de8f5a4db81e06a67a90ca058191bca697f2`.

Focused GREEN:

```text
conformance:       25 passed
catalog_integrity: 30 passed
schema_cli:        27 passed
```

## Final verification

All required pre-commit and Task 2 gates exited 0 after a mechanical `rustfmt` correction:

```powershell
cargo +1.97.1 fmt --all -- --check
cargo +1.97.1 clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo +1.97.1 test --workspace --all-features --locked
cargo +1.97.1 test -p graphhelm-cli --test cli_smoke --locked
cargo +1.97.1 metadata --locked --no-deps --format-version 1
cargo +1.97.1 test -p graphhelm-schema-evolution --test conformance --locked
cargo +1.97.1 test -p graphhelm-schema-evolution --test catalog_integrity --locked
cargo +1.97.1 test -p graphhelm-cli --test schema_cli --locked
cargo +1.97.1 run --locked -p graphhelm-cli -- schema catalog --catalog schemas/catalog.json
cargo +1.97.1 run --locked -p graphhelm-cli -- schema check --baseline schemas/releases/1.0.0/catalog.json --candidate schemas/catalog.json
cargo +1.97.1 run --locked -p graphhelm-cli -- schema conformance --catalog schemas/catalog.json --fixtures conformance/manifest.json
git diff --exit-code 7573170 -- schemas/releases/1.0.0
git diff --check
```

Observed results: catalog `14@1.1.0`; compatibility exact `minor`, compatible, no migration; public conformance `48/48`; root/release event schema raw bytes equal at SHA-256 `1CD7F37ED94DADCEDACBDDF9E6F0FDECBD9BD6EB6BB05A91A15B37B62F06FC2E`; immutable release `1.0.0` unchanged.

## Self-review and residuals

- The round-three delta is limited to the root/snapshot event schema, their catalog digests, and the Task 2 conformance test. This ignored report is the only additional file.
- The exact nested Serde envelope and all 17 closed replay-safe variants remain covered. Artifact, evidence, inventory, release, p50.dev identifier, and SemVer gates remain green.
- The schema now fails closed on unnamespaced semantic metadata and on uppercase/mixed-case extension keys. Explicit `x-*` extension values remain bounded and deterministic.
- Task 5 must still scan permitted string content before append; key-shape validation alone is intentionally not represented as secret-content detection.
- The exact 192-bit base64url nonce encoding remains the previously deferred Minor and was not expanded into this round.

## Final commit

Commit subject: `fix(schema): close semantic metadata bypasses`

The commit SHA is reported in the handoff because this report is included in that same commit.
