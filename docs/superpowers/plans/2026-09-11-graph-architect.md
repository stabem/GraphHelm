# Graph Architect Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A new crate `core/architect` that turns a goal into a Graph DSL document through one model call plus a bounded repair loop, validated by the same chain authored graphs use, stamped so every synthesized graph is completable, refusing goals outside the runtime's capability catalog — exposed on CLI, HTTP and MCP, and proven end to end with a keyless recorded model (closes #107, #183, #184).

**Architecture:** Pure compiler (`core/architect`: template → `DraftModel` port → parse → stamp customs → `load_graph_json` → `lint` → viability + catalog checks → repair ≤2 → refuse or emit) with metadata owned by the compiler; a `RecordedDraftModel` fixture adapter inside the crate; thin adapters in `apps/cli` over the existing gateway adapters and `ServeModelPort`; three doors that return one JSON.

**Tech Stack:** Rust 1.97.1, serde/serde_json, sha2/hex (workspace), existing crates `graphhelm-schema`, `graphhelm-graph`, `graphhelm-protocols`, `graphhelm-runtime` (for `classify`), `graphhelm-gateway` (for `Usage`). No new external dependencies.

**Spec:** `docs/superpowers/specs/2026-09-11-graph-architect-design.md` (decisions D1–D10; read first).

## Global Constraints

- Toolchain `cargo +1.97.1`, always `--locked`, `CARGO_TARGET_DIR=E:/o-107-target`. Work only in worktree `D:/o-107`, branch `issue-107-graph-architect`.
- No `TODO`/stubs; every public fn behaves in the commit that adds it. English only.
- No schema changes; no new external crates. Add `core/architect` to `Cargo.toml` `[workspace] members` (alphabetical position after `core/tool-broker`? — the list is not alphabetical; append after `"core/tool-broker",`).
- Closing keywords ONLY in the final PR body (`Closes #107`, `Closes #183`, `Closes #184`). Commits use `Refs #N`. Never write `close|closes|closed|fix|fixes|fixed|resolve|resolves|resolved #N` elsewhere.
- Commit footer on every commit:
  ```
  Session: subagent-<your name> of projeto-status-graphhelm-migration-e008b-f1 [7034f3] | Head: <sha8 of the parent commit>

  Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
  ```
- `cargo +1.97.1 fmt --all` before each commit; clippy `-D warnings` clean per touched crate; `git add` by name.
- Tests read no clock, no network, no credentials. The model in every test is `RecordedDraftModel`.
- Track A (`issue-159-customs-acting-surface`) is landing in parallel and edits `apps/cli/src/args.rs`, `commands/mod.rs`, `mcp/tools.rs`, `serve/mod.rs`, `serve/routes.rs`, `tests/mcp_stdio.rs`, `tests/development_surface_parity.rs`. Keep this plan's edits to those files SMALL and at the END of each list/enum/match so the rebase is a one-line merge. This branch will be rebased onto main after Track A lands; the MCP tool count becomes 27 then (24 + claim + clear + synthesize).

---

### Task 1: Crate skeleton, refusals, profile, catalog

**Files:**
- Create: `core/architect/Cargo.toml`, `core/architect/src/lib.rs`, `core/architect/src/refusal.rs`, `core/architect/src/profile.rs`, `core/architect/src/catalog.rs`
- Modify: `Cargo.toml` (workspace members)
- Test: `core/architect/tests/catalog.rs`

**Interfaces (produced):**
```rust
// refusal.rs
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum ArchitectRefusal {
    ModelUnavailable { message: String },
    FixtureMissing { prompt_sha256: String },
    NotJson { round: u8, message: String },
    Invalid { rounds: u8, diagnostics: Vec<graphhelm_protocols::Diagnostic> },
    CapabilityMissing { node: String, program: String },
    TooManyNodes { count: usize, max: usize },
    NotCompletable { nodes: Vec<String> },   // GHG102 survived stamping — must be unreachable, but named
}
// profile.rs
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TaskProfile { pub goal: String, #[serde(default = "default_mode")] pub mode: String /* "autopilot"|"supervised"|"manual" */,
    #[serde(default = "default_max_nodes")] pub max_nodes: usize, #[serde(default = "default_wait")] pub wait_within_seconds: u64,
    #[serde(default = "default_clearance")] pub clearance_within_seconds: u64 }
impl TaskProfile { pub fn new(goal: &str) -> Self; pub fn validate(&self) -> Result<(), ArchitectRefusal> /* goal non-empty ≤ 4 KiB, max_nodes 1..=50, mode in the three */ }
// catalog.rs
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CapabilityCatalog { pub node_types: Vec<String> /* wire names of viable NodeTypes, sorted */, pub tool_families: Vec<&'static str> /* ["repository","shell","tests"] */, pub programs: Vec<String> /* sorted, deduped */ }
impl CapabilityCatalog { pub fn from_runtime(programs: &[String]) -> Self; pub fn allows_program(&self, program: &str) -> bool; pub fn allows_node_type(&self, wire_name: &str) -> bool }
pub const ALL_NODE_TYPES: [graphhelm_protocols::NodeType; 16];  // every variant; the test below proves it is every variant
```

- [ ] **Step 1: Write the failing test** `core/architect/tests/catalog.rs`
```rust
use graphhelm_architect::{ALL_NODE_TYPES, CapabilityCatalog};
use graphhelm_protocols::NodeType;

/// Exhaustiveness by construction: a new NodeType variant makes this match fail to compile,
/// which is the moment ALL_NODE_TYPES must be extended.
#[test]
fn all_node_types_lists_every_variant() {
    for variant in ALL_NODE_TYPES {
        match variant {
            NodeType::Agent | NodeType::Tool | NodeType::Classifier | NodeType::Planner | NodeType::Evaluator | NodeType::Gate
            | NodeType::Fork | NodeType::Join | NodeType::HumanDecision | NodeType::Timer | NodeType::Trigger | NodeType::Subgraph
            | NodeType::Materializer | NodeType::Deploy | NodeType::Rollback | NodeType::ArtifactTransform | NodeType::DeadLetter => {}
        }
    }
    let unique: std::collections::BTreeSet<String> = ALL_NODE_TYPES.iter().map(|t| serde_json::to_string(t).unwrap()).collect();
    assert_eq!(unique.len(), ALL_NODE_TYPES.len(), "no variant is listed twice");
}

#[test]
fn the_catalog_admits_exactly_the_types_the_runtime_executes() {
    let catalog = CapabilityCatalog::from_runtime(&[]);
    for variant in ALL_NODE_TYPES {
        let wire = serde_json::to_string(&variant).unwrap().trim_matches('"').to_owned();
        assert_eq!(catalog.allows_node_type(&wire), graphhelm_runtime::classify::work_kind(&variant).is_ok(), "{wire}");
    }
    assert!(catalog.allows_node_type("agent") && catalog.allows_node_type("tool") && !catalog.allows_node_type("deploy"));
}

#[test]
fn programs_are_the_operators_allowlist_and_nothing_else() {
    let catalog = CapabilityCatalog::from_runtime(&["cargo".to_owned(), "git".to_owned(), "cargo".to_owned()]);
    assert_eq!(catalog.programs, vec!["cargo", "git"]);
    assert!(catalog.allows_program("git") && !catalog.allows_program("python"));
    assert!(CapabilityCatalog::from_runtime(&[]).programs.is_empty(), "no default program: the allowlist is never invented");
}
```
(Fix the variant list to whatever `NodeType` actually declares — read `core/protocols/src/graph.rs:200-260`; `ALL_NODE_TYPES`'s arity must equal the variant count.)

- [ ] **Step 2: Run** `cargo +1.97.1 test -p graphhelm-architect --test catalog --locked` → fails: crate missing.

- [ ] **Step 3: Implement.** `core/architect/Cargo.toml`:
```toml
[package]
name = "graphhelm-architect"
version.workspace = true
edition.workspace = true
license.workspace = true
rust-version.workspace = true

[dependencies]
graphhelm-gateway = { path = "../gateway" }
graphhelm-graph = { path = "../graph" }
graphhelm-protocols = { path = "../protocols" }
graphhelm-runtime = { path = "../runtime" }
graphhelm-schema = { path = "../schema" }
hex.workspace = true
serde.workspace = true
serde_json.workspace = true
sha2.workspace = true
thiserror.workspace = true

[dev-dependencies]
tempfile.workspace = true
```
`lib.rs`: `pub mod catalog; pub mod model; pub mod profile; pub mod refusal; pub mod synthesize; pub mod template;` with re-exports `pub use catalog::{ALL_NODE_TYPES, CapabilityCatalog}; pub use model::{DraftModel, DraftReply, RecordedDraftModel}; pub use profile::TaskProfile; pub use refusal::ArchitectRefusal; pub use synthesize::{SynthesizedGraph, NodeRationale, synthesize}; pub use template::{TEMPLATE, template_sha256, assemble_prompt};` (add `model`, `synthesize`, `template` modules in Tasks 2–3; for this commit create them as empty-but-real modules ONLY if lib.rs references them — otherwise leave the lines out until their task). `catalog.rs`: `from_runtime` iterates `ALL_NODE_TYPES`, keeps those where `graphhelm_runtime::classify::work_kind(&t).is_ok()`, serializes each with `serde_json::to_string` and trims quotes for the wire name; programs sorted+deduped via `BTreeSet`. Add the crate to the workspace `members`.

- [ ] **Step 4: Run** the test file → PASS. `cargo +1.97.1 clippy -p graphhelm-architect --all-targets --locked -- -D warnings` clean.

- [ ] **Step 5: Commit** — `feat(architect): the crate, its refusals, the caller-supplied profile, and a catalog derived from what the runtime executes` / `Refs #107, #184`.

---

### Task 2: Template and the model port with the recorded adapter

**Files:**
- Create: `core/architect/src/template.rs`, `core/architect/src/model.rs`
- Test: `core/architect/tests/template.rs`

**Interfaces (produced):**
```rust
// template.rs
pub const TEMPLATE: &str = "...";                       // the full prompt text, see Step 3
pub fn template_sha256() -> String;                     // hex sha256 of TEMPLATE
pub fn assemble_prompt(profile: &TaskProfile, catalog: &CapabilityCatalog, previous: Option<&RepairContext>) -> String;
pub struct RepairContext<'a> { pub draft: &'a str, pub diagnostics: &'a [graphhelm_protocols::Diagnostic] }
pub fn prompt_sha256(prompt: &str) -> String;
// model.rs
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DraftReply { pub text: String, pub usage: Option<graphhelm_gateway::call::Usage> }
pub trait DraftModel { fn draft(&self, prompt: &str) -> Result<DraftReply, ArchitectRefusal>; }
pub struct RecordedDraftModel { replies: std::collections::BTreeMap<String, String> }
impl RecordedDraftModel { pub fn from_json(bytes: &[u8]) -> Result<Self, ArchitectRefusal> /* {"replies": {sha: text}} */; pub fn from_file(path: &std::path::Path) -> Result<Self, ArchitectRefusal>; pub fn single(prompt_sha256: &str, text: &str) -> Self; }
impl DraftModel for RecordedDraftModel { /* look up prompt_sha256(prompt); missing → FixtureMissing { prompt_sha256 } */ }
```

- [ ] **Step 1: Failing tests** `core/architect/tests/template.rs`:
```rust
#[test]
fn the_assembled_prompt_is_a_pure_function_of_its_inputs_and_carries_the_catalog() {
    let profile = TaskProfile::new("check that the repository builds");
    let catalog = CapabilityCatalog::from_runtime(&["cargo".to_owned()]);
    let a = assemble_prompt(&profile, &catalog, None);
    let b = assemble_prompt(&profile, &catalog, None);
    assert_eq!(a, b);
    assert!(a.contains("cargo") && a.contains("\"agent\"") && a.contains("maxNodes"));
    assert!(a.contains(&template_sha256()), "the template hash is IN the prompt, so the fixture key moves with the template");
}

#[test]
fn a_repair_round_appends_the_diagnostics_verbatim() { /* RepairContext with one Diagnostic; prompt contains its code, pointer, message and the previous draft */ }

#[test]
fn a_recorded_model_answers_only_the_prompt_it_recorded_and_names_the_missing_hash() {
    let prompt = "hello";
    let model = RecordedDraftModel::single(&prompt_sha256(prompt), "{\"spec\":{}}");
    assert_eq!(model.draft(prompt).unwrap().text, "{\"spec\":{}}");
    match model.draft("other") { Err(ArchitectRefusal::FixtureMissing { prompt_sha256 }) => assert_eq!(prompt_sha256, super_prompt_sha256("other")), other => panic!("{other:?}") }
}

#[test]
fn a_recorded_model_file_is_bounded_and_shaped() { /* from_json rejects > 4 MiB, rejects a missing "replies" object, accepts the documented shape */ }
```

- [ ] **Step 2: Run** → fails (modules missing).

- [ ] **Step 3: Implement.** `TEMPLATE` (English, deterministic) must contain, in this order: role line ("You are the Graph Architect of GraphHelm. Output ONE JSON object and nothing else."); the required top-level keys of `spec`; the node contract (`type`, `name`, `objective`, `optionality: "required"`; for `agent`: `agent.ephemeral { purpose, capabilities: [..], inputSchema: "schema://<Name>@1", outputSchema: "schema://<Name>@1", completionContract: { requires: [..] }, instructions }`; for `tool`: `tool.call` — one of the three families with the exact JSON shapes from the spec §3); the edge contract; `budgets.maxNodes`; `completion.terminalNodes`; the rule "use the smallest graph that satisfies the goal; every node must earn its place"; the placeholders `{{GOAL}}`, `{{MODE}}`, `{{MAX_NODES}}`, `{{NODE_TYPES}}`, `{{PROGRAMS}}`, `{{TEMPLATE_SHA256}}`, `{{REPAIR}}`. `assemble_prompt` substitutes with `str::replace` in a fixed order; `{{REPAIR}}` becomes empty or `"Your previous draft was refused. Diagnostics:\n- <code> at <pointer>: <message>\n…\nPrevious draft:\n<draft>\nReturn a corrected JSON object."`. `prompt_sha256` = hex sha256 of the UTF-8 bytes. `RecordedDraftModel::from_json` bounds input at 4 MiB and requires `{"replies": {...}}` with string values.

- [ ] **Step 4: Run** → PASS; clippy clean.
- [ ] **Step 5: Commit** — `feat(architect): a versioned template and the recorded model door` / `Refs #107`.

---

### Task 3: `synthesize` — parse, stamp, validate, repair, emit; golden and sabotage fixtures

**Files:**
- Create: `core/architect/src/synthesize.rs`, `core/architect/fixtures/first-compile/replies.json`, `core/architect/fixtures/first-compile/expected.json`, `core/architect/fixtures/sabotage/*.json` (one recorded reply per case), `core/architect/fixtures/README.md` (how to record a new reply: run with `--fixture` missing → copy the printed prompt sha256; the reply text is authored by hand for tests)
- Test: `core/architect/tests/golden.rs`

**Interfaces (produced):**
```rust
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)] #[serde(rename_all = "camelCase")]
pub struct NodeRationale { pub node: String, pub reason: String }
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)] #[serde(rename_all = "camelCase")]
pub struct SynthesizedGraph { pub document: serde_json::Value, pub rationale: Vec<NodeRationale>, pub stamped_customs: Vec<String>,
    pub template_sha256: String, pub rounds: u8, pub prompt_sha256s: Vec<String>, #[serde(skip_serializing_if = "Option::is_none")] pub usage: Option<graphhelm_gateway::call::Usage> }
pub const MAX_REPAIR_ROUNDS: u8 = 2;
pub fn synthesize(profile: &TaskProfile, catalog: &CapabilityCatalog, model: &dyn DraftModel) -> Result<SynthesizedGraph, ArchitectRefusal>;
```

Algorithm (one function, small private helpers):
1. `profile.validate()?`.
2. `round = 1; previous = None; loop { prompt = assemble_prompt(profile, catalog, previous); sha = prompt_sha256(&prompt); reply = model.draft(&prompt)?; parse: serde_json::from_str::<serde_json::Value>(&reply.text) else NotJson{round,message} (a NotJson on round < 3 is fed back as a synthetic Diagnostic code "GHA001_NOT_JSON" and retried; on round 3 it is the refusal); build the document: take `parsed["spec"]` (or the whole object if it has no `spec` key but has `nodes`), write metadata per D5, `apiVersion: "p50.dev/graph/v1"`, `kind: "ExecutionGraph"`; STAMP: for every node whose `type` is in {agent, planner, classifier, evaluator, tool} and has no `completion.customs`, set `completion.customs = { budgets: { waitWithinSeconds, clearanceWithinSeconds } }` (keep any existing `completion.requires`) and push the node id to `stamped`; validate: `graphhelm_schema::load_graph_json(&serde_json::to_vec(&document)?, "architect-draft")` → on Err(diagnostics) → repair or Invalid; `let report = graphhelm_graph::lint(&loaded.graph, &loaded.source)`; errors non-empty → repair or Invalid; if any warning code == "GHG102_UNBOUNDED_CUSTOMS" → NotCompletable { nodes } (refusal, not repair: the compiler's own stamp failed, which is a bug to see, not a model error); node count > profile.max_nodes → TooManyNodes (refusal, no repair); for each node: `work_kind(&node.node_type)` Err → repair with a synthetic diagnostic "GHA002_NODE_TYPE_NOT_EXECUTABLE" at `/spec/nodes/<id>/type`; tool nodes: parse `properties["tool"]["call"]` as `serde_json::Value`, if `tool == "shell"` and `!catalog.allows_program(program)` → CapabilityMissing { node, program } (refusal, no repair — the model cannot authorize a program); every `rationale` entry: the node's `objective` ("why it exists: <objective>") plus `stamped customs` when stamped; success → SynthesizedGraph { document, rationale, stamped_customs, template_sha256(), rounds: round, prompt_sha256s, usage: reply.usage }. Repair: if round > MAX_REPAIR_ROUNDS → Invalid { rounds: round, diagnostics }; else previous = Some(RepairContext{draft:&reply.text, diagnostics:&diagnostics}); round += 1 }`.
3. Determinism: the document is `serde_json::Value` with `BTreeMap`-ordered objects (serde_json's default `Map` preserves insertion order unless the `preserve_order` feature is off — check `Cargo.lock`/workspace features; if insertion order is preserved, canonicalize by round-tripping through `graphhelm_graph`'s canonical JSON function if one is public, else sort keys recursively with a private helper before returning).

- [ ] **Step 1: Author the fixtures.** `replies.json` needs the prompt sha256 for the first-compile goal: write a tiny ignored test or a `#[test] fn print_prompt_sha_for_recording()` that prints `prompt_sha256(&assemble_prompt(&TaskProfile::new(GOAL), &catalog_with_cargo(), None))`; run it once, copy the hash into `replies.json` as the key, with this reply text (JSON, one line is fine):
```json
{"entrypoints":["build_check"],"nodes":{"build_check":{"type":"tool","name":"Build check","objective":"Run the repository build and record its exit code.","optionality":"required","tool":{"call":{"tool":"shell","program":"cargo","arguments":["build","--locked"]}}},"summarize":{"type":"agent","name":"Summarize the build","objective":"Summarize the build outcome for the operator.","optionality":"required","agent":{"ephemeral":{"purpose":"Summarize the build result.","capabilities":["summarize.build"],"inputSchema":"schema://BuildReport@1","outputSchema":"schema://BuildSummary@1","completionContract":{"requires":["summary"]},"instructions":"State whether the build passed and cite the exit code."}}}},"edges":[{"id":"build_to_summary","from":"build_check","to":"summarize","type":"control"}],"budgets":{"maxNodes":2},"policies":[],"completion":{"terminalNodes":["summarize"]}}
```
Then run synthesis once in a test that writes `expected.json` if absent (guarded by an env var `ARCHITECT_RECORD=1`) and otherwise compares byte-for-byte — the standard golden pattern; commit the recorded `expected.json`. Sabotage replies: (a) `not-json.json` reply "I would build a graph…" for all three rounds; (b) `edge-to-missing-node.json` (edge `to: "ghost"`), (c) `innocent-deploy.json` (node `publish_summary` with `"type": "deploy"`), (d) `too-many-nodes.json` (7 trivially valid agent nodes, profile max_nodes 6), (e) `program-outside-catalog.json` (shell program `python`), (f) `repairs-on-round-two.json` — a replies map with TWO keys: the round-1 prompt sha → the edge-to-missing-node draft, the round-2 prompt sha (assembled with that draft's diagnostics — compute it in the test by calling `assemble_prompt` with a `RepairContext` built from the SAME diagnostics `lint` returns for that draft) → the golden reply. Each sabotage reply that is "the same on every round" is recorded under every round's sha the test computes.

- [ ] **Step 2: Failing tests** `core/architect/tests/golden.rs` — one test per row of spec §5 (golden byte-stable; five sabotage refusals each asserting its OWN `ArchitectRefusal` variant and, for `Invalid`, that `diagnostics` contains the expected code (`GHG003_EDGE_TARGET_UNKNOWN` for (b)); the repair test asserting `rounds == 2` and document == golden; the stamping test: `stamped_customs == ["build_check","summarize"]` and re-linting the emitted document yields zero `GHG102`; the template-hash test: `document["metadata"]["labels"]["template"] == template_sha256()` and `prompt_sha256s.len() == rounds`).

- [ ] **Step 3: Run** → fails (`synthesize` missing). **Step 4: Implement** per the algorithm. **Step 5: Run** → PASS; clippy clean. **Step 6: Commit** — `feat(architect): synthesize compiles a goal into a completable, catalog-bounded graph document, or refuses with the diagnostics` / `Refs #107, #183, #184`.

---

### Task 4: CLI `graph synthesize` and the first-compile journey

**Files:**
- Modify: `apps/cli/Cargo.toml` (add `graphhelm-architect = { path = "../../core/architect" }`)
- Modify: `apps/cli/src/args.rs` (`GraphCommand::Synthesize { .. }` appended after `Apply`)
- Modify: `apps/cli/src/commands/mod.rs` (dispatch arm appended at the end of the `GraphCommand` match)
- Create: `apps/cli/src/commands/architect.rs` (the command + the gateway-backed `DraftModel` adapter)
- Modify: `apps/cli/src/commands/mod.rs` (`pub(crate) mod architect;`)
- Test: `apps/cli/tests/architect_cli.rs`

**Interfaces (produced, used by Task 5):**
```rust
// commands/architect.rs
pub(crate) struct SynthesizeRequest<'a> { pub goal: &'a str, pub mode: &'a str, pub max_nodes: Option<usize>, pub allow_programs: &'a [String], pub wait_within_seconds: Option<u64>, pub clearance_within_seconds: Option<u64> }
pub(crate) enum ModelSource<'a> { Fixture(&'a Path), Gateway { manifest: &'a Path, route: &'a str, broker: Option<&'a Path>, keyring: Option<&'a Path>, key_id: Option<&'a str> } }
pub(crate) fn execute(request: &SynthesizeRequest<'_>, model: &dyn graphhelm_architect::DraftModel) -> Result<serde_json::Value, Failure>;   // the one JSON of spec D8
pub(crate) fn build_model(source: &ModelSource<'_>) -> Result<Box<dyn graphhelm_architect::DraftModel>, Failure>;
pub(crate) struct GatewayDraftModel { /* holds a ModelRoute and either a leased SecretBytes (direct_api) or nothing (native_runtime); draft() calls ByokAdapter/RuntimeAdapter::call with ModelCall { prompt, max_tokens: 8192 } exactly the way serve/ports.rs:133 does, synchronously */ }
pub fn run(goal, out: &Path, mode, max_nodes, allow_programs, fixture, manifest, route, broker, keyring, key_id) -> Outcome;
```
CLI flags: `graph synthesize --goal <text> --out <path.json> [--mode autopilot|supervised|manual] [--max-nodes N] [--allow-program P]* (--fixture <replies.json> | --manifest <m> --route <id> [--broker <dir> --keyring <dir> --key-id <id>])`. `--out` must end in `.json` and must not exist (refuse `GHCLI001`-style argument diagnostic; read `error_codes.rs` for the right argument code). On success write the document with `serde_json::to_vec_pretty` + trailing newline, then print the D8 JSON with `"out": <path as given>` added. On `ArchitectRefusal`, exit through `Outcome::domain` with ONE Diagnostic whose `code` is `GHCLI0xx_ARCHITECT_REFUSED` (add the constant to `apps/cli/src/error_codes.rs` following its numbering), `pointer` `/goal`, and the refusal serialized into the diagnostic's message as compact JSON (so the operator sees `{"kind":"capabilityMissing","node":"…","program":"python"}`). Command name `graph.synthesize`.

- [ ] **Step 1: Failing test** `apps/cli/tests/architect_cli.rs`:
```rust
/// THE FIRST COMPILE (PRD §25 step 5, made falsifiable): one command, keyless, produces a
/// document that lints clean, starts, and completes.
#[test]
fn a_goal_becomes_a_document_that_starts_and_completes() {
    let directory = tempfile::tempdir().unwrap();
    let out = directory.path().join("first-compile.json");
    let fixture = root().join("core/architect/fixtures/first-compile/replies.json");
    let output = command().args(["graph","synthesize","--goal", FIRST_COMPILE_GOAL, "--out", out.to_str().unwrap(), "--allow-program","cargo","--fixture", fixture.to_str().unwrap()]).output().unwrap();
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["ok"], true, "{value}");
    assert_eq!(value["data"]["stampedCustoms"], serde_json::json!(["build_check","summarize"]));
    assert_eq!(value["data"]["rounds"], 1);
    // lint: zero GHG102 — the #183 property, measured on the file the operator would use
    let lint: Value = envelope(&["graph","lint", out.to_str().unwrap()]);
    assert_eq!(lint["ok"], true);
    assert!(!lint.to_string().contains("GHG102"), "{lint}");
    // execute it: the same road every authored graph takes
    let events = directory.path().join("events");
    let fixtures = write_json(directory.path(), "fixtures.json", serde_json::json!({"nodeOutcomes": {"build_check":"success","summarize":"success"}}));
    let started = envelope(&["execution","start","--file", out.to_str().unwrap(), "--events", events.to_str().unwrap(), "--fixtures", fixtures.to_str().unwrap(), "--mode","supervised","--execution","exec-first-compile"]);
    assert_eq!(started["data"]["status"], "completed", "{started}");
}

#[test]
fn the_document_is_byte_identical_across_two_runs() { /* synthesize twice into two files; compare bytes */ }

#[test]
fn a_goal_needing_a_program_outside_the_allowlist_is_refused_and_names_the_program() { /* fixture program-outside-catalog.json with --allow-program cargo → ok=false, diagnostics[0].code == GHCLI0xx_ARCHITECT_REFUSED, message contains "python"; --out not created */ }

#[test]
fn an_existing_out_path_is_never_overwritten() { /* pre-create the file → argument refusal, content unchanged */ }

#[test]
fn a_fixture_without_the_prompt_prints_the_hash_to_record() { /* empty replies fixture → message contains "fixtureMissing" and a 64-hex string */ }
```
`FIRST_COMPILE_GOAL` must be the exact string the fixture was recorded with (put it in `core/architect/fixtures/first-compile/GOAL.txt` and read it in both the crate test and here, so one string exists).

- [ ] **Step 2: Run** → unknown subcommand. **Step 3: Implement** args, dispatch, `architect.rs` (the gateway adapter mirrors `gateway/probe.rs:105-180` for the direct_api lease and `serve/ports.rs:170-195` for the call; native_runtime needs only the route). **Step 4: Run** the new test file plus `cli_smoke`, `execution_cli` → green; clippy clean. **Step 5: Commit** — `feat(cli): graph synthesize compiles a goal into a document on the road every authored graph takes` / `Refs #107`.

---

### Task 5: HTTP route and MCP tool

**Files:**
- Modify: `apps/cli/src/commands/serve/routes.rs` (handler `synthesize`), `apps/cli/src/commands/serve/mod.rs` (`.route("/v1/graphs/synthesize", post(routes::synthesize))` appended after the last `/v1/graph/...` route)
- Modify: `apps/cli/src/commands/mcp/tools.rs` (ToolSpec `synthesize` LAST in `TOOLS`, `synthesize_schema` closed object with `goal` required and `mode, maxNodes, allowPrograms(array of string), fixture, route`; match arm POSTing the body)
- Modify: `apps/cli/tests/mcp_stdio.rs` (append `"synthesize"` to `MCP_TOOL_NAMES`, arity +1), `apps/cli/tests/development_surface_parity.rs` (append to `NON_DEVELOPMENT_TOOLS`, arity +1, comment `// #107`)
- Test: `apps/cli/tests/api_http.rs` (one test), `apps/cli/tests/architect_cli.rs` (MCP parity via the existing `mcp_session` helper pattern from `mcp_stdio.rs`, or a second test in `mcp_stdio.rs`)

Handler: body `{goal, mode?, maxNodes?, allowPrograms?, fixture?, route?}`; NOT a mutation on an execution (no Idempotency-Key, no execution id) — it is a read-shaped POST like `/v1/graph/topology` (`routes.rs` `graph_topology`; copy its auth/response shape). Model: `fixture` given → `RecordedDraftModel::from_file` (path on the server's disk, same trust seam as `start`'s `file`); else `state.runtime` must be `Some` → `resolve_requested_route` + `ServeModelPort::build`, and `draft()` runs `spawn_blocking` over the port's `call` (the port is async; wrap with a small `struct ServeDraftModel` implementing `DraftModel` by `tokio::task::block_in_place`/`Handle::current().block_on` inside `spawn_blocking` — read how `gateway_probe` bridges sync/async and do the same); neither → `bad_request` "this server has no runtime wiring; pass \"fixture\" or start serve with --manifest/--route". `allowPrograms` defaults to the server's `wiring.allow_programs` when wiring exists, else `[]`. Reply `200` with the D8 JSON under `data`.

- [ ] **Step 1: Failing HTTP test**: `the_api_and_the_cli_compile_the_same_goal_to_the_same_bytes` — start fixture-only serve, POST with `fixture` path and `allowPrograms: ["cargo"]`, compare `data.document` bytes with the CLI's `--out` file bytes (parse both to `Value`, compare `serde_json::to_vec`) and `data.templateSha256` equality.
- [ ] **Step 2: Run** → 404. **Step 3: Implement.** **Step 4: Run** `api_http`, `mcp_stdio`, `development_surface_parity`, `surface_completeness`, `mcp_capability` → green. **Step 5: Commit** — `feat(runtime-api): synthesize reaches HTTP and MCP with the CLI's exact reply` / `Refs #107`.

---

### Task 6: Records

**Files:**
- Create: `docs/harness/GRAPH_ARCHITECT.md` (what shipped vs HARNESS_SPEC §4: one box built, inputs/outputs table, the ten named non-goals, Tier A vs Tier B, the refusal vocabulary, the recording procedure for fixtures)
- Create: `docs/acceptance/m11-first-compile-2026-09-11.md` (the exact command, the document's sha256, the lint output, the execution's final status, the test names that hold each cell of spec §5)
- Modify: `docs/DECISION_REGISTER.md` (two rows: `D-051` synthesized graphs are born completable — the compiler stamps customs and treats GHG102 as a synthesis failure; `D-052` the architect refuses programs outside the operator's allowlist and never widens it; each row cites #183/#184 and this design)
- Modify: `CHANGELOG.md` (top entry `## The first compile: the Graph Architect, #107 - 2026-09-11`)
- Modify: `docs/product/ROADMAP_AND_ACCEPTANCE.md` §3.2.1 — one sentence after the table noting #107's slice landed and what it does not do (profiler, discovery).
- Modify: `README.md` — one line in the CLI list if it enumerates commands (check; skip if it does not).

- [ ] Run the first-compile command once by hand from `D:/o-107` and paste its real output into the acceptance record. Commit — `docs(107): the architect's records — decisions, acceptance run, harness note` / `Refs #107`.

---

### Task 7: Final check before the PR

- [ ] `cargo +1.97.1 fmt --all -- --check`; `cargo +1.97.1 clippy --workspace --all-targets --all-features --locked -- -D warnings`; `cargo +1.97.1 test -p graphhelm-architect -p graphhelm-cli --locked`; `git diff --check`.
- [ ] `git log origin/main..HEAD --format=%B | grep -inE "(close|closes|closed|fix|fixes|fixed|resolve|resolves|resolved) #"` → nothing.
- [ ] Push `git push -u origin issue-107-graph-architect`; report the head sha. The orchestrator opens the PR and rebases onto main after Track A merges.
