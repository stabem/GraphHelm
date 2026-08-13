# Task 2 fix round 4 report

## Status

DONE — both remaining Important findings are corrected within the Task 2 schema, catalog, and contract-test scope. No Task 3+ production Rust source changed, and the deferred nonce Minor remains untouched.

## Review verification and RED evidence

Both findings were verified against base `38498f0edf8d29cd2a4c35ed800130a94340b230` and received independent regression tests before any schema change.

```powershell
cargo +1.97.1 test -p graphhelm-schema-evolution --test conformance semantic_metadata_rejects_concatenated_families_and_repeated_separators_at_any_depth --locked
cargo +1.97.1 test -p graphhelm-schema-evolution --test conformance shared_opaque_ids_follow_normative_printable_ascii_128_byte_grammar --locked
```

Observed REDs were specific to the missing behavior:

- the metadata test failed because root extension key `x-prompttext` was accepted; its table also covers `prompttext`, `systemprompt`, `requestbody`, `accesscredential`, `api__key`, `private--key`, labels, nested objects, and mixed case;
- the opaque-ID test failed because `repository-scope` rejected normative printable-ASCII ID `id+tag`; the same behavioral probe covers every Task 2 schema that defines the shared `opaqueId` (`repository-scope`, `event-envelope`, `evidence-record`, and `artifact-reference`). `sensitivity` has no identifier field.

## GREEN changes

- Metadata keys remain lowercase and bounded, but forbidden replay-field families are now rejected as substrings without token-boundary assumptions. Separator-aware families accept zero or more `.`, `_`, or `-` separators, so concatenation and repeated-separator bypasses fail at root extensions, labels, and every supported nested object level.
- Existing valid `labels`, `policyHash`, `x-retry-policy`, value/property/depth bounds, and the mandatory Task 5 label/extension string content-scanner comment are preserved.
- Shared `opaqueId` is now exactly 1..128 printable ASCII bytes excluding whitespace, `/`, `\`, `:`, controls, DEL, and non-ASCII. Positive probes include `id+tag`, `user@example.com`, `id=1`, and the exact 128-byte boundary; negative probes cover empty, whitespace, each forbidden separator, control text, non-ASCII, and 129 bytes.
- The distinct 256-byte `actorId` grammar and canonical `artifact://sha256/<digest>` locator grammar are unchanged and retain their prior focused tests.
- Root and `schemas/releases/1.1.0` copies are byte-identical for all four changed schema documents. Both catalogs use canonical digests calculated by `graphhelm_schema_evolution::schema_digest`:
  - `artifact-reference`: `sha256:f7a50b0d09dd5d249496ebb2808547dead84dd4cd0b712e610796c0616ba8cc1`
  - `event-envelope`: `sha256:92bdb23c8f98022238329370ef6ee4c83ff8346d4ed28447604a744b6944bfb9`
  - `evidence-record`: `sha256:e6054ce33e6f3e910bef2b8fd3a0eb2f81d975e4e4125e407f4539899468f079`
  - `repository-scope`: `sha256:1ee9f223457782ec6b0e27ff6707d43141589c235c018a9cc5ab715d5a164534`

Focused GREEN results after implementation were `conformance 27/27`, `catalog_integrity 30/30`, and `schema_cli 27/27`.

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

Verified outputs remain catalog `14@1.1.0`, compatible exact `minor` with no migration, public conformance `48/48`, exact public inventory `50/50`, exact nested Serde envelope and 17 event variants, preserved `p50.dev` identifiers, and byte-identical immutable release `1.0.0`.

## Self-review and residuals

- The round-four delta changes only Task 2 schema definitions, their matching immutable `1.1.0` copies/catalog hashes, the focused conformance tests, and this required report.
- No public fixture/manifest count, Foundation schema, dependency, Cargo file, protocol type, event implementation, actor grammar, artifact locator, or Task 3+ behavior changed.
- The metadata content-scanner responsibility remains assigned to Task 5; schema key validation is not represented as secret-content detection.
- The exact 192-bit base64url nonce encoding remains the previously deferred Minor.

## Final commit

Commit subject: `fix(schema): close persistence contract gaps`

The commit SHA is reported in the handoff because this report is included in that same commit.
