# Protocols and Schema Evolution Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make all nine checked-in GraphHelm JSON Schemas independently releasable through immutable snapshots, canonical content hashes, conservative compatibility classification, exact SemVer gates, bounded declarative migrations, public conformance fixtures, deterministic views, and JSON-only CLI commands.

**Architecture:** Add a pure `core/schema-evolution` crate that consumes explicit in-memory catalogs, schemas, migration manifests, and fixture definitions. Extend `core/schema` with an offline in-memory schema set, while keeping all path resolution, bounded file reads, symlink confinement, atomic output, and CLI presentation in `apps/cli`. The evolution crate depends only on `core/protocols` plus pinned data-processing libraries and never performs filesystem, network, Git, clock, model, provider, or process operations.

**Tech Stack:** Rust 1.97.1 (edition 2024), Serde/JSON, JSON Schema draft 2020-12, SHA-256, `semver 1.0.28`, Clap, property tests, assert_cmd, tempfile, and GitHub Actions on Windows/Linux.

## Global constraints

- Track the work in GitHub issue `#3`, branch `issue-3-protocol-schema-evolution`, and worktree `F:\github\GraphHelm\.worktrees\issue-3-protocol-schema-evolution`.
- Preserve every current `https://p50.dev/...` `$id`, relative `$ref`, wire discriminator, and root `schemas/*.schema.json` path. Namespace migration is out of scope.
- Cover exactly the nine current schemas: `agent`, `claim`, `context-capsule`, `edge`, `extension`, `graph`, `graph-signal`, `node`, and `policy-waiver`.
- Treat catalog files, schemas, migration manifests, fixture manifests, fixtures, and input documents as untrusted.
- Use only explicit in-memory resources in core crates. Reject unresolved and remote references without network retrieval.
- Pin `semver = "=1.0.28"`; keep every other existing direct dependency exactly pinned and run with `--locked`.
- Canonical schema bytes are compact UTF-8 JSON with object keys recursively sorted, array order preserved, and numbers/strings serialized by `serde_json`. Hashes are lowercase `sha256:`-prefixed hex.
- Reject input files larger than 4 MiB, aggregate catalog resources larger than 32 MiB, JSON depth above 128, catalogs above 256 schemas, migrations above 1,024 operations, JSON Pointers above 2,048 UTF-8 bytes, and conformance manifests above 4,096 cases. Migrated output is limited to 4 MiB.
- Compatibility is conservative: an unrecognized validation-affecting keyword, unprovable composition change, or ambiguous transformation is breaking.
- Migration application is clone-first and all-or-nothing. The CLI writes only to an explicit output path through a temporary sibling followed by an atomic rename.
- CLI stdout is exactly one JSON document. It never echoes migrated/user payloads, secret-shaped values, absolute user-home paths, backtraces, or temporary paths.
- Domain failures exit `2`; filesystem/internal failures exit `4`; existing graph command behavior and exit codes remain unchanged.
- No SDK generation, Runtime/Studio API, database, network, signing, Git-history analysis, arbitrary Rust migration, scripting, shell, provider, model, plugin, or external service belongs in this milestone.

---

## Locked file and dependency map

```text
Cargo.toml
Cargo.lock
.github/workflows/ci.yml
core/schema/
  Cargo.toml
  src/{lib,registry}.rs
  tests/offline_registry.rs
core/schema-evolution/
  Cargo.toml
  src/{lib,limits,canonical,catalog,compatibility,release,migration,conformance,view}.rs
  tests/{catalog_integrity,compatibility,release_gate,migration,conformance,determinism}.rs
apps/cli/
  Cargo.toml
  src/{args,commands/mod}.rs
  src/commands/schema/{mod,io,catalog,check,migrate,conformance,view}.rs
  tests/{cli_smoke,schema_cli}.rs
schemas/
  catalog.json
  CHANGELOG.md
  *.schema.json
  releases/1.0.0/{catalog.json,*.schema.json}
  migrations/<schema>/<from>--<to>.json
conformance/
  manifest.json
  schemas/{valid,invalid}/*.json
  compatibility/*.json
  migrations/*.json
docs/milestones/protocols-and-schema-evolution.md
README.md
```

Dependency direction is fixed:

```text
protocols <- schema
protocols <- schema-evolution
schema + schema-evolution <- cli
```

`core/schema` and `core/schema-evolution` do not depend on one another. The CLI passes `core/schema` validation closures into migration and conformance APIs.

## Public interfaces fixed by this plan

```rust
pub const MAX_FILE_BYTES: usize = 4 * 1024 * 1024;
pub const MAX_RESOURCE_BYTES: usize = 32 * 1024 * 1024;
pub const MAX_JSON_DEPTH: usize = 128;
pub const MAX_SCHEMAS: usize = 256;
pub const MAX_MIGRATION_OPERATIONS: usize = 1_024;
pub const MAX_POINTER_BYTES: usize = 2_048;
pub const MAX_CONFORMANCE_CASES: usize = 4_096;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize)]
pub struct SchemaDigest(String);

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SchemaCatalog {
    pub format_version: u32,
    pub release_version: semver::Version,
    pub schemas: std::collections::BTreeMap<String, CatalogEntry>,
}

pub struct CatalogResources {
    pub catalog_source: String,
    pub catalog: SchemaCatalog,
    pub schemas: std::collections::BTreeMap<String, serde_json::Value>,
}

pub fn canonical_json(value: &serde_json::Value) -> Result<Vec<u8>, EvolutionError>;
pub fn schema_digest(value: &serde_json::Value) -> Result<SchemaDigest, EvolutionError>;
pub fn validate_catalog(resources: &CatalogResources) -> CatalogReport;
pub fn compare_catalogs(baseline: &CatalogResources, candidate: &CatalogResources) -> CompatibilityReport;
pub fn enforce_release(
    baseline: &CatalogResources,
    candidate: &CatalogResources,
    compatibility: &CompatibilityReport,
    evidence: &ReleaseEvidence,
) -> ReleaseReport;

pub fn apply_migration<Source, Target>(
    document: &serde_json::Value,
    manifest: &MigrationManifest,
    catalogs: &MigrationCatalogs<'_>,
    validate_source: Source,
    validate_target: Target,
) -> MigrationResult
where
    Source: Fn(&serde_json::Value) -> Vec<Diagnostic>,
    Target: Fn(&serde_json::Value) -> Vec<Diagnostic>;

pub fn plan_migration_chain(
    schema: &str,
    from: &semver::Version,
    to: &semver::Version,
    manifests: &[MigrationManifest],
) -> Result<Vec<MigrationManifest>, Vec<Diagnostic>>;

pub fn run_conformance<V>(suite: &ConformanceSuite, resources: &ConformanceResources, validate: V)
    -> ConformanceReport
where
    V: Fn(&str, &serde_json::Value) -> Vec<Diagnostic>;

pub fn canonical_view(resources: &CatalogResources, schema: &str)
    -> Result<CanonicalSchemaView, Vec<Diagnostic>>;
```

`core/schema` adds this independent adapter API:

```rust
pub struct OfflineSchemaSet { /* compiled in-memory registry */ }

impl OfflineSchemaSet {
    pub fn compile(resources: std::collections::BTreeMap<String, serde_json::Value>)
        -> Result<Self, Vec<Diagnostic>>;
    pub fn validate(
        &self,
        schema_id: &str,
        document: &serde_json::Value,
        source: &str,
    ) -> Vec<Diagnostic>;
}
```

All report types derive `Serialize`, use `BTreeMap`/sorted vectors, and expose no filesystem handles. `CompatibilityChange` sorts by `(schema, pointer, code)` and contains `baseline_summary`/`candidate_summary`, never a whole schema or instance.

## Stable diagnostic catalog

| Code | Meaning and primary path |
|---|---|
| `GHC001_CATALOG_INVALID` | Invalid catalog shape, key, path, `$id`, document version, duplicate identity, resource bound, or offline reference; catalog/schema pointer. |
| `GHC002_HASH_MISMATCH` | Canonical schema digest differs from the catalog; `/schemas/{escaped-name}/sha256`. |
| `GHC003_BREAKING_CHANGE` | Breaking or conservatively unknown schema change; exact schema JSON Pointer. |
| `GHC004_SEMVER_MISMATCH` | Document/catalog version does not equal the required impact or required breaking evidence is absent; version/evidence pointer. |
| `GHM001_MIGRATION_UNSUPPORTED` | Downgrade, gap, cycle, wrong schema/version, or unavailable migration; manifest/version pointer. |
| `GHM002_SCHEMA_HASH_MISMATCH` | Migration source/target digest disagrees with catalogs; hash field pointer. |
| `GHM003_PATCH_INVALID` | Unsupported/bounded/invalid patch or failed `test`; operation/pointer path. |
| `GHM004_DESTINATION_INVALID` | Source or destination schema validation prevents migration publication; `/source` or `/destination`. |
| `GHCONF001_FIXTURE_FAILED` | Fixture actual result differs from its declared expectation; `/cases/{index}`. |

JSON Pointer segments escape `~` as `~0` and `/` as `~1`. All diagnostics sort by `(source, path, code, message)` before serialization.

---

### Task 1: Create the schema-evolution crate and deterministic primitives

**Files:**
- Modify: `Cargo.toml`
- Modify: `Cargo.lock`
- Create: `core/schema-evolution/Cargo.toml`
- Create: `core/schema-evolution/src/{lib,limits,canonical}.rs`
- Create: `core/schema-evolution/tests/determinism.rs`

**Interfaces:** Establish the pure crate boundary, `SchemaDigest`, canonical JSON, JSON-depth checks, stable diagnostic sorting, and fixed limits.

- [ ] **Step 1: Add a failing canonicalization test before the crate implementation**

```rust
use graphhelm_schema_evolution::{canonical_json, schema_digest};

#[test]
fn object_order_and_whitespace_do_not_change_schema_digest() {
    let left = serde_json::json!({"type":"object","properties":{"b":{"type":"string"},"a":{"type":"integer"}}});
    let right: serde_json::Value = serde_json::from_str(
        r#"{ "properties": { "a": {"type":"integer"}, "b": {"type":"string"} }, "type":"object" }"#,
    ).unwrap();
    assert_eq!(canonical_json(&left).unwrap(), canonical_json(&right).unwrap());
    assert_eq!(schema_digest(&left).unwrap(), schema_digest(&right).unwrap());
}

#[test]
fn arrays_remain_order_sensitive() {
    assert_ne!(
        schema_digest(&serde_json::json!({"enum":["a","b"]})).unwrap(),
        schema_digest(&serde_json::json!({"enum":["b","a"]})).unwrap(),
    );
}
```

- [ ] **Step 2: Run the focused test to prove RED**

```powershell
cargo +1.97.1 test -p graphhelm-schema-evolution --test determinism --locked
```

Expected: Cargo reports the missing workspace package/module.

- [ ] **Step 3: Add the crate and exact dependency pins**

Add workspace member `core/schema-evolution` and workspace dependency `semver = { version = "=1.0.28", features = ["serde"] }`. The crate dependencies are only `graphhelm-protocols`, `hex`, `semver`, `serde`, `serde_json`, `sha2`, and `thiserror`; dev dependencies are `proptest` and `tempfile`.

- [ ] **Step 4: Implement bounded recursive canonicalization**

`canonical_json` first rejects depth above `MAX_JSON_DEPTH`, then recursively rebuilds objects into `BTreeMap<String, Value>`, preserves array order, serializes with `serde_json::to_vec`, and hashes those exact bytes with SHA-256. `SchemaDigest::parse` accepts only `sha256:` plus exactly 64 lowercase hexadecimal characters.

- [ ] **Step 5: Add property tests for determinism and bounds**

Generate shallow JSON objects, permute insertion order, and assert equal bytes/digests. Build a 129-level nested array and assert a redacted `GHC001_CATALOG_INVALID` diagnostic without a panic or serialized payload.

- [ ] **Step 6: Verify and commit the primitive boundary**

```powershell
cargo +1.97.1 fmt --all
cargo +1.97.1 test -p graphhelm-schema-evolution --test determinism --locked
cargo +1.97.1 clippy -p graphhelm-schema-evolution --all-targets --locked -- -D warnings
git add Cargo.toml Cargo.lock core/schema-evolution
git commit -m "feat(schema): add deterministic evolution primitives" -m "Refs #3" -m "Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

---

### Task 2: Validate catalogs and compile arbitrary offline schema sets

**Files:**
- Create: `core/schema-evolution/src/catalog.rs`
- Modify: `core/schema-evolution/src/lib.rs`
- Create: `core/schema-evolution/tests/catalog_integrity.rs`
- Modify: `core/schema/src/{lib,registry}.rs`
- Create: `core/schema/tests/offline_registry.rs`

**Interfaces:** Parse/validate `SchemaCatalog`, verify canonical hashes and identity coherence, and compile all explicit schema resources without retrieval.

- [ ] **Step 1: Write RED tests for catalog coherence**

```rust
#[test]
fn catalog_rejects_key_id_version_and_digest_disagreement() {
    let mut resources = one_schema_catalog("graph", "1.0.0");
    resources.catalog.schemas.get_mut("graph").unwrap().id =
        "https://p50.dev/schemas/node.schema.json".into();
    let report = validate_catalog(&resources);
    assert_eq!(report.diagnostics[0].code, "GHC001_CATALOG_INVALID");
    assert_eq!(report.diagnostics[0].path, "/schemas/graph/id");
}

#[test]
fn formatting_only_change_keeps_catalog_hash_valid() {
    let resources = catalog_with_equivalent_reformatted_schema();
    assert!(validate_catalog(&resources).ok);
}
```

Also test `formatVersion != 1`, invalid schema names, absolute/drive/URL/`..` paths, missing `x-graphhelm-schema-version`, more than 256 entries, duplicate `$id`, malformed digest, and catalog/resource size overflow.

- [ ] **Step 2: Run catalog and registry tests to prove RED**

```powershell
cargo +1.97.1 test -p graphhelm-schema-evolution --test catalog_integrity --locked
cargo +1.97.1 test -p graphhelm-schema --test offline_registry --locked
```

Expected: catalog types and `OfflineSchemaSet` do not exist.

- [ ] **Step 3: Implement catalog validation in the pure crate**

Use `#[serde(deny_unknown_fields)]` on the catalog and entry types. Require schema keys to match `^[a-z][a-z0-9-]*$`; require repository-relative normalized `/` paths below `schemas/`; require each loaded schema's `$id` and `x-graphhelm-schema-version` to equal its entry; verify canonical digest; and require every catalog entry to have exactly one loaded resource.

- [ ] **Step 4: Generalize the existing schema registry without changing graph APIs**

Refactor `core/schema/src/registry.rs` so `validate_graph_value` and `validate_waiver` build/use `OfflineSchemaSet` over the existing embedded resources. `OfflineSchemaSet::compile` registers each explicit `$id` with `jsonschema::Registry` draft 2020-12 and calls `prepare`; `validate` accepts only an already-registered root ID. Do not add a retriever or filesystem/network callback.

- [ ] **Step 5: Prove unresolved and remote references fail offline**

Add registry tests for an unresolved relative `$ref` and `https://example.com/remote.json`; both must fail compilation with `GHS002_SCHEMA` without an HTTP attempt. Re-run all existing `graphhelm-schema` tests to prove compatibility.

- [ ] **Step 6: Verify and commit**

```powershell
cargo +1.97.1 test -p graphhelm-schema-evolution --test catalog_integrity --locked
cargo +1.97.1 test -p graphhelm-schema --locked
cargo +1.97.1 clippy -p graphhelm-schema-evolution -p graphhelm-schema --all-targets --locked -- -D warnings
git add core/schema-evolution core/schema
git commit -m "feat(schema): validate versioned offline catalogs" -m "Refs #3" -m "Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

---

### Task 3: Classify compatibility conservatively and deterministically

**Files:**
- Create: `core/schema-evolution/src/compatibility.rs`
- Modify: `core/schema-evolution/src/lib.rs`
- Create: `core/schema-evolution/tests/compatibility.rs`
- Create: `conformance/compatibility/{annotation-only,compatible-optional-property,breaking-required-property,breaking-unknown-keyword,unresolved-ref}.json`

**Interfaces:** Produce sorted `CompatibilityReport`, `CompatibilityChange`, `CompatibilityClass`, and `SemverImpact` values for explicit baseline/candidate resources.

- [ ] **Step 1: Add table-driven RED tests for the rule matrix**

```rust
#[test]
fn representative_changes_have_exact_impacts() {
    for (fixture, class, impact, code, pointer) in [
        ("annotation-only.json", "annotation", "patch", "GHC101_ANNOTATION_CHANGED", "/description"),
        ("compatible-optional-property.json", "compatible", "minor", "GHC102_OPTIONAL_PROPERTY_ADDED", "/properties/nickname"),
        ("breaking-required-property.json", "breaking", "major", "GHC003_BREAKING_CHANGE", "/required"),
        ("breaking-unknown-keyword.json", "breaking", "major", "GHC003_BREAKING_CHANGE", "/mysteryConstraint"),
    ] {
        let report = compare_fixture(fixture);
        assert_change(&report, class, impact, code, pointer);
    }
}
```

Add cases for property removal, type/enum/union branch narrowing and widening, `const`, numeric/string/array bounds, `format`, `pattern`, `$id`, `$ref`, discriminator/`apiVersion`, `additionalProperties`, `unevaluatedProperties`, `allOf`/`anyOf`/`oneOf`/`not`, and unchanged schemas.

- [ ] **Step 2: Run compatibility tests to prove RED**

```powershell
cargo +1.97.1 test -p graphhelm-schema-evolution --test compatibility --locked
```

Expected: compatibility module/API is absent.

- [ ] **Step 3: Implement an explicit keyword classifier**

Register annotation-only keywords `title`, `description`, `$comment`, `examples`, `default`, `deprecated`, `readOnly`, and `writeOnly`. Implement specialized comparisons for `properties`/`required`, sets (`type`, `enum`), identity (`$id`, `$ref`, `const`), lower/upper bounds, patterns/formats, boolean-or-schema additional/unevaluated properties, array items, and composition arrays. Any changed key not explicitly proven annotation-only or compatible emits `GHC003_BREAKING_CHANGE`.

- [ ] **Step 4: Resolve local references before compatibility decisions**

Resolve relative `$ref` against the owning catalog entry `$id` using only the catalog resource map. Compare stable resolved IDs, detect cycles with an explicit visited stack, and reject missing/remote targets as breaking. Never recursively inline an unbounded graph.

- [ ] **Step 5: Make reports deterministic and payload-safe**

Sort changes by `(schema, pointer, code)`. Summaries are bounded labels such as `required set expanded` or `enum member removed`; they must not serialize raw property values, patterns, examples, or whole schemas.

- [ ] **Step 6: Verify and commit**

```powershell
cargo +1.97.1 test -p graphhelm-schema-evolution --test compatibility --locked
cargo +1.97.1 test -p graphhelm-schema-evolution --test determinism --locked
cargo +1.97.1 clippy -p graphhelm-schema-evolution --all-targets --locked -- -D warnings
git add core/schema-evolution conformance/compatibility
git commit -m "feat(schema): classify protocol compatibility" -m "Refs #3" -m "Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

---

### Task 4: Enforce exact SemVer and breaking-change evidence

**Files:**
- Create: `core/schema-evolution/src/release.rs`
- Modify: `core/schema-evolution/src/lib.rs`
- Create: `core/schema-evolution/tests/release_gate.rs`
- Create: `conformance/compatibility/{semver-too-small,semver-skip,breaking-missing-migration,breaking-missing-fixture,breaking-missing-changelog}.json`

**Interfaces:** Compute exact document/catalog version transitions and require explicit migration, before/after fixture pair, and changelog evidence for every breaking schema.

- [ ] **Step 1: Write RED tests for exact transitions**

```rust
#[test]
fn exact_impact_is_required_without_version_skips() {
    assert_gate("1.2.3", "1.2.3", SemverImpact::None, true);
    assert_gate("1.2.3", "1.2.4", SemverImpact::Patch, true);
    assert_gate("1.2.3", "1.3.0", SemverImpact::Minor, true);
    assert_gate("1.2.3", "2.0.0", SemverImpact::Major, true);
    assert_gate("1.2.3", "1.4.0", SemverImpact::Minor, false);
    assert_gate("1.2.3", "2.1.0", SemverImpact::Major, false);
}

#[test]
fn breaking_release_needs_all_three_evidence_kinds() {
    for missing in ["migration", "fixturePair", "changelog"] {
        let report = breaking_release_without(missing);
        assert_eq!(report.diagnostics[0].code, "GHC004_SEMVER_MISMATCH");
    }
}
```

- [ ] **Step 2: Run release tests to prove RED**

```powershell
cargo +1.97.1 test -p graphhelm-schema-evolution --test release_gate --locked
```

Expected: release gate types/functions are missing.

- [ ] **Step 3: Implement exact SemVer transition functions**

For each document, map `None` to unchanged, `Patch` to `(major, minor, patch + 1)`, `Minor` to `(major, minor + 1, 0)`, and `Major` to `(major + 1, 0, 0)`. Apply the most severe document impact to `catalog.releaseVersion`. Prerelease/build metadata are rejected in milestone 02 catalogs.

- [ ] **Step 4: Implement structured evidence checks**

`ReleaseEvidence` contains `BTreeSet<MigrationKey>`, `BTreeSet<FixturePairKey>`, and `BTreeSet<ChangelogKey>`. A breaking `graph 1.0.0 -> 2.0.0` needs the exact same tuple in all sets. The CLI later builds these sets from migration manifests, the conformance manifest, and a changelog section headed `## [2.0.0]` containing a non-empty `- BREAKING graph:` line.

- [ ] **Step 5: Verify unchanged current-vs-snapshot release behavior**

Add a test where current and `releases/1.0.0` have identical canonical schemas/versions but different catalog paths. Path differences are packaging metadata and do not create a schema change; the gate succeeds with release version unchanged.

- [ ] **Step 6: Verify and commit**

```powershell
cargo +1.97.1 test -p graphhelm-schema-evolution --test release_gate --locked
cargo +1.97.1 clippy -p graphhelm-schema-evolution --all-targets --locked -- -D warnings
git add core/schema-evolution conformance/compatibility
git commit -m "feat(schema): enforce exact protocol semver" -m "Refs #3" -m "Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

---

### Task 5: Apply bounded JSON Patch migrations transactionally

**Files:**
- Create: `core/schema-evolution/src/migration.rs`
- Modify: `core/schema-evolution/src/lib.rs`
- Create: `core/schema-evolution/tests/migration.rs`
- Create: `conformance/migrations/{success,source-invalid,hash-mismatch,invalid-pointer,partial-failure,destination-invalid,cycle,gap,downgrade,remote-ref}.json`

**Interfaces:** Deserialize strict migration manifests, plan explicit increasing chains, and apply RFC 6902 `add`, `remove`, `replace`, `move`, `copy`, and `test` to a clone under fixed bounds.

- [ ] **Step 1: Add RED tests for success, validation, bounds, and atomicity**

```rust
#[test]
fn failed_second_operation_leaves_the_caller_document_unchanged() {
    let original = serde_json::json!({"apiVersion":"p50.dev/graph/v1","name":"before"});
    let before = original.clone();
    let result = apply_fixture("partial-failure.json", &original);
    assert!(!result.ok);
    assert_eq!(result.diagnostics[0].code, "GHM003_PATCH_INVALID");
    assert_eq!(original, before);
    assert!(result.document.is_none());
}

#[test]
fn exact_increasing_gap_free_chain_is_selected() {
    let chain = plan_fixture_chain("1.0.0", "3.0.0").unwrap();
    assert_eq!(chain.iter().map(|m| m.to_version.to_string()).collect::<Vec<_>>(), ["2.0.0", "3.0.0"]);
}
```

Add tests for invalid source, source/target digest mismatch, invalid pointer escaping/index, failed `test`, more than 1,024 operations, pointer over 2,048 bytes, input/output over 4 MiB, depth over 128, invalid destination, duplicate outgoing edge, cycle, gap, downgrade, and remote reference.

- [ ] **Step 2: Run migration tests to prove RED**

```powershell
cargo +1.97.1 test -p graphhelm-schema-evolution --test migration --locked
```

Expected: migration API is absent.

- [ ] **Step 3: Implement strict manifest and operation wire types**

Use `#[serde(tag = "op", rename_all = "lowercase", deny_unknown_fields)]`. Require `formatVersion == 1`, `fromVersion < toVersion`, exact schema key, valid digests, and operation/value fields appropriate to each operation. Parse JSON Pointer tokens centrally, including `~0`, `~1`, array index rules, and `-` only for final `add`.

- [ ] **Step 4: Implement clone-first patch execution**

Validate input size/depth and source schema first. Clone once, apply operations sequentially to the clone, recheck depth/output size after every structural operation, then validate the final clone with the target closure. On any error return `document: None`; never return partial content or include tested/replaced values in diagnostics.

- [ ] **Step 5: Implement chain planning with graph invariants**

Index manifests by `(schema, fromVersion)`. Reject multiple outgoing migrations, repeated versions, non-increasing edges, target overshoot, cycles, gaps, and downgrades. Return only the unique chain whose last `toVersion` equals the requested target.

- [ ] **Step 6: Add property tests for panic freedom and atomic failure**

Generate bounded random documents and operation sequences. Wrap application in normal test execution (no unsafe/catch suppression), assert it never mutates the borrowed input, and assert every failure has no output document.

- [ ] **Step 7: Verify and commit**

```powershell
cargo +1.97.1 test -p graphhelm-schema-evolution --test migration --locked
cargo +1.97.1 clippy -p graphhelm-schema-evolution --all-targets --locked -- -D warnings
git add core/schema-evolution conformance/migrations
git commit -m "feat(schema): apply bounded declarative migrations" -m "Refs #3" -m "Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

---

### Task 6: Publish all nine schema fixtures and deterministic conformance reports

**Files:**
- Create: `core/schema-evolution/src/conformance.rs`
- Modify: `core/schema-evolution/src/lib.rs`
- Create: `core/schema-evolution/tests/conformance.rs`
- Create: `conformance/manifest.json`
- Create: `conformance/schemas/valid/{agent,claim,context-capsule,edge,extension,graph,graph-signal,node,policy-waiver}.json`
- Create: `conformance/schemas/invalid/{agent,claim,context-capsule,edge,extension,graph,graph-signal,node,policy-waiver}.json`

**Interfaces:** Run manifest-declared schema, compatibility, release, and migration cases with deterministic per-case results and aggregate counts.

- [ ] **Step 1: Define the manifest contract in a failing test**

```json
{
  "formatVersion": 1,
  "cases": [
    {
      "id": "schema.graph.valid.minimum",
      "kind": "schema",
      "schema": "graph",
      "input": "conformance/schemas/valid/graph.json",
      "expect": {"ok": true, "codes": []}
    },
    {
      "id": "schema.graph.invalid.kind",
      "kind": "schema",
      "schema": "graph",
      "input": "conformance/schemas/invalid/graph.json",
      "expect": {"ok": false, "codes": ["GHS002_SCHEMA"]}
    }
  ]
}
```

The real manifest lists every fixture created in Tasks 3–6 and declares before/after pairs used by the release gate.

- [ ] **Step 2: Run conformance tests to prove RED**

```powershell
cargo +1.97.1 test -p graphhelm-schema-evolution --test conformance --locked
```

Expected: conformance types/runner are absent.

- [ ] **Step 3: Implement strict suite and report types**

Reject duplicate/unsorted case IDs, more than 4,096 cases, unknown kinds, unsafe paths, and unexpected manifest fields. Run cases in lexicographic ID order. Each `ConformanceCaseResult` contains only ID, pass/fail, sorted diagnostic codes/paths, and elapsed-independent metadata; aggregate counts use deterministic integers.

- [ ] **Step 4: Create valid and invalid instances for every schema**

Base fixtures on each checked-in schema's true required fields. Each invalid fixture changes one field only and declares the exact expected schema diagnostic. Do not copy secret-like tokens or production data into fixtures.

- [ ] **Step 5: Connect compatibility/release/migration fixtures**

The runner loads already-parsed fixture resources supplied by the caller, dispatches to the pure APIs, and converts any expectation mismatch to `GHCONF001_FIXTURE_FAILED` at `/cases/{index}`. A fixture's expected failure is a passing conformance case.

- [ ] **Step 6: Prove order independence**

Reverse the supplied resource-map insertion order and assert byte-identical serialized reports. Also assert a deliberately wrong expected code fails only that case and yields stable aggregate counts.

- [ ] **Step 7: Verify and commit**

```powershell
cargo +1.97.1 test -p graphhelm-schema-evolution --test conformance --locked
cargo +1.97.1 test -p graphhelm-schema-evolution --locked
cargo +1.97.1 clippy -p graphhelm-schema-evolution --all-targets --locked -- -D warnings
git add core/schema-evolution conformance
git commit -m "test(schema): publish protocol conformance suite" -m "Refs #3" -m "Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

---

### Task 7: Generate canonical schema views without a second source of truth

**Files:**
- Create: `core/schema-evolution/src/view.rs`
- Modify: `core/schema-evolution/src/lib.rs`
- Add tests to: `core/schema-evolution/tests/catalog_integrity.rs`

**Interfaces:** Return catalog identity, versions, digest, sorted local dependency IDs, and normalized schema JSON on demand.

- [ ] **Step 1: Add a RED view test**

```rust
#[test]
fn graph_view_is_canonical_and_lists_only_local_dependencies() {
    let view = canonical_view(&release_resources(), "graph").unwrap();
    assert_eq!(view.name, "graph");
    assert_eq!(view.schema_id, "https://p50.dev/schemas/graph.schema.json");
    assert_eq!(view.dependencies, [
        "https://p50.dev/schemas/edge.schema.json",
        "https://p50.dev/schemas/node.schema.json",
    ]);
    assert!(view.schema.is_object());
}
```

- [ ] **Step 2: Run to prove RED**

```powershell
cargo +1.97.1 test -p graphhelm-schema-evolution --test catalog_integrity graph_view --locked
```

Expected: view API is absent.

- [ ] **Step 3: Implement canonical views**

Walk `$ref` values with the same depth/visited bounds, resolve only catalog-local IDs, deduplicate/sort dependencies, and attach the recursively key-sorted schema. Do not inline dependencies or include catalog/schema filesystem paths.

- [ ] **Step 4: Verify views are generated only**

Add tests for missing schema, remote/unresolved reference rejection, and stable JSON bytes. Do not create or commit `views/` output files.

- [ ] **Step 5: Verify and commit**

```powershell
cargo +1.97.1 test -p graphhelm-schema-evolution --test catalog_integrity --locked
cargo +1.97.1 clippy -p graphhelm-schema-evolution --all-targets --locked -- -D warnings
git add core/schema-evolution
git commit -m "feat(schema): generate canonical protocol views" -m "Refs #3" -m "Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

---

### Task 8: Add the confined filesystem adapter and JSON-only schema CLI

**Files:**
- Modify: `apps/cli/Cargo.toml`
- Modify: `apps/cli/src/args.rs`
- Modify: `apps/cli/src/commands/mod.rs`
- Create: `apps/cli/src/commands/schema/{mod,io,catalog,check,migrate,conformance,view}.rs`
- Create: `apps/cli/tests/schema_cli.rs`

**Interfaces:** Expose the five approved commands, load bounded resources safely, build release evidence, validate through `OfflineSchemaSet`, and atomically write migration output.

- [ ] **Step 1: Add RED CLI contract tests**

```rust
#[test]
fn catalog_command_returns_one_json_document() {
    let output = command().args(["schema", "catalog", "--catalog", path("schemas/catalog.json")]).output().unwrap();
    assert!(output.status.success());
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["command"], "schema.catalog");
    assert_eq!(value["data"]["schemaCount"], 9);
}

#[test]
fn failed_migration_never_creates_or_replaces_output() {
    let output_path = temp_file_containing(b"sentinel");
    let output = run_invalid_migration(&output_path);
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(std::fs::read(output_path).unwrap(), b"sentinel");
}
```

Also test every command name, success/failure exit mapping, compact/pretty one-document JSON, symlink escape (skip only when Windows symlink creation is unavailable), `..`, absolute catalog resource path, remote reference, oversized file, home-path redaction, payload-secret redaction, output different from input, refusal to overwrite an existing output, and no leftover temp file after failure.

- [ ] **Step 2: Run CLI tests to prove RED**

```powershell
cargo +1.97.1 test -p graphhelm-cli --test schema_cli --locked
```

Expected: Clap rejects the `schema` top-level command.

- [ ] **Step 3: Add the exact command tree**

```text
graphhelm schema catalog --catalog PATH
graphhelm schema check --baseline PATH --candidate PATH
graphhelm schema migrate --catalog PATH --migration PATH --input PATH --output PATH
graphhelm schema conformance --catalog PATH --fixtures PATH
graphhelm schema view --catalog PATH --schema NAME
```

Add `TopLevel::Schema(SchemaArgs)` without modifying existing graph variants. Each module returns the existing `Outcome`; domain diagnostics use exit `2`, redacted I/O/internal failures use exit `4`.

- [ ] **Step 4: Implement bounded, confined loading in `io.rs`**

Open each caller path without printing its canonical form, read at most limit plus one byte, and parse JSON. Determine the repository root by walking catalog ancestors upward and selecting the nearest ancestor whose direct `schemas/` child contains the catalog path; fail if no such ancestor exists. Resolve every catalog/fixture path as repository-relative from that root. Normalize `/`; reject absolute, drive-prefixed, URL, empty, and `..` resource paths. Canonicalize existing parent/resource paths and require `starts_with(repository_root)` to reject symlink escape. Track aggregate bytes before parsing the next resource.

- [ ] **Step 5: Orchestrate catalog/check/conformance/view**

`catalog` validates and returns release version/count/digests. `check` loads both catalogs, compares them, builds `ReleaseEvidence` from migration manifests, `conformance/manifest.json`, and `schemas/CHANGELOG.md`, then enforces release policy. `conformance` loads only manifest-declared confined fixtures. `view` returns the pure generated view. No command scans Git history or makes network requests.

- [ ] **Step 6: Orchestrate migration and atomic output**

Require input/output to be distinct after parent canonicalization and reject an output path that already exists. The supplied catalog must match the manifest target version/hash; load the source schema from the exact immutable `schemas/releases/{fromVersion}/catalog.json` under the same repository root. Compile source and target offline validators, call the pure migration API, serialize only the complete result to a uniquely named sibling opened with `create_new`, call `sync_all`, then rename it to the still-absent destination. Remove only a temporary file whose exact validated sibling path was created by this command. Stdout contains schema/from/to and source/output digests, not the document. Multi-hop chain planning remains a library/conformance/release-gate capability; one CLI invocation applies the one explicit manifest passed by `--migration`.

- [ ] **Step 7: Verify existing and new CLI behavior**

```powershell
cargo +1.97.1 test -p graphhelm-cli --test schema_cli --locked
cargo +1.97.1 test -p graphhelm-cli --test cli_smoke --locked
cargo +1.97.1 clippy -p graphhelm-cli --all-targets --locked -- -D warnings
```

Expected: both test binaries pass; all pre-existing Foundation commands retain outputs and exit codes.

- [ ] **Step 8: Commit the adapter/CLI**

```powershell
git add apps/cli
git commit -m "feat(cli): expose schema evolution commands" -m "Refs #3" -m "Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

---

### Task 9: Publish the immutable 1.0.0 schema release

**Files:**
- Modify: all nine `schemas/*.schema.json`
- Create: `schemas/catalog.json`
- Create: `schemas/CHANGELOG.md`
- Create: all nine `schemas/releases/1.0.0/*.schema.json`
- Create: `schemas/releases/1.0.0/catalog.json`
- Create directories only when populated by a real future breaking release: `schemas/migrations/<schema>/`
- Add tests to: `core/schema-evolution/tests/catalog_integrity.rs`

**Interfaces:** Establish the current and immutable `1.0.0` package with byte-independent canonical hashes and no wire-format change beyond the registered annotation.

- [ ] **Step 1: Add a RED repository-release test**

```rust
#[test]
fn checked_in_current_and_1_0_0_release_are_complete_and_equivalent() {
    let current = load_repo_catalog("schemas/catalog.json");
    let release = load_repo_catalog("schemas/releases/1.0.0/catalog.json");
    assert_eq!(current.catalog.schemas.len(), 9);
    assert!(validate_catalog(&current).ok);
    assert!(validate_catalog(&release).ok);
    assert_release_equivalent(&current, &release);
}
```

- [ ] **Step 2: Run to prove RED**

```powershell
cargo +1.97.1 test -p graphhelm-schema-evolution --test catalog_integrity checked_in --locked
```

Expected: catalogs and snapshots do not exist.

- [ ] **Step 3: Add version metadata to all current schemas**

Add exactly `"x-graphhelm-schema-version": "1.0.0"` immediately after each `$id`. Do not alter another validation keyword, `$id`, `$ref`, discriminator, or formatting-dependent meaning.

- [ ] **Step 4: Create immutable snapshots and catalogs with real digests**

Copy each versioned current schema to `schemas/releases/1.0.0/`. Compute each digest through the library canonicalization (a focused test/helper may print computed catalog entries during development), then write real lowercase `sha256:` values into both catalogs. Root catalog paths are `schemas/<name>.schema.json`; release catalog paths are `schemas/releases/1.0.0/<name>.schema.json`. Do not commit fabricated or placeholder hashes.

- [ ] **Step 5: Create the initial changelog**

`schemas/CHANGELOG.md` contains `## [1.0.0] - 2026-08-08` and records the initial publication of all nine contracts, preservation of `p50.dev` IDs, and absence of data migrations. Do not create empty migration manifests/directories.

- [ ] **Step 6: Verify catalog/snapshot equivalence**

Assert equal key sets, IDs, document versions, and canonical digests while allowing packaging paths to differ. Compile both complete resource sets offline and validate every conformance schema instance against both.

- [ ] **Step 7: Verify and commit the release package**

```powershell
cargo +1.97.1 test -p graphhelm-schema-evolution --test catalog_integrity --locked
cargo +1.97.1 test -p graphhelm-schema-evolution --test conformance --locked
cargo +1.97.1 run --locked -p graphhelm-cli -- schema catalog --catalog schemas/catalog.json
cargo +1.97.1 run --locked -p graphhelm-cli -- schema check --baseline schemas/releases/1.0.0/catalog.json --candidate schemas/catalog.json
git add schemas core/schema-evolution/tests/catalog_integrity.rs
git commit -m "feat(schema): publish immutable protocol release 1.0.0" -m "Refs #3" -m "Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

---

### Task 10: Add cross-platform gates, operator documentation, and final review

**Files:**
- Modify: `.github/workflows/ci.yml`
- Create: `docs/milestones/protocols-and-schema-evolution.md`
- Modify: `README.md`

**Interfaces:** Make catalog, compatibility, conformance, formatting, lint, build, and CLI checks mandatory on Windows/Linux and document verified contracts.

- [ ] **Step 1: Run the future CI gate locally before editing the workflow**

```powershell
cargo +1.97.1 fmt --all -- --check
cargo +1.97.1 clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo +1.97.1 test --workspace --all-features --locked
cargo +1.97.1 test -p graphhelm-cli --test cli_smoke --locked
cargo +1.97.1 test -p graphhelm-cli --test schema_cli --locked
cargo +1.97.1 run --locked -p graphhelm-cli -- schema catalog --catalog schemas/catalog.json
cargo +1.97.1 run --locked -p graphhelm-cli -- schema check --baseline schemas/releases/1.0.0/catalog.json --candidate schemas/catalog.json
cargo +1.97.1 run --locked -p graphhelm-cli -- schema conformance --catalog schemas/catalog.json --fixtures conformance/manifest.json
cargo +1.97.1 metadata --locked --no-deps --format-version 1
git diff --check
```

Expected: any incomplete gate fails visibly. Fix failures through a focused RED test before proceeding.

- [ ] **Step 2: Extend the existing Windows/Linux workflow**

Keep `actions/checkout@v6`, read-only permissions, pinned Rust, formatting, Clippy, workspace tests, CLI smoke, and locked metadata. Add named schema catalog, baseline compatibility, conformance, and schema CLI test steps using only checked-in files. Rename the workflow to `GraphHelm CI`. No credentials, service containers, Docker, Git history, or network retrieval are permitted.

- [ ] **Step 3: Document the shipped protocol release**

Write `docs/milestones/protocols-and-schema-evolution.md` with crate boundaries, catalog contract, exact limits, compatibility matrix, SemVer rules, migration transaction/chain rules, conformance format, CLI commands/exit codes, security controls, immutable snapshot policy, rollback, and acceptance evidence. Update README status/commands and link the milestone doc without claiming future releases or migrations exist.

- [ ] **Step 4: Run migration CLI acceptance without risking repository files**

Use a `tempfile`-created directory inside the acceptance test, a test-only 1.0.0-to-2.0.0 catalog/manifest, and an explicit output path. Assert the output validates, stdout contains only metadata/digests, and failure leaves a pre-existing destination byte-for-byte unchanged. Do not write smoke output under `schemas/`.

- [ ] **Step 5: Run the full verification gate again**

```powershell
cargo +1.97.1 fmt --all -- --check
cargo +1.97.1 clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo +1.97.1 test --workspace --all-features --locked
cargo +1.97.1 test -p graphhelm-cli --test cli_smoke --locked
cargo +1.97.1 test -p graphhelm-cli --test schema_cli --locked
cargo +1.97.1 run --locked -p graphhelm-cli -- schema catalog --catalog schemas/catalog.json
cargo +1.97.1 run --locked -p graphhelm-cli -- schema check --baseline schemas/releases/1.0.0/catalog.json --candidate schemas/catalog.json
cargo +1.97.1 run --locked -p graphhelm-cli -- schema conformance --catalog schemas/catalog.json --fixtures conformance/manifest.json
cargo +1.97.1 metadata --locked --no-deps --format-version 1
git diff --check
git status --short
```

Expected: all commands exit `0`; the catalogs report nine schemas; baseline comparison reports no change; all conformance cases pass; status shows only intended documentation/workflow changes before commit.

- [ ] **Step 6: Commit CI and milestone evidence**

```powershell
git add .github/workflows/ci.yml docs/milestones/protocols-and-schema-evolution.md README.md
git commit -m "ci(schema): gate protocol evolution" -m "Refs #3" -m "Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

- [ ] **Step 7: Request independent security-first review**

Invoke `superpowers:requesting-code-review`. Review specification compliance, path/symlink confinement, bounded resource use, offline `$ref`, conservative classification, hash trust, patch pointer/index correctness, clone atomicity, output replacement/rollback, payload/path redaction, and Windows/Linux parity. Address every critical/important finding through RED/GREEN tests and rerun the full gate.

- [ ] **Step 8: Verify before declaring completion**

Invoke `superpowers:verification-before-completion`, then run:

```powershell
git status --short --branch
git log --oneline origin/main..HEAD
git diff --check origin/main..HEAD
git diff --stat origin/main..HEAD
gh issue view 3 --repo stabem/GraphHelm
```

Expected: clean scoped branch, all intended commits present, no placeholder artifacts, independent review clear, and issue #3 open until a PR containing `Closes #3` is squash-merged.

## Self-review traceability

| Acceptance requirement | Owning task/test |
|---|---|
| All nine schemas versioned/cataloged/snapshotted | Task 9 repository release test |
| Canonical whitespace-insensitive SHA-256 | Task 1 determinism/property tests |
| Catalog key/ID/version/path/hash integrity | Task 2 catalog tests, Task 9 current/snapshot parity |
| Offline-only references and all schemas compile | Task 2 registry tests, Task 9 full resource compilation |
| Annotation/compatible/breaking/unknown classification | Task 3 matrix and fixtures |
| Exact document and aggregate SemVer | Task 4 release gate tests |
| Breaking migration + fixture pair + changelog | Task 4 missing-evidence tests |
| Declarative bounded RFC 6902 subset | Task 5 operation/bound tests |
| Source/hash/destination validation and atomicity | Task 5 migration tests, Task 8 CLI tests |
| Chain cycle/gap/downgrade rejection | Task 5 chain tests/fixtures |
| Valid and invalid public instance for every schema | Task 6 eighteen schema fixtures |
| Deterministic conformance report | Task 6 order-independence test |
| Deterministic generated schema views | Task 7 view tests |
| Confined paths, symlink defense, bounded reads | Task 8 adapter acceptance tests |
| Atomic explicit output and payload/path redaction | Task 8 migration CLI tests |
| JSON-only CLI and stable exit codes | Task 8 `schema_cli.rs` |
| Existing Foundation behavior preserved | Tasks 2, 8, and 10 full workspace/CLI gates |
| Windows/Linux enforcement | Task 10 GitHub Actions matrix |
| Independent security/code review | Task 10 review and verification steps |

Every design acceptance criterion has a concrete owner and automated test. Tasks 1–7 keep domain logic deterministic and in-memory; Task 8 is the only new filesystem boundary; Task 9 publishes only the real initial release; Task 10 makes the completed behavior enforceable. Later Harness Compiler, Governor, Runtime, Context, agent, model, sandbox, Dreams, and Studio milestones remain outside this plan.
