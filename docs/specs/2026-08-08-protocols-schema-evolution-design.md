# Protocols and Schema Evolution Design

**Status:** Approved for implementation planning
**Milestone:** 02
**Original tracking issue:** #3 in the private development archive.
**Normative baseline:** `72c376499e4fc92f7a1097432c703d73c1b2f6b0`

## 1. Purpose

This milestone makes GraphHelm's wire contracts releasable and safely evolvable. It adds a versioned catalog for every checked-in JSON Schema, an offline compatibility classifier, strict SemVer gates, bounded declarative migrations, public conformance fixtures, deterministic JSON views, CLI commands, and cross-platform CI enforcement.

The design preserves the current `https://p50.dev/...` identifiers. Namespace migration requires a separate compatibility ADR and is not part of this milestone.

## 2. Scope

The catalog covers all nine checked-in schemas:

1. `agent.schema.json`
2. `claim.schema.json`
3. `context-capsule.schema.json`
4. `edge.schema.json`
5. `extension.schema.json`
6. `graph.schema.json`
7. `graph-signal.schema.json`
8. `node.schema.json`
9. `policy-waiver.schema.json`

The milestone delivers:

- immutable schema release snapshots;
- catalog and content-hash verification;
- structural compatibility analysis;
- exact SemVer enforcement;
- declarative JSON Patch migrations;
- conformance fixtures and reports;
- canonical JSON schema views;
- JSON-only CLI commands;
- Windows and Linux CI gates.

## 3. Non-goals

This milestone does not:

- rename or redirect `p50.dev` identifiers;
- implement Runtime or Studio APIs;
- generate TypeScript or Python SDKs;
- add database or network adapters;
- sign releases or artifacts;
- migrate persisted Graph Versions or Event Store records;
- execute user code, expressions, plugins, scripts, or external commands;
- implement the later export/replay conformance program.

## 4. Architecture

Add a responsibility-focused Rust crate at `core/schema-evolution`.

```text
apps/cli
  |-- core/schema
  `-- core/schema-evolution
        `-- core/protocols
```

`core/schema` remains responsible for bounded YAML/JSON loading and validation against in-memory schema registries. `core/schema-evolution` owns catalog integrity, compatibility classification, release policy, migration planning/application, canonical schema views, and conformance reports.

The new crate accepts already-loaded values and explicit resources. It has no hidden filesystem, Git, network, clock, model, provider, Graph Engine, Event Store, Policy Engine, or Governor dependency. CLI code is the filesystem adapter and presentation layer.

## 5. Repository layout

Current canonical paths remain stable:

```text
schemas/*.schema.json
```

New evolution artifacts use:

```text
schemas/catalog.json
schemas/CHANGELOG.md
schemas/releases/1.0.0/catalog.json
schemas/releases/1.0.0/*.schema.json
schemas/migrations/<schema-name>/<from>--<to>.json
conformance/schemas/valid/*.json
conformance/schemas/invalid/*.json
conformance/compatibility/*.json
conformance/migrations/*.json
```

Release snapshots are immutable after merge. A later release adds a new directory; it never edits a prior release directory.

## 6. Catalog contract

`schemas/catalog.json` is the current catalog. Its deterministic wire shape is:

```json
{
  "formatVersion": 1,
  "releaseVersion": "1.0.0",
  "schemas": {
    "graph": {
      "id": "https://p50.dev/schemas/graph.schema.json",
      "documentVersion": "1.0.0",
      "path": "schemas/graph.schema.json",
      "sha256": "sha256:<lowercase-hex>"
    }
  }
}
```

Every current and snapshot schema contains `x-graphhelm-schema-version: "1.0.0"`. The current catalog points to files directly under `schemas/`; an immutable release catalog points only to files in its own release directory. Catalog keys, `$id`, extension version, paths, and hashes must agree. When publishing a release, CI requires the current and release-catalog entries to have identical IDs, versions, and hashes. Hashes use canonical JSON bytes with recursively ordered object keys; formatting changes do not change a digest.

Paths are repository-relative, normalized with `/`, and must remain below `schemas/`. Absolute paths, `..`, drive prefixes, symlink escapes, and remote URLs are rejected by the CLI adapter before reads.

## 7. Compatibility model

The checker compares an explicit baseline catalog with an explicit candidate catalog. It resolves `$ref` only against resources supplied by those catalogs and never retrieves remote content.

Each change has:

- schema name;
- stable diagnostic code;
- JSON Pointer;
- baseline and candidate summaries;
- compatibility class;
- required SemVer impact.

Results are sorted by schema name, pointer, and diagnostic code.

### 7.1 Breaking changes

The following require a major version increment:

- adding a required property;
- removing a property, type, enum value, accepted union branch, or compatible schema target;
- narrowing `type`, `enum`, `const`, numeric/string/array bounds, format, pattern, or composition;
- changing `$id`, `$ref`, wire discriminator, field name, or `apiVersion` constant;
- changing `additionalProperties` or `unevaluatedProperties` from permissive to restrictive;
- introducing an unrecognized validation-affecting keyword or an ambiguous composition change.

Unknown or non-provable changes are breaking by default.

### 7.2 Backward-compatible changes

The following require a minor version increment:

- adding an optional property;
- widening an enum, type union, accepted branch, or numeric/string/array bound;
- relaxing a validation constraint without changing identity or wire discriminators.

### 7.3 Annotation-only changes

Changes limited to `title`, `description`, `$comment`, `examples`, and other explicitly registered non-validation annotations require a patch increment.

### 7.4 Exact version gates

For a document and for the aggregate catalog release:

- no content change keeps the version unchanged;
- annotation-only change increments patch;
- compatible validation change increments minor and resets patch;
- breaking change increments major and resets minor/patch.

Version skips and segment changes inconsistent with the most severe classified change fail the gate. A breaking change additionally requires a migration manifest, before/after conformance fixture, and changelog entry.

## 8. Migration contract

Migrations are declarative JSON documents. They cannot call Rust functions or arbitrary code.

```json
{
  "formatVersion": 1,
  "schema": "graph",
  "fromVersion": "1.0.0",
  "toVersion": "2.0.0",
  "sourceSchemaHash": "sha256:<hex>",
  "targetSchemaHash": "sha256:<hex>",
  "operations": [
    {"op": "test", "path": "/apiVersion", "value": "p50.dev/graph/v1"},
    {"op": "replace", "path": "/apiVersion", "value": "p50.dev/graph/v2"}
  ]
}
```

Supported operations are the RFC 6902 operations `add`, `remove`, `replace`, `move`, `copy`, and `test`. The implementation enforces an absolute document-size limit, nesting limit, operation-count limit, pointer-length limit, and output-size limit.

Application is transactional:

1. verify manifest shape and source/target versions;
2. verify source/target schema hashes against the catalogs;
3. validate the original document against the source schema;
4. apply operations to an isolated clone in declared order;
5. validate the result against the target schema;
6. return the complete result only after every check succeeds.

Any failure discards the clone. Migration chains are explicit, strictly increasing by version, cycle-free, and gap-free. Unsupported migrations and downgrades fail with stable diagnostics; there is no best-effort fallback.

The CLI writes migrated content only to an explicit `--output` path using a temporary sibling file followed by an atomic rename. Standard output contains only metadata and hashes, never the migrated instance.

## 9. Conformance suite

Public fixtures cover:

- at least one valid and one invalid instance for each of the nine schemas;
- catalog integrity and digest mismatch;
- annotation, compatible, breaking, and conservative-unknown diffs;
- correct and incorrect SemVer transitions;
- breaking changes with each required artifact missing;
- successful migration;
- invalid source document;
- source/target hash mismatch;
- invalid JSON Pointer and excessive operations;
- partial patch failure proving atomicity;
- invalid destination document;
- migration cycle, gap, and downgrade rejection;
- unresolved or remote `$ref` rejection.

Reports contain fixture ID, result, diagnostics, and deterministic aggregate counts. The same fixtures execute through the library and CLI on Windows and Linux.

## 10. Canonical JSON views

The view generator emits a deterministic machine-facing representation of a schema catalog entry and its normalized schema. It includes catalog identity, versions, hash, resolved local dependency IDs, and recursively sorted schema JSON.

Views contain schemas, not user instances. They do not resolve network references, inline imported content, or expose absolute filesystem paths. They are generated on demand and are not committed as a second source of truth.

## 11. CLI contract

Add these JSON-only commands:

```text
graphhelm schema catalog --catalog <path>
graphhelm schema check --baseline <path> --candidate <path>
graphhelm schema migrate --catalog <path> --migration <path> --input <path> --output <path>
graphhelm schema conformance --catalog <path> --fixtures <path>
graphhelm schema view --catalog <path> --schema <name>
```

Exit codes follow the existing CLI contract:

- `0`: success;
- `2`: invalid catalog, incompatibility, SemVer violation, migration rejection, or conformance failure;
- `4`: filesystem/internal failure.

Expected diagnostic families are:

- `GHC001_CATALOG_INVALID`
- `GHC002_HASH_MISMATCH`
- `GHC003_BREAKING_CHANGE`
- `GHC004_SEMVER_MISMATCH`
- `GHM001_MIGRATION_UNSUPPORTED`
- `GHM002_SCHEMA_HASH_MISMATCH`
- `GHM003_PATCH_INVALID`
- `GHM004_DESTINATION_INVALID`
- `GHCONF001_FIXTURE_FAILED`

Diagnostics use stable JSON Pointers and repository-relative sources. They never include user-home paths, backtraces, instance payloads, or secret-shaped values.

## 12. CI gates

CI runs on Windows and Linux and must:

1. verify catalog shape, paths, versions, and hashes;
2. verify all nine current schemas and release snapshots compile offline;
3. compare the current catalog against the latest immutable release;
4. enforce exact SemVer impact;
5. require migration, conformance fixtures, and changelog for breaking changes;
6. run the public conformance suite;
7. run formatting, warning-free Clippy, workspace tests, CLI smoke tests, and locked metadata.

CI never depends on Git history, network schema retrieval, provider credentials, Docker, or external services.

## 13. Security and trust boundaries

Catalogs, schemas, migration manifests, fixtures, and input instances are untrusted. Controls include:

- bounded reads before parsing;
- bounded JSON depth and collection sizes;
- offline-only `$ref` registry;
- repository-relative path confinement and symlink escape checks;
- canonical hashing before trust decisions;
- conservative compatibility classification;
- bounded transactional JSON Patch;
- atomic output writes;
- no payload echo in normal or error output;
- no arbitrary code, shell, model, plugin, or network execution.

The rollback is a normal Git revert of the milestone. No production data migration or external side effect occurs in this release.

## 14. Testing strategy

Every behavior follows RED -> GREEN -> REFACTOR. Tests use deterministic maps and ordered diagnostics, and assert codes and paths rather than prose alone.

Test layers:

- unit tests for catalog validation, canonical hashing, diff rules, SemVer, and patch bounds;
- property tests for canonical ordering, deterministic reports, and atomic failure;
- integration tests for release snapshots, migration chains, and conformance fixtures;
- CLI acceptance tests for one-JSON-document output, exit codes, path redaction, and atomic output;
- CI execution on Windows and Linux.

## 15. Acceptance criteria

The milestone is complete when:

- all nine schemas have version metadata, current catalog entries, and immutable `1.0.0` snapshots;
- catalog integrity is proven offline by content hashes;
- compatibility reports are deterministic and conservatively correct;
- exact SemVer gates behave as specified;
- breaking changes cannot pass without migration, fixtures, and changelog;
- migrations are declarative, bounded, validated at both ends, and atomic;
- conformance and canonical views are publicly accessible through library and CLI;
- all existing Foundation Graph Kernel behavior remains compatible;
- full local and Windows/Linux CI gates pass;
- independent security and code review find no unresolved critical or important issue.
