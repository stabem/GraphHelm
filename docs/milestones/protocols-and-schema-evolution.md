# Protocols and Schema Evolution

Status: implemented on milestone branch. Toolchain: Rust 1.97.1, edition 2024. Published protocol release: `1.0.0`.

This milestone makes the checked-in GraphHelm JSON Schemas independently verifiable, releasable, and safely evolvable. The current catalog contains 16 schemas at release `1.1.0`; the immutable `1.0.0` baseline contains 15. The implementation adds canonical content hashes, current and immutable catalogs, deterministic compatibility and SemVer gates, bounded declarative migrations, public conformance fixtures, canonical schema views, JSON-only CLI commands, and local Windows validation. The provisional `https://p50.dev/...` identifiers remain unchanged.

## Crate boundaries

| Crate | Responsibility |
|---|---|
| `graphhelm-schema` | Compile explicitly supplied Draft 2020-12 resources into offline validator sets and validate documents without retrieval. |
| `graphhelm-schema-evolution` | Pure, in-memory canonicalization, catalog integrity, compatibility, release policy, migration planning/application, conformance, and canonical views. |
| `graphhelm-cli` | Confined and bounded filesystem reads, checked-in evidence discovery, atomic migration output, stable exit codes, and JSON presentation. |

`graphhelm-schema` and `graphhelm-schema-evolution` do not depend on each other. The CLI supplies validation closures and owns every filesystem boundary. The evolution core has no filesystem, Git, network, clock, model, provider, process, Graph Engine, Event Store, Policy Engine, or Governor dependency.

## Release and catalog contract

`schemas/catalog.json` is the current catalog. It has `formatVersion: 1`, `releaseVersion: "1.1.0"`, and 16 entries. `schemas/releases/1.0.0/catalog.json` is the immutable initial release with 15 sibling schema files. The current catalog adds `execution-accounting-receipt` to the baseline keys: `agent`, `artifact-reference`, `claim`, `context-capsule`, `edge`, `event-envelope`, `evidence-record`, `extension`, `graph`, `graph-signal`, `node`, `persisted-graph-version`, `policy-waiver`, `repository-scope`, and `sensitivity`.

Each entry binds one catalog key to:

- its preserved `https://p50.dev/schemas/<name>.schema.json` identity;
- the schema's stable `x-graphhelm-schema-version`;
- a normalized repository-relative path below `schemas/`;
- lowercase `sha256:` plus the SHA-256 digest of compact canonical UTF-8 JSON.

Canonicalization sorts object keys recursively and preserves array order. Whitespace and object insertion order therefore do not affect the digest. Catalog key, `$id`, document version, path, loaded resource, and digest must all agree. For the 15 schemas shared by both packages, the current package and the `1.0.0` snapshot have equal identities, versions, canonical hashes, and public validation behavior; only their packaging paths differ. The current `1.1.0` package additionally contains `execution-accounting-receipt`, so the packages as a whole are not equal.

Merged release directories are append-only policy: a later release must add a new `schemas/releases/<version>/` package and must never edit an earlier snapshot. This milestone publishes no migration manifest or migration directory because the initial release requires no data migration.

## Enforced resource limits

| Resource | Maximum |
|---|---:|
| One catalog, schema, manifest, fixture, input, or migrated output | 4 MiB |
| Aggregate loaded schema resources | 32 MiB |
| JSON nesting depth | 128 |
| Schemas or versioned validator resources | 256 |
| Operations in one migration | 1,024 |
| One JSON Pointer, measured in UTF-8 bytes | 2,048 |
| Conformance cases | 4,096 |

Limits are checked before expensive parsing or the next aggregate resource read where applicable. Catalog and fixture paths must use normalized `/` separators, remain repository-relative, and stay inside the catalog repository after canonicalization. Absolute paths, drive-prefixed paths, `..`, remote references, and symlink escapes fail closed.

## Compatibility matrix

The checker compares two explicit loaded catalogs, resolves `$ref` only from their supplied resources, and sorts changes by `(schema, JSON Pointer, code)`. Each result carries payload-safe baseline/candidate summaries, a compatibility class, and an exact SemVer impact.

| Change | Class | Required impact |
|---|---|---|
| No schema content change | `unchanged` | none |
| Registered annotations such as `title`, `description`, `$comment`, `examples`, `default`, `deprecated`, `readOnly`, or `writeOnly` | `annotation` | patch |
| Add an optional property; widen a type/enum/accepted branch or bound; remove a provably restrictive validation constraint | `compatible` | minor |
| Add a required property; remove a property/type/enum branch; narrow a constraint; change identity, `$ref`, discriminator, field name, or API-version constant | `breaking` | major |
| Add or change an unrecognized validation keyword, unresolved reference, ambiguous composition, or another change whose acceptance direction cannot be proved | `breaking` (conservative) | major |

Boolean schemas, array-item policies, `additionalProperties`, `unevaluatedProperties`, `const`, numeric/string/array bounds, formats, patterns, and compositions use directional rules. Candidate-only schema subtrees and local fragments are still checked for unresolved references. Unknown validation effects never default to compatible.

## Exact SemVer and release evidence

Stable versions only are accepted; prerelease and build metadata are rejected. For each schema document and for the aggregate catalog:

- unchanged content keeps the version unchanged;
- annotation-only changes increment patch exactly;
- compatible changes increment minor exactly and reset patch;
- breaking changes increment major exactly and reset minor and patch.

Version skips and unrelated segment changes fail. A newly introduced milestone-02 schema must start at `1.0.0`. Schema removal fails closed because this milestone defines no tombstone contract. Every breaking schema change additionally requires an exact migration manifest, a declared before/after conformance fixture pair, and a non-empty `BREAKING` entry in the candidate release's changelog section.

Stable diagnostic families are `GHC001_CATALOG_INVALID`, `GHC002_HASH_MISMATCH`, `GHC003_BREAKING_CHANGE`, `GHC004_SEMVER_MISMATCH`, `GHM001_MIGRATION_UNSUPPORTED`, `GHM002_SCHEMA_HASH_MISMATCH`, `GHM003_PATCH_INVALID`, `GHM004_DESTINATION_INVALID`, and `GHCONF001_FIXTURE_FAILED`.

## Migration transaction and chain rules

A migration manifest has `formatVersion: 1`, one schema name, exact stable `fromVersion` and `toVersion`, source and target schema hashes, and an ordered RFC 6902 subset: `add`, `remove`, `replace`, `move`, `copy`, and `test`. It is declarative data; no script, expression, command, plugin, model, or network operation can execute.

Application is all-or-nothing:

1. validate the strict manifest and target catalog identity/version/hash;
2. load and verify the exact immutable source catalog and hash;
3. bound and validate the original document against the source schema;
4. apply operations in order to an isolated clone, checking pointers and bounds;
5. validate the complete clone against the target schema;
6. serialize within the 4 MiB limit and publish only to the explicit absent `--output` path.

Failure returns no document and discards the clone. The CLI creates a unique sibling temporary file, writes and synchronizes it, then uses no-replacement publication on Windows/Linux; races, existing files, and symlinks fail without changing destination bytes. Temporary names, user payloads, absolute home paths, and secret-shaped values are redacted from normal and error output.

Library chain planning requires a single explicit, strictly increasing, gap-free route. It rejects downgrade, duplicate outgoing edges, cycles, gaps, target overshoot, and discontinuity between adjacent target/source schema hashes. One `schema migrate` CLI call applies exactly the one manifest named by `--migration`; it does not discover or execute a multi-hop chain.

## Public conformance format

`conformance/manifest.json` is strict `formatVersion: 1`. It contains lexicographically sorted, unique canonical case IDs and normalized fixture paths. Cases have one of four kinds: `schema`, `compatibility`, `release`, or `migration`, with an expected `ok` value plus sorted unique diagnostic codes and paths. `validatorResources` explicitly maps version-qualified targets such as `graph@1.0.0` to sorted, confined schema resource paths used only by conformance.

The checked-in suite has 52 cases: 32 schema cases, five compatibility cases, five release cases, and ten migration cases. Its synthetic `graph@2.0.0` validator exists only under conformance fixtures; it is not an official release or a published migration. Reports are deterministic and payload-free, with case ID, pass/fail result, diagnostic codes/paths, kind metadata, and aggregate totals.

## CLI

```text
graphhelm schema catalog --catalog schemas/catalog.json
graphhelm schema check --baseline schemas/releases/1.0.0/catalog.json --candidate schemas/catalog.json
graphhelm schema migrate --catalog <target-catalog> --migration <manifest> --input <document> --output <new-file>
graphhelm schema conformance --catalog schemas/catalog.json --fixtures conformance/manifest.json
graphhelm schema view --catalog schemas/catalog.json --schema graph
```

Every command prints exactly one JSON document; `--pretty` changes whitespace only. `catalog` reports release metadata and canonical digests. `check` reports compatibility and release policy. `migrate` reports schema/from/to metadata and source/output digests, never the migrated instance. `conformance` reports deterministic aggregate and per-case metadata. `view` emits an on-demand canonical schema view with local dependency IDs and no catalog paths.

Schema-command exit codes are `0` for success, `2` for invalid catalog, incompatible release, SemVer/evidence violation, rejected migration, or conformance failure, and `4` for filesystem/internal failure. Existing graph commands retain their established exit-code contract.

## Security controls

- Catalogs, schemas, manifests, fixtures, and instances are untrusted and bounded before use.
- Draft 2020-12 registries contain only explicitly loaded local resources; unresolved and remote `$ref` values are rejected without retrieval.
- Canonical hashes are recomputed before catalog or migration trust decisions.
- Compatibility classification is deterministic and conservative when proof is unavailable.
- Path normalization, repository-root confinement, symlink checks, aggregate read budgets, and bounded directory walks protect the filesystem adapter.
- Migrations are clone-first, endpoint-validated, size/depth/pointer/operation bounded, and atomically published without replacement.
- CLI diagnostics do not expose instances, schema payloads, secrets, backtraces, temporary names, or unrelated filesystem paths.
- CI uses read-only repository permission and checked-in inputs only; it has no credentials, services, Docker, provider calls, or schema-network retrieval.

## Rollback and acceptance evidence

Rollback is a normal Git revert of this milestone. Release `1.0.0` remains the immutable recovery baseline; rollback never rewrites a snapshot. This release performs no production data migration and has no external runtime side effect.

The authoritative local gate verifies formatting, warning-free Clippy, all-feature locked workspace tests, CLI suites, catalog integrity, compatibility against the immutable `1.0.0` baseline, all 52 conformance cases, locked metadata, a clean diff, and the PostgreSQL ignored matrix in two locales. GitHub Actions is disabled and is not a CI fallback. Migration acceptance uses a temporary test repository and synthetic `1.0.0`-to-`2.0.0` resources: the output validates against the target schema, stdout contains only metadata/digests, and a pre-existing destination remains byte-for-byte unchanged.

## Explicitly out of scope

No future protocol release or real data migration is published here. SDK generation, protocol signing, Runtime/Studio APIs, Graph Version or Event Store record migration, database adapters, Git-history analysis, provider/model/plugin execution, arbitrary migration code, export/replay conformance, Harness Compiler, Context Compiler, Agent Registry, Tool Broker, sandboxes, Dreams, and Studio remain separate milestones.
