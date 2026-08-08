# Foundation Graph Kernel Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the smallest production-quality offline kernel proving GraphHelm can load, validate, version, hash, lint, govern, mutate, audit, simulate, and replay execution graphs through a machine-readable CLI.

**Architecture:** A pinned Rust workspace separates wire/domain contracts, in-memory schema validation, pure graph semantics, deterministic policies, append-only events/projections, transactional governance, simulation, and CLI presentation. Core crates use dependency inversion and injected clock/ID sources; every accepted mutation is represented by a new immutable `GraphVersion` and a committed ordered event batch.

**Tech Stack:** Rust 1.97.1 (edition 2024), Serde/JSON/YAML, JSON Schema draft 2020-12, SHA-256, cross-platform advisory file locking, Clap, property tests, and GitHub Actions on Windows/Linux.

## Global Constraints

- Track all implementation in GitHub issue `#1` and branch/worktree `feat/foundation-graph-kernel`.
- Preserve checked-in `p50.dev` wire identifiers and use the existing files under `schemas/` as canonical contracts.
- Pin Rust `1.97.1`; use `rustfmt` and Clippy; deny warnings in CI.
- Pin direct dependencies exactly: `serde 1.0.229`, `serde_json 1.0.151`, `serde_yaml_ng 0.10.0`, `jsonschema 0.49.2`, `sha2 0.11.0`, `hex 0.4.3`, `chrono 0.4.45`, `uuid 1.24.0`, `thiserror 2.0.19`, `fs2 0.4.3`, `clap 4.6.4`, `proptest 1.11.0`, `assert_cmd 2.2.2`, `predicates 3.1.4`, and `tempfile 3.27.0`.
- Build JSON Schema validators only from in-memory checked-in resources; disable `jsonschema` default network/file retrieval features.
- Tests and CLI acceptance runs require no internet, model, provider credential, Docker, browser, shell execution by the graph, or production access.
- Use private fields and read-only getters for `GraphVersion`; draft application returns a new value and never mutates the base.
- The semantic hash is SHA-256 over canonical UTF-8 JSON. Include `apiVersion`, `kind`, semantic metadata labels, all of `spec` except node `ui`, versioned policy/schema references, expressions, and permitted unknown operational fields. Exclude metadata identity/history/presentation (`id`, `name`, `executionId`, `version`, `basedOn`, `createdAt`, `createdBy`, `mutationId`, `description`, `annotations`) and every node-level `ui` subtree.
- Owner waivers apply only to logical quality obligations. Missing deploy targets, hard deny policies, invalid schemas, unknown graph references, uncontrolled cycles, impossible compensation, and stale base versions/hashes remain blockers.
- Default CLI output is one valid JSON document. Domain failures use non-zero exit code `2`, concurrency/policy application failures use `3`, and I/O/internal failures use `4`; Clap usage errors retain exit code `2` with Clap's own usage output.
- No empty future modules, provider adapters, Runtime API, Studio, database adapter, sandbox, agent runtime, Context Compiler, Knowledge Graph, Tool Broker, Dreams, or deployment execution.

---

## Locked file and dependency map

```text
Cargo.toml
Cargo.lock
rust-toolchain.toml
rustfmt.toml
.github/workflows/ci.yml
core/protocols/
  Cargo.toml
  src/{lib,actor,diagnostic,graph,policy,draft,event,simulation}.rs
  tests/wire_roundtrip.rs
core/schema/
  Cargo.toml
  src/{lib,document,registry}.rs
  tests/canonical_examples.rs
core/graph/
  Cargo.toml
  src/{lib,canonical,version}.rs
  src/lint/{mod,reachability,cycle,binding,security,deployment,budget}.rs
  tests/{canonical_hash,lint_rules,immutability}.rs
core/policy/
  Cargo.toml
  src/{lib,evaluator}.rs
  tests/obligations.rs
core/events/
  Cargo.toml
  src/{lib,store,jsonl,projection}.rs
  tests/{append_only,replay}.rs
core/governor/
  Cargo.toml
  src/{lib,candidate,apply}.rs
  tests/draft_application.rs
core/simulation/
  Cargo.toml
  src/{lib,engine,fixtures}.rs
  tests/deterministic_simulation.rs
apps/cli/
  Cargo.toml
  src/{main,args,output}.rs
  src/commands/{mod,validate,lint,hash,simulate,draft,replay}.rs
  tests/cli_smoke.rs
tests/fixtures/invalid/*.yaml
tests/fixtures/drafts/*.yaml
tests/fixtures/simulation/*.json
docs/milestones/foundation-graph-kernel.md
```

Dependency direction is fixed:

```text
protocols <- schema
protocols <- graph <- policy
protocols <- events
protocols + schema + graph + policy + events <- governor
protocols + graph + events <- simulation
all completed core crates <- cli
```

No reverse dependency and no cycle is allowed. `apps/cli` contains serialization/presentation and orchestration only.

## Public interfaces fixed by this plan

```rust
pub trait Clock: Send + Sync {
    fn now(&self) -> chrono::DateTime<chrono::Utc>;
}

pub trait IdGenerator: Send + Sync {
    fn next_id(&self, prefix: &'static str) -> String;
}

pub struct GraphVersion {
    graph: ExecutionGraph,
    predecessor: Option<GraphVersionRef>,
    semantic: serde_json::Value,
    content_hash: SemanticHash,
    created_by: Actor,
    created_at: chrono::DateTime<chrono::Utc>,
}
impl GraphVersion {
    pub fn publish(
        graph: ExecutionGraph,
        predecessor: Option<GraphVersionRef>,
        actor: Actor,
        created_at: chrono::DateTime<chrono::Utc>,
    ) -> Result<Self, GraphError>;
    pub fn graph(&self) -> &ExecutionGraph;
    pub fn number(&self) -> u64;
    pub fn predecessor(&self) -> Option<&GraphVersionRef>;
    pub fn semantic(&self) -> &serde_json::Value;
    pub fn content_hash(&self) -> &SemanticHash;
    pub fn to_record(&self) -> GraphVersionRecord;
    pub fn from_record(record: GraphVersionRecord) -> Result<Self, GraphError>;
}

pub fn load_graph(path: &std::path::Path) -> Result<LoadedGraph, Vec<Diagnostic>>;
pub fn validate_waiver(value: &serde_json::Value, source: &str) -> Vec<Diagnostic>;
pub fn lint(graph: &ExecutionGraph, source: &str) -> LintReport;
pub fn canonicalize(graph: &ExecutionGraph) -> Result<CanonicalGraph, GraphError>;
pub fn evaluate_transition(
    base: &GraphVersion,
    candidate: &ExecutionGraph,
    manual_override: Option<&ManualOverride>,
) -> PolicyReport;

pub trait EventStore: Send + Sync {
    fn append_batch(
        &self,
        stream_id: &str,
        expected_next_sequence: u64,
        events: &[NewEvent],
    ) -> Result<Vec<EventEnvelope>, EventStoreError>;
    fn read_stream(&self, stream_id: &str) -> Result<Vec<EventEnvelope>, EventStoreError>;
}

pub fn replay(events: &[EventEnvelope]) -> Result<ExecutionProjection, ReplayError>;
pub fn analyze_draft(base: &GraphVersion, draft: &GraphDraft) -> DraftAnalysis;
pub fn apply_draft(
    base: &GraphVersion,
    draft: &GraphDraft,
    services: &ApplyServices<'_>,
) -> Result<ApplyResult, ApplyError>;
pub fn simulate(
    graph: &GraphVersion,
    fixtures: &SimulationFixtures,
    services: &SimulationServices<'_>,
) -> Result<SimulationResult, SimulationError>;
```

## Stable diagnostic catalog

| Code | Severity | Meaning and primary path |
|---|---|---|
| `GHS001_PARSE` | error | YAML/JSON parse failure; nearest parser path or `/`. |
| `GHS002_SCHEMA` | error | JSON Schema violation; validator instance JSON Pointer. |
| `GHS003_TYPED` | error | schema-valid document cannot enter the stable typed subset; typed field pointer. |
| `GHG001_ENTRYPOINT_UNKNOWN` | error | entrypoint is absent from nodes; `/spec/entrypoints/{index}`. |
| `GHG002_EDGE_SOURCE_UNKNOWN` | error | edge source absent; `/spec/edges/{index}/from`. |
| `GHG003_EDGE_TARGET_UNKNOWN` | error | edge target absent; `/spec/edges/{index}/to`. |
| `GHG004_EDGE_ID_DUPLICATE` | error | duplicate edge ID; `/spec/edges/{index}/id`. |
| `GHG005_NO_TERMINAL_PATH` | error | reachable node cannot reach declared/implicit terminal; `/spec/nodes/{escaped-id}`. |
| `GHG006_UNCONTROLLED_CYCLE` | error | cyclic SCC has no positive `loop.maxIterations`; `/spec/nodes/{escaped-id}/loop`. |
| `GHG007_BINDING_SOURCE_UNKNOWN` | error | `outputs.{node}.*` binding names absent node; exact binding pointer. |
| `GHG008_INLINE_SECRET` | error | prohibited literal secret-shaped field/value; exact value pointer. |
| `GHG009_DEPLOY_TARGET_MISSING` | error | deploy lacks non-empty `targetRef`; `/spec/nodes/{id}/targetRef`. |
| `GHG010_COMPENSATION_MISSING` | error | reversible/compensation-required effect lacks valid node; `/spec/nodes/{id}/effects/compensationNode`. |
| `GHG011_NODE_BUDGET_EXCEEDED` | error | node count exceeds `maxNodes`; `/spec/budgets/maxNodes`. |
| `GHG012_DEPTH_BUDGET_EXCEEDED` | error | longest acyclic path exceeds `maxDepth`; `/spec/budgets/maxDepth`. |
| `GHG013_RETRY_BUDGET_EXCEEDED` | error | node attempts exceed graph retry budget; node retry pointer. |
| `GHG014_HARD_POLICY_DENIED` | error | supported inline hard deny conflicts with graph; policy pointer. |
| `GHG101_DEFAULT_TIMEOUT` | warning | executable node relies on runtime default timeout; node pointer. |
| `GHP001_STRUCTURAL_IMPOSSIBILITY` | error | policy obligation is impossible and non-waivable. |
| `GHP002_OVERRIDE_REQUIRED` | error | logical obligation is unsatisfied without complete owner override. |
| `GHD001_STALE_VERSION` | error | draft expected version differs from base. |
| `GHD002_STALE_HASH` | error | draft expected hash differs from base. |
| `GHD003_OPERATION_INVALID` | error | typed operation/path/value is invalid. |
| `GHE001_SEQUENCE_CONFLICT` | error | expected next event sequence differs from store. |
| `GHE002_CORRUPT_BATCH` | error | committed event batch has invalid JSON/checksum/order. |
| `GHSIM001_UNKNOWN_CONDITION` | error | simulation cannot evaluate supported condition; node pauses rather than guesses. |

JSON Pointer segments must escape `~` as `~0` and `/` as `~1`.

---

### Task 1: Create the isolated execution worktree

**Files:** None.

**Interfaces:** This task produces the clean branch/worktree used by every later task.

- [ ] **Step 1: Confirm the planning commit and clean main checkout**

Run:

```powershell
git status --short --branch
git log -3 --oneline --decorate
git rev-parse main
```

Expected: `main` is clean, includes the committed `AGENTS.md` and both plan files, and tracks `origin/main`.

- [ ] **Step 2: Read and invoke the worktree skill**

Read `superpowers:using-git-worktrees` completely and follow its directory-selection, ignore, and baseline-test checks.

- [ ] **Step 3: Create the mandated branch/worktree**

Run from `F:\github\GraphHelm` after the worktree skill selects the repository-local location:

```powershell
git worktree add .worktrees/foundation-graph-kernel -b feat/foundation-graph-kernel main
git -C .worktrees/foundation-graph-kernel status --short --branch
```

Expected: branch is `feat/foundation-graph-kernel`, status is clean, and no implementation file exists yet.

- [ ] **Step 4: Install the pinned local toolchain because the preflight found Rust absent**

Install `rustup` from the verified Windows Package Manager package, refresh the current process path, and install the pinned toolchain:

```powershell
winget install --id Rustlang.Rustup --exact --source winget --accept-source-agreements --accept-package-agreements
$env:Path = "$env:USERPROFILE\.cargo\bin;$env:Path"
rustup toolchain install 1.97.1 --profile minimal --component rustfmt clippy
rustc +1.97.1 --version
cargo +1.97.1 --version
```

Expected: both tools report `1.97.1`; installation does not alter the repository.

---

### Task 2: Establish the Rust workspace and stable protocol types

**Files:**
- Create: `Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml`, `rustfmt.toml`
- Create: all `Cargo.toml` files in the locked file map
- Create: `core/protocols/src/{lib,actor,diagnostic,graph,policy,draft,event,simulation}.rs`
- Test: `core/protocols/tests/wire_roundtrip.rs`
- Modify: `.gitignore`

**Interfaces:**
- Consumes: checked-in Graph/Node/Edge/Waiver schemas and normative state/event names.
- Produces: `Actor`, `Diagnostic`, `ExecutionGraph`, `GraphNode`, `GraphEdge`, `GraphVersionRef`, `GraphVersionRecord`, `SemanticHash`, `ManualOverride`, `GraphDraft`, `DraftOperation`, `PolicyObligation`, `PolicyWaiver`, `EventEnvelope`, `EventKind`, `NodeState`, `SimulationStatus`, `Clock`, and `IdGenerator`.

- [ ] **Step 1: Add workspace manifests and a failing wire-roundtrip test**

Root `Cargo.toml` must declare resolver `3`, edition `2024`, all seven core members plus CLI, exact workspace dependency pins, and release/test profiles. Use `default-features = false` for `jsonschema` and only `derive` for Clap/Serde where needed.

Add this first test shape before protocol implementations:

```rust
use graphhelm_protocols::{ExecutionGraph, NodeState};

#[test]
fn canonical_graph_wire_shape_round_trips() {
    let source = include_str!("../../../examples/graphs/manual-override-deploy.yaml");
    let value: serde_json::Value = serde_yaml_ng::from_str(source).unwrap();
    let graph: ExecutionGraph = serde_json::from_value(value).unwrap();
    assert_eq!(graph.metadata.version, 13);
    assert_eq!(graph.spec.nodes["deploy"].node_type.as_str(), "deploy");
    assert_eq!(serde_json::to_value(NodeState::WaitingCapacity).unwrap(), "waiting_capacity");
}
```

- [ ] **Step 2: Run the test to prove RED**

Run:

```powershell
cargo +1.97.1 test -p graphhelm-protocols --test wire_roundtrip --locked
```

Expected: compilation fails because `ExecutionGraph` and `NodeState` are not defined.

- [ ] **Step 3: Implement the typed stable subset and forward-compatible storage**

Use `BTreeMap<String, serde_json::Value>` for schema-permitted unknown metadata/node properties. Use tagged enums with exact wire names. Required shapes:

```rust
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionGraph {
    pub api_version: String,
    pub kind: String,
    pub metadata: GraphMetadata,
    pub spec: GraphSpec,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct GraphNode {
    #[serde(rename = "type")]
    pub node_type: NodeType,
    pub name: String,
    pub objective: String,
    pub optionality: Optionality,
    #[serde(flatten)]
    pub properties: std::collections::BTreeMap<String, serde_json::Value>,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeState {
    Draft, Linting, Ready, Queued, Running, WaitingInput, WaitingCapacity,
    Paused, Blocked, Succeeded, Failed, Waived, Skipped, Cancelled, Invalidated,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphVersionRecord {
    pub graph: ExecutionGraph,
    pub predecessor: Option<GraphVersionRef>,
    pub semantic: serde_json::Value,
    pub content_hash: SemanticHash,
    pub created_by: Actor,
    pub created_at: chrono::DateTime<chrono::Utc>,
}
```

Define every public type in the interface list with rustdoc explaining wire stability and override limits. `GraphDraft` uses `#[serde(tag = "op", rename_all = "camelCase")]` and supports only `addNode`, `removeNode`, `patchNode`, `addEdge`, and `removeEdge`.

- [ ] **Step 4: Add negative wire tests**

Assert unknown `NodeType` fails, unknown node fields survive roundtrip, `policy-waiver.schema.json` field names map correctly, and every normative node/event/state string serializes exactly.

- [ ] **Step 5: Run protocol tests and formatting**

Run:

```powershell
cargo +1.97.1 fmt --all
cargo +1.97.1 test -p graphhelm-protocols --locked
cargo +1.97.1 clippy -p graphhelm-protocols --all-targets --locked -- -D warnings
```

Expected: all protocol tests pass with zero warnings.

- [ ] **Step 6: Commit the independently buildable protocol boundary**

```powershell
git add Cargo.toml Cargo.lock rust-toolchain.toml rustfmt.toml .gitignore core/protocols core/*/Cargo.toml apps/cli/Cargo.toml
git commit -m "feat(protocols): define foundation wire contracts"
```

---

### Task 3: Load YAML/JSON and validate checked-in schemas offline

**Files:**
- Create: `core/schema/src/{lib,document,registry}.rs`
- Test: `core/schema/tests/canonical_examples.rs`
- Create fixtures: `tests/fixtures/invalid/schema-missing-kind.yaml`, `tests/fixtures/invalid/schema-invalid-edge.yaml`

**Interfaces:**
- Consumes: `ExecutionGraph`, `Diagnostic`.
- Produces: `LoadedGraph { source: String, raw: Value, graph: ExecutionGraph }`, `load_graph`, `validate_graph_value`, and `validate_waiver`.

- [ ] **Step 1: Write tests that require all canonical examples and stable schema diagnostics**

```rust
#[test]
fn all_canonical_graph_examples_validate_offline() {
    for path in [
        "../../examples/graphs/software-feature.yaml",
        "../../examples/graphs/manual-override-deploy.yaml",
        "../../examples/graphs/research-to-publish.yaml",
    ] {
        let loaded = graphhelm_schema::load_graph(path.as_ref()).unwrap_or_else(|d| panic!("{path}: {d:?}"));
        assert_eq!(loaded.graph.kind, "ExecutionGraph");
    }
}

#[test]
fn invalid_edge_reports_schema_code_pointer_and_source() {
    let diagnostics = graphhelm_schema::load_graph(
        "../../tests/fixtures/invalid/schema-invalid-edge.yaml".as_ref(),
    ).unwrap_err();
    assert!(diagnostics.iter().any(|d| d.code == "GHS002_SCHEMA"
        && d.path == "/spec/edges/0/type"
        && d.source.ends_with("schema-invalid-edge.yaml")));
}
```

- [ ] **Step 2: Run schema tests to prove RED**

```powershell
cargo +1.97.1 test -p graphhelm-schema --test canonical_examples --locked
```

Expected: failure because the loader and validator registry do not exist.

- [ ] **Step 3: Implement format detection and bounded reading**

Read at most 4 MiB. Select JSON only for `.json`; select YAML for `.yaml`/`.yml`; unsupported extensions return `GHS001_PARSE`. Parse into `serde_json::Value`, validate, then deserialize to `ExecutionGraph`. Sort diagnostics by `(path, code, message)`.

- [ ] **Step 4: Build a network-free JSON Schema registry**

Embed `graph.schema.json`, `node.schema.json`, `edge.schema.json`, `agent.schema.json`, and `policy-waiver.schema.json` using `include_str!`. Construct `jsonschema::Registry`, register each canonical `$id` URI, let the relative node/edge references resolve against the graph schema's `https://p50.dev/schemas/graph.schema.json` base, call `jsonschema::options().with_registry(&registry).should_validate_formats(true).build(&graph_schema)`, and expose only in-memory validators. No retriever implementation may access filesystem or HTTP.

- [ ] **Step 5: Map every validation failure into stable diagnostics**

Use `ValidationError::instance_path()` for the JSON Pointer, `GHS002_SCHEMA`, the caller-provided source, and a concise message without schema internals. Map typed-deserialization failures to `GHS003_TYPED`.

- [ ] **Step 6: Run canonical, negative, and no-network tests**

Add a test with an intentionally unknown `$ref` in a test-only registry and assert validator construction fails locally without a network attempt. Then run:

```powershell
cargo +1.97.1 test -p graphhelm-schema --locked
cargo +1.97.1 clippy -p graphhelm-schema --all-targets --locked -- -D warnings
```

Expected: three canonical graphs pass; invalid fixtures return stable code/path/source; no network is used.

- [ ] **Step 7: Commit schema loading**

```powershell
git add core/schema tests/fixtures/invalid/schema-*.yaml
git commit -m "feat(schema): validate graph documents offline"
```

---

### Task 4: Canonicalize, hash, and publish immutable Graph Versions

**Files:**
- Create: `core/graph/src/{lib,canonical,version}.rs`
- Test: `core/graph/tests/{canonical_hash,immutability}.rs`
- Create fixtures: `tests/fixtures/invalid/hash-ui-moved.yaml`, `tests/fixtures/invalid/hash-edge-changed.yaml`

**Interfaces:**
- Consumes: `ExecutionGraph`, `Actor`, `GraphVersionRef`, `SemanticHash`.
- Produces: `CanonicalGraph { value, bytes }`, `canonicalize`, `semantic_hash`, private-field `GraphVersion`, and checked conversion to/from the persisted `GraphVersionRecord` wire type.

- [ ] **Step 1: Write golden and immutability tests first**

```rust
#[test]
fn ui_coordinates_and_map_order_do_not_change_semantic_hash() {
    let base = load("../../examples/graphs/software-feature.yaml");
    let moved = load("../../tests/fixtures/invalid/hash-ui-moved.yaml");
    assert_eq!(semantic_hash(&base).unwrap(), semantic_hash(&moved).unwrap());
}

#[test]
fn operational_edge_change_changes_semantic_hash() {
    let base = load("../../examples/graphs/software-feature.yaml");
    let changed = load("../../tests/fixtures/invalid/hash-edge-changed.yaml");
    assert_ne!(semantic_hash(&base).unwrap(), semantic_hash(&changed).unwrap());
}

#[test]
fn publishing_successor_does_not_mutate_predecessor() {
    let first = publish_version(1);
    let before = serde_json::to_value(first.graph()).unwrap();
    let second = publish_successor(&first);
    assert_eq!(first.number(), 1);
    assert_eq!(serde_json::to_value(first.graph()).unwrap(), before);
    assert_eq!(second.number(), 2);
    assert_eq!(second.predecessor().unwrap().content_hash, *first.content_hash());
}
```

- [ ] **Step 2: Run the focused tests to prove RED**

```powershell
cargo +1.97.1 test -p graphhelm-graph --test canonical_hash --locked
cargo +1.97.1 test -p graphhelm-graph --test immutability --locked
```

Expected: compilation failure because canonicalization and `GraphVersion` are absent.

- [ ] **Step 3: Implement explicit semantic projection**

Construct a new JSON value rather than deleting arbitrary keys from input. Include exactly the semantic fields listed in Global Constraints. Recursively sort every object key, preserve array order, normalize JSON numbers through `serde_json::Number`, reject non-JSON YAML scalars during loading, and serialize compact UTF-8 with `serde_json::to_vec`.

- [ ] **Step 4: Implement SHA-256 and immutable publication**

Use `sha2::Sha256` and lowercase hex prefixed with `sha256:`. `GraphVersion::publish` verifies metadata version and predecessor monotonicity, computes semantic form/hash internally, stores all fields privately, and offers no mutable getter.

- [ ] **Step 5: Add bounded property tests**

Generate maps with shuffled insertion order and random node `ui.position` values; assert hash stability. Generate one operational edge mutation; assert hash changes. Keep each case under 20 nodes and 256 cases.

- [ ] **Step 6: Record and review golden hashes**

Golden-test all three canonical examples. The expected digest literals are written only after inspecting the canonical JSON once; store canonical JSON snapshots under `core/graph/tests/golden/` so a hash change has a human-reviewable semantic diff.

- [ ] **Step 7: Run graph tests and commit**

```powershell
cargo +1.97.1 test -p graphhelm-graph canonical --locked
cargo +1.97.1 test -p graphhelm-graph immutability --locked
cargo +1.97.1 clippy -p graphhelm-graph --all-targets --locked -- -D warnings
git add core/graph tests/fixtures/invalid/hash-*.yaml
git commit -m "feat(graph): canonicalize immutable graph versions"
```

---

### Task 5: Implement deterministic semantic lint

**Files:**
- Create: `core/graph/src/lint/{mod,reachability,cycle,binding,security,deployment,budget}.rs`
- Test: `core/graph/tests/lint_rules.rs`
- Create fixtures: `tests/fixtures/invalid/{entrypoint,unknown-edge,duplicate-edge,no-terminal,uncontrolled-cycle,unknown-binding,inline-secret,deploy-no-target,compensation-missing,node-budget,depth-budget,retry-budget,hard-policy}.yaml`

**Interfaces:**
- Consumes: validated `ExecutionGraph`.
- Produces: `LintReport { errors: Vec<Diagnostic>, warnings: Vec<Diagnostic> }` and `lint`.

- [ ] **Step 1: Add one table-driven failing test per stable code**

```rust
#[test]
fn invalid_fixtures_emit_exact_primary_code_and_path() {
    for (name, code, path) in [
        ("entrypoint.yaml", "GHG001_ENTRYPOINT_UNKNOWN", "/spec/entrypoints/0"),
        ("uncontrolled-cycle.yaml", "GHG006_UNCONTROLLED_CYCLE", "/spec/nodes/a/loop"),
        ("deploy-no-target.yaml", "GHG009_DEPLOY_TARGET_MISSING", "/spec/nodes/deploy/targetRef"),
        ("compensation-missing.yaml", "GHG010_COMPENSATION_MISSING", "/spec/nodes/deploy/effects/compensationNode"),
    ] {
        let report = lint_fixture(name);
        assert!(report.errors.iter().any(|d| d.code == code && d.path == path), "{name}: {report:?}");
    }
}
```

- [ ] **Step 2: Run to prove RED**

```powershell
cargo +1.97.1 test -p graphhelm-graph --test lint_rules --locked
```

Expected: `lint` is missing.

- [ ] **Step 3: Implement reference and duplicate rules**

Build node and edge indexes once. Validate entrypoints, edge endpoints, unique edge IDs, and binding references in edge `map` plus node `input.bindings`. Support only `outputs.{node}.` and `nodes.{node}.output` source syntax; other expression strings remain semantically preserved and are not executed.

- [ ] **Step 4: Implement reachability, terminal paths, and bounded cycles without `petgraph`**

Use iterative DFS/BFS and Tarjan SCC in focused modules. Terminal nodes are declared by `spec.completion.terminalNodes` or implicit out-degree zero. A cyclic SCC is controlled only if at least one member declares `loop.maxIterations` as a positive integer; still enforce graph `maxDepth` on the condensed DAG.

- [ ] **Step 5: Implement security/deploy/compensation rules**

Secret detection inspects key names matching case-insensitive `password`, `secret`, `token`, `api_key`, `apiKey`, `private_key`, or `privateKey`; references beginning `secret://`, `environment://`, `env://`, or ending `Ref` are allowed. Deploy requires non-empty `targetRef`. If `effects.reversible` or `effects.compensationRequired` is true, `effects.compensationNode` must name an existing rollback node.

- [ ] **Step 6: Implement static budget and hard-policy rules**

Enforce node count, condensed-DAG depth, and per-node `retry.maxAttempts <= maxRetriesPerNode + 1`. Support deterministic inline hard denies for `production.deploy`, `deploy`, and `secret.inline`; report the policy array pointer that caused denial.

- [ ] **Step 7: Add warnings and deterministic ordering**

Warn `GHG101_DEFAULT_TIMEOUT` for `agent`, `tool`, `deploy`, `rollback`, and `artifact_transform` nodes without `timeoutSeconds`. Sort errors and warnings independently by path/code/message.

- [ ] **Step 8: Run all lint tests and commit**

```powershell
cargo +1.97.1 test -p graphhelm-graph --test lint_rules --locked
cargo +1.97.1 test -p graphhelm-graph --locked
cargo +1.97.1 clippy -p graphhelm-graph --all-targets --locked -- -D warnings
git add core/graph tests/fixtures/invalid
git commit -m "feat(graph): add deterministic semantic lint"
```

---

### Task 6: Evaluate deterministic policy obligations

**Files:**
- Create: `core/policy/src/{lib,evaluator}.rs`
- Test: `core/policy/tests/obligations.rs`

**Interfaces:**
- Consumes: base `GraphVersion`, candidate `ExecutionGraph`, optional `ManualOverride`, lint diagnostics.
- Produces: `PolicyReport` with ordered `PolicyObligation { requirement, status, evidence, reason, overrideable }` and `evaluate_transition`.

- [ ] **Step 1: Write failing obligation-state tests**

```rust
#[test]
fn removed_review_is_unsatisfied_then_waived_only_by_complete_owner_override() {
    let base = base_with_review_gate();
    let candidate = candidate_without_review_gate();
    let absent = evaluate_transition(&base, &candidate, None);
    assert_eq!(absent.requirement("review").status, ObligationStatus::Unsatisfied);

    let report = evaluate_transition(&base, &candidate, Some(&owner_override("review")));
    assert_eq!(report.requirement("review").status, ObligationStatus::Waived);
}

#[test]
fn missing_deploy_target_is_impossible_even_with_override() {
    let report = evaluate_transition(&base(), &deploy_without_target(), Some(&owner_override("deploy_target")));
    assert_eq!(report.requirement("deploy_target").status, ObligationStatus::Impossible);
    assert!(!report.requirement("deploy_target").overrideable);
}
```

- [ ] **Step 2: Run to prove RED**

```powershell
cargo +1.97.1 test -p graphhelm-policy --test obligations --locked
```

Expected: evaluator symbols are missing.

- [ ] **Step 3: Implement obligation discovery and statuses**

Discover logical obligations from removed base nodes with type `gate` or optionality `required`, explicit `bypassedRequirements`, and supported inline policy requirements. Structural lint failures become `Impossible`. Evidence contains graph/node/diagnostic references, never prose-only success.

- [ ] **Step 4: Validate override completeness**

An override is eligible only when actor type/ID resolves to owner, `reason` is non-empty, each waived requirement is named, acknowledged risks are non-empty, and scope is `node`, `branch`, or `execution`. Unknown requirements remain unsatisfied; hard/structural obligations remain impossible.

- [ ] **Step 5: Exercise the supplied manual override graph**

Load `examples/graphs/manual-override-deploy.yaml`; assert `integration_tests` and `independent_security_review` are representable as waived quality obligations while deploy target is satisfied and result label remains `deployed_without_full_validation`.

- [ ] **Step 6: Run and commit**

```powershell
cargo +1.97.1 test -p graphhelm-policy --locked
cargo +1.97.1 clippy -p graphhelm-policy --all-targets --locked -- -D warnings
git add core/policy
git commit -m "feat(policy): evaluate deterministic graph obligations"
```

---

### Task 7: Append ordered event batches and rebuild projections

**Files:**
- Create: `core/events/src/{lib,store,jsonl,projection}.rs`
- Test: `core/events/tests/{append_only,replay}.rs`

**Interfaces:**
- Consumes: `NewEvent`, `EventEnvelope`, `EventKind`, injected `Clock`/`IdGenerator`.
- Produces: `EventStore`, `JsonlEventStore`, `append_batch`, `read_stream`, `ExecutionProjection`, and `replay`.

- [ ] **Step 1: Write failing append/order/corruption tests**

```rust
#[test]
fn append_batch_preserves_order_and_rejects_stale_sequence() {
    let store = temp_store();
    let written = store.append_batch("exec-1", 1, &events(2)).unwrap();
    assert_eq!(written.iter().map(|e| e.sequence).collect::<Vec<_>>(), vec![1, 2]);
    let error = store.append_batch("exec-1", 1, &events(1)).unwrap_err();
    assert_eq!(error.code(), "GHE001_SEQUENCE_CONFLICT");
}

#[test]
fn replay_reconstructs_published_graph_and_simulation_status() {
    let events = published_and_completed_events();
    let projection = graphhelm_events::replay(&events).unwrap();
    assert_eq!(projection.current_graph.unwrap().number, 2);
    assert_eq!(projection.simulation_status, Some(SimulationStatus::Completed));
}
```

- [ ] **Step 2: Run to prove RED**

```powershell
cargo +1.97.1 test -p graphhelm-events --locked
```

Expected: store and projection implementations are missing.

- [ ] **Step 3: Implement a complete local JSONL batch format**

Each newline-terminated physical line is one committed `StoredBatch` containing `streamId`, `startSequence`, ordered envelopes, and SHA-256 checksum of canonical envelope JSON. Under one `fs2::FileExt::lock_exclusive`, read and validate committed batches, verify expected sequence/idempotency, serialize the entire new batch to one buffer, append one line, flush, call `sync_data`, and unlock. Advisory locking is documented. A non-newline-terminated tail is an uncommitted interrupted append and is ignored/quarantined; invalid JSON/checksum/order in a newline-terminated batch returns `GHE002_CORRUPT_BATCH`.

- [ ] **Step 4: Implement all required event variants**

The exact variants are `GraphImported`, `GraphValidationFailed`, `GraphVersionPublished`, `DraftProposed`, `DraftRejected`, `DraftApplied`, `PolicyObligationEvaluated`, `PolicyWaiverCreated`, `SimulationStarted`, `NodeStateChanged`, and `SimulationCompleted`. `GraphVersionPublished` carries the complete `GraphVersionRecord` so replay needs no graph-crate dependency or external snapshot.

- [ ] **Step 5: Implement pure projection replay**

Verify contiguous sequence and matching execution stream, then fold events into current graph, draft/waiver history, node states, and simulation status. Duplicate idempotency keys with identical content are ignored; conflicting duplicates fail.

- [ ] **Step 6: Test fresh-handle replay and corruption**

Close the first `JsonlEventStore`, construct a new instance on the same path, read/replay, and compare projection JSON. Tamper one checksum and assert replay/store read fails without returning a partial successful projection.

- [ ] **Step 7: Run and commit**

```powershell
cargo +1.97.1 test -p graphhelm-events --locked
cargo +1.97.1 clippy -p graphhelm-events --all-targets --locked -- -D warnings
git add core/events
git commit -m "feat(events): append and replay ordered event batches"
```

---

### Task 8: Apply transactional Graph Drafts and create schema-valid waivers

**Files:**
- Create: `core/governor/src/{lib,candidate,apply}.rs`
- Test: `core/governor/tests/draft_application.rs`
- Create fixtures: `tests/fixtures/drafts/{add-node,remove-review-with-waiver,stale-version,stale-hash,invalid-operation,impossible-deploy}.yaml`

**Interfaces:**
- Consumes: schema validation, `GraphVersion`, lint, policy report, `EventStore`, clock/ID generator.
- Produces: `DraftAnalysis`, `ApplyServices`, `ApplyResult { version, waivers, events, policy_report }`, `analyze_draft`, and `apply_draft`.

- [ ] **Step 1: Write failing immutable/atomic/stale/waiver tests**

```rust
#[test]
fn valid_draft_publishes_n_plus_one_and_preserves_n() {
    let base = base_version();
    let before = serde_json::to_value(base.graph()).unwrap();
    let result = apply_fixture(&base, "add-node.yaml").unwrap();
    assert_eq!(result.version.number(), base.number() + 1);
    assert_eq!(serde_json::to_value(base.graph()).unwrap(), before);
}

#[test]
fn stale_hash_fails_without_publishing_successor() {
    let store = recording_store();
    let error = apply_with_store(&base_version(), "stale-hash.yaml", &store).unwrap_err();
    assert_eq!(error.code(), "GHD002_STALE_HASH");
    assert!(store.events().iter().any(|event| matches!(event.kind, EventKind::DraftRejected(_))));
    assert!(!store.events().iter().any(|event| matches!(event.kind, EventKind::GraphVersionPublished(_) | EventKind::DraftApplied(_))));
}

#[test]
fn bypass_creates_schema_valid_waiver_and_auditable_status() {
    let result = apply_fixture(&base_with_review(), "remove-review-with-waiver.yaml").unwrap();
    assert!(!result.waivers.is_empty());
    assert!(result.waivers.iter().all(waiver_schema_is_valid));
    assert_eq!(result.policy_report.result_status, "completed_with_waivers");
}
```

- [ ] **Step 2: Run to prove RED**

```powershell
cargo +1.97.1 test -p graphhelm-governor --test draft_application --locked
```

Expected: draft analysis/application symbols are absent.

- [ ] **Step 3: Implement ordered operations on an isolated candidate**

Verify base version/hash first. Clone only the `ExecutionGraph`, apply operations in order, require exact semantic paths (`/spec/nodes/{id}` for node operations and edge IDs for edge removal), and reject duplicate additions/missing removals/invalid patch shapes with `GHD003_OPERATION_INVALID`.

- [ ] **Step 4: Run the complete validation pipeline before publication**

Serialize candidate to JSON, validate against the canonical Graph Schema, deserialize, lint, evaluate policies, and stop on any schema/lint/impossible/unsatisfied obligation. Manual override converts only eligible obligations to waived.

- [ ] **Step 5: Generate and validate waivers**

Create one `PolicyWaiver` per waived requirement using injected actor/time/ID, candidate graph version N+1, acknowledged risks, and scope. Validate each serialized waiver with `validate_waiver`; validation failure aborts the entire apply.

- [ ] **Step 6: Commit one event batch, then expose the result**

For success, build ordered events: `DraftProposed`, one `PolicyObligationEvaluated` per obligation, waiver events, `GraphVersionPublished`, and `DraftApplied`; call `append_batch` once. For stale, invalid, lint-blocked, or policy-blocked drafts, append one audit batch containing `DraftProposed`, available obligation events, and `DraftRejected`, but never publication/application/waiver events. Return `ApplyResult` only after the success batch is durable; otherwise the caller retains base as active and no successor is exposed.

- [ ] **Step 7: Prove structural impossibility is not waivable**

The `impossible-deploy.yaml` draft removes/omits deploy `targetRef` while providing owner risks. Assert apply returns `GHP001_STRUCTURAL_IMPOSSIBILITY`, no waiver, no successor, and a rejection audit batch with no publication/application event.

- [ ] **Step 8: Run and commit**

```powershell
cargo +1.97.1 test -p graphhelm-governor --locked
cargo +1.97.1 clippy -p graphhelm-governor --all-targets --locked -- -D warnings
git add core/governor tests/fixtures/drafts
git commit -m "feat(governor): apply transactional graph drafts"
```

---

### Task 9: Simulate graph state transitions without effects

**Files:**
- Create: `core/simulation/src/{lib,engine,fixtures}.rs`
- Test: `core/simulation/tests/deterministic_simulation.rs`
- Create fixtures: `tests/fixtures/simulation/{all-success,unknown-condition,node-failure}.json`

**Interfaces:**
- Consumes: `GraphVersion`, `SimulationFixtures`, `EventStore`, clock/ID generator.
- Produces: `SimulationServices`, `SimulationResult`, and `simulate`.

- [ ] **Step 1: Write deterministic transition and unknown-condition tests**

```rust
#[test]
fn same_graph_and_fixtures_emit_same_ordered_transition_kinds() {
    let first = run_with_fixed_services("all-success.json");
    let second = run_with_fixed_services("all-success.json");
    assert_eq!(first.transition_trace(), second.transition_trace());
    assert_eq!(first.status, SimulationStatus::Completed);
}

#[test]
fn unknown_condition_pauses_instead_of_guessing() {
    let result = run_with_fixed_services("unknown-condition.json");
    assert_eq!(result.status, SimulationStatus::Paused);
    assert_eq!(result.diagnostics[0].code, "GHSIM001_UNKNOWN_CONDITION");
}
```

- [ ] **Step 2: Run to prove RED**

```powershell
cargo +1.97.1 test -p graphhelm-simulation --locked
```

Expected: simulator API is absent.

- [ ] **Step 3: Implement bounded deterministic scheduling**

Use entrypoints, satisfied predecessor edges, and stable node-ID ordering. Emit `queued -> running -> fixture outcome` transitions. Never execute node payloads. Default fixture outcome is success; explicit outcomes are `success`, `failure`, or `unknown`. Bound transitions to `max(4 * node_count, 16)` plus declared finite loop iterations; exceeding the bound returns blocked/no-progress.

- [ ] **Step 4: Implement the supported condition subset**

Evaluate missing condition as true; support literal `true`/`false`, `nodes.{id}.output.passed == true|false`, and fixture-provided condition keys. Any other condition is unknown and follows `onUnknown`; without explicit behavior it pauses.

- [ ] **Step 5: Emit one ordered simulation event batch**

Include `SimulationStarted`, every `NodeStateChanged`, and `SimulationCompleted` with completed/failed/paused status. Append once through `EventStore`; return result only after append.

- [ ] **Step 6: Compare simulation result with replay projection**

Replay the emitted events from a fresh event-store handle and assert node states and terminal status equal `SimulationResult`.

- [ ] **Step 7: Run and commit**

```powershell
cargo +1.97.1 test -p graphhelm-simulation --locked
cargo +1.97.1 clippy -p graphhelm-simulation --all-targets --locked -- -D warnings
git add core/simulation tests/fixtures/simulation
git commit -m "feat(simulation): simulate graph transitions deterministically"
```

---

### Task 10: Expose the machine-readable cross-platform CLI

**Files:**
- Create: `apps/cli/src/{main,args,output}.rs`
- Create: `apps/cli/src/commands/{mod,validate,lint,hash,simulate,draft,replay}.rs`
- Test: `apps/cli/tests/cli_smoke.rs`

**Interfaces:**
- Consumes: all completed core APIs.
- Produces: binary `graphhelm` and exact command tree from the bootstrap prompt.

- [ ] **Step 1: Write failing CLI contract tests**

```rust
#[test]
fn validate_returns_json_and_zero_for_canonical_yaml() {
    let output = command().args(["graph", "validate", "../../examples/graphs/software-feature.yaml"]).output().unwrap();
    assert!(output.status.success());
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["ok"], true);
}

#[test]
fn lint_failure_is_json_and_exit_two() {
    let output = command().args(["graph", "lint", "../../tests/fixtures/invalid/entrypoint.yaml"]).output().unwrap();
    assert_eq!(output.status.code(), Some(2));
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["diagnostics"][0]["code"], "GHG001_ENTRYPOINT_UNKNOWN");
}
```

- [ ] **Step 2: Run to prove RED**

```powershell
cargo +1.97.1 test -p graphhelm-cli --test cli_smoke --locked
```

Expected: binary/commands do not exist.

- [ ] **Step 3: Implement the exact command tree**

```text
graphhelm graph validate FILE
graphhelm graph lint FILE
graphhelm graph hash FILE
graphhelm graph simulate FILE --events PATH [--fixtures PATH]
graphhelm graph draft apply BASE_FILE DRAFT_FILE --actor owner-local --events PATH
graphhelm graph replay --events PATH
```

Use `clap` derive in `args.rs`. Default output is compact JSON; optional `--pretty` is global and only changes whitespace. Paths are never canonicalized into user-home disclosure in output; preserve caller spelling in `source`.

- [ ] **Step 4: Implement one output envelope and exit mapping**

```rust
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandOutput<T> {
    pub ok: bool,
    pub command: &'static str,
    pub data: Option<T>,
    pub diagnostics: Vec<Diagnostic>,
}
```

Print exactly once to stdout. Domain errors still produce valid JSON on stdout; unexpected internal/I/O errors use a redacted JSON envelope and exit 4. Never print Rust debug/backtrace in normal operation.

- [ ] **Step 5: Implement command orchestration**

`validate` runs schema only; `lint` runs schema then lint; `hash` runs schema then canonicalization; `simulate` imports/publishes the graph and appends simulation events; `draft apply` imports/publishes base when the stream is empty and applies via Governor; `replay` reads the stream from a fresh store and emits projection JSON.

- [ ] **Step 6: Test YAML, JSON, invalid schema/lint, draft waiver/impossibility, simulation, and fresh-process replay**

Create a JSON copy of one canonical YAML in the temp test directory. Spawn a separate `graph replay` process after simulate/apply and compare the final projection with the producing command result.

- [ ] **Step 7: Run CLI acceptance and commit**

```powershell
cargo +1.97.1 test -p graphhelm-cli --test cli_smoke --locked
cargo +1.97.1 run --locked -p graphhelm-cli -- graph validate examples/graphs/software-feature.yaml
cargo +1.97.1 run --locked -p graphhelm-cli -- graph hash examples/graphs/manual-override-deploy.yaml
cargo +1.97.1 clippy -p graphhelm-cli --all-targets --locked -- -D warnings
git add apps/cli
git commit -m "feat(cli): expose foundation graph kernel commands"
```

---

### Task 11: Add CI, milestone evidence, and whole-workspace verification

**Files:**
- Create: `.github/workflows/ci.yml`
- Create: `docs/milestones/foundation-graph-kernel.md`
- Modify only if required by implemented commands: `README.md`

**Interfaces:**
- Consumes: every milestone command and test suite.
- Produces: Windows/Linux CI evidence and operator-facing milestone documentation.

- [ ] **Step 1: Add a failing CI parity check locally**

Run the future CI command set before adding/fixing workflow-specific issues:

```powershell
cargo +1.97.1 fmt --all -- --check
cargo +1.97.1 clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo +1.97.1 test --workspace --all-features --locked
cargo +1.97.1 metadata --locked --no-deps --format-version 1
git diff --check
```

Expected: any remaining formatting, warning, test, or lockfile gap fails visibly; record and correct each before CI commit.

- [ ] **Step 2: Create GitHub Actions workflow**

Use `actions/checkout@v6`, `permissions: contents: read`, `timeout-minutes: 20`, matrix `windows-latest` and `ubuntu-latest`, install Rust 1.97.1/rustfmt/clippy with `rustup`, then run format, Clippy, workspace tests, CLI smoke, and metadata. Do not add service containers, secrets, provider tokens, or network-dependent tests.

- [ ] **Step 3: Write milestone documentation from verified behavior**

Document crate boundaries, semantic hash inclusion/exclusion, stable diagnostics, local batch event format, CLI commands/exit codes, supported expression subset, security properties, acceptance matrix, and explicit out-of-scope list. Include actual command output counts only after final verification.

- [ ] **Step 4: Run schema and CLI end-to-end acceptance from a clean event path**

```powershell
Remove-Item -LiteralPath 'target/foundation-smoke-events.jsonl' -ErrorAction SilentlyContinue
cargo +1.97.1 run --locked -p graphhelm-cli -- graph validate examples/graphs/software-feature.yaml
cargo +1.97.1 run --locked -p graphhelm-cli -- graph validate examples/graphs/manual-override-deploy.yaml
cargo +1.97.1 run --locked -p graphhelm-cli -- graph validate examples/graphs/research-to-publish.yaml
cargo +1.97.1 run --locked -p graphhelm-cli -- graph simulate examples/graphs/software-feature.yaml --events target/foundation-smoke-events.jsonl
cargo +1.97.1 run --locked -p graphhelm-cli -- graph replay --events target/foundation-smoke-events.jsonl
```

Expected: every output parses as JSON; three validations succeed; simulation and replay terminal/node projections match.

- [ ] **Step 5: Run the full verification gate**

```powershell
cargo +1.97.1 fmt --all -- --check
cargo +1.97.1 clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo +1.97.1 test --workspace --all-features --locked
cargo +1.97.1 test -p graphhelm-cli --test cli_smoke --locked
cargo +1.97.1 metadata --locked --no-deps --format-version 1
git diff --check
git status --short
```

Expected: all commands exit 0; tests report no failure/ignored acceptance scenario; `git status --short` lists only intended documentation/workflow changes before commit.

- [ ] **Step 6: Commit CI and evidence**

```powershell
git add .github/workflows/ci.yml docs/milestones/foundation-graph-kernel.md README.md
git commit -m "ci: verify foundation graph kernel"
```

If `README.md` did not require a change, omit it from `git add`.

- [ ] **Step 7: Request independent review and verify the review fixes**

Invoke `superpowers:requesting-code-review`. Review specification compliance first, then code quality/security. Address findings through RED/GREEN tests and rerun the full verification gate after the final fix.

- [ ] **Step 8: Final branch review and publication handoff**

Invoke `superpowers:verification-before-completion`, then inspect:

```powershell
git status --short --branch
git log --oneline main..HEAD
git diff --check main..HEAD
git diff --stat main..HEAD
gh issue view 1 --repo stabem/GraphHelm
```

Expected: clean implementation branch, only scoped files, review verdict clear, and issue #1 still open until a PR with `Closes #1` is merged.

## Self-review traceability

| Acceptance requirement | Owning task/test |
|---|---|
| YAML and JSON schema validation; canonical examples | Task 3 `canonical_examples.rs`, Task 10 CLI JSON-copy smoke |
| immutable N/N+1 graph versions | Task 4 `immutability.rs`, Task 8 draft test |
| key/UI hash stability and operational hash change | Task 4 golden/property tests |
| missing entrypoint, cycles, bindings, secrets, deploy target, compensation, budgets, hard policy | Task 5 table-driven lint fixtures |
| deterministic obligations; satisfied/unsatisfied/waived/impossible | Task 6 `obligations.rs` |
| append-only ordered events and fresh-process replay | Task 7 store/replay tests, Task 10 smoke |
| atomic stale/invalid drafts | Task 8 stale/invalid tests and recording store |
| schema-valid waiver and auditable status | Task 8 waiver test using `validate_waiver` |
| impossible deploy blocked despite owner waiver | Task 6 and Task 8 impossibility tests |
| deterministic simulation, unknown condition pause | Task 9 simulation tests |
| machine-readable CLI and non-zero failures | Task 10 `cli_smoke.rs` |
| no internet/credentials/Docker/model/browser | Task 3 in-memory registry, Task 9 effect-free engine, Task 11 CI |
| formatting/lint/types/unit/integration/schema/CLI | Task 11 full gate |

The file/type names above are internally consistent with the dependency map. Every required first-milestone behavior has an owning task and automated test. Later product subsystems remain in the program index and are not represented by empty code in this plan.
