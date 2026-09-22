# Structured Agent Context Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make AI communication travel as schema validated, version and digest bound development envelopes with bounded lazy references, while keeping human output derived and leaving existing host `SKILL.md` files untouched.

**Architecture:** Keep wire types in `core/protocols`, and put schema-first loading in `core/schema`, which may consume protocol types but does not create a dependency cycle. Runtime callers produce envelopes from real measured inputs; the context compiler resolves only bounded references whose snapshot binding is still fresh. CLI, MCP, and HTTP adapters consume the same decision function, while human rendering remains a projection. Existing Markdown skills remain host adapters and are not converted.

**Tech Stack:** Rust 1.97.1, Serde, checked in JSON Schema, `sha256` canonical digests, existing `core/runtime` ports and retrieval compiler, CLI integration tests, offline deterministic fixtures.

**Spec:** `docs/superpowers/specs/2026-09-21-graphhelm-methodology-adoption-design.md`

## Global Constraints

- Read the spec and this plan before implementation; implementation starts only after the issue-first task is assigned.
- All documentation added by this plan is English.
- Schema validation happens in `core/schema` before typed deserialization; unknown major versions fail closed.
- Every binding carries document version, schema version, canonical digest, producer, scope, and snapshot identity.
- A reference is resolved lazily and only within declared byte, item, depth, page, and token limits; missing or stale references produce typed refusal data.
- Runtime core depends on ports and contracts, never on concrete filesystem, network, provider, or host adapters.
- Human rendering is derived from the machine envelope and cannot add facts or alter status.
- Existing host `SKILL.md` files are preserved byte for byte; this plan adds no forced conversion.
- JSON/YAML/Markdown comparisons measure correctness and end to end cost only; economic ownership and money decisions remain outside this plan.
- Tests are deterministic, offline, and require no network, credentials, Docker, browser, or provider account.
- `ci/gate.ps1` is the authoritative repository gate; use the pinned Rust 1.97.1 toolchain and put `CARGO_TARGET_DIR` on the same PowerShell command line as every `cargo` invocation.

## Review Focus

- A caller sends a valid looking envelope with an unsupported major or mismatched schema/digest; validation must refuse before any retrieval or render.
- A lazy reference points at a stale repository/index snapshot or crosses scope; resolution must refuse without reading bytes from the wrong snapshot.
- A bounded source returns more pages, bytes, or items than declared; the result must carry a stable refusal and no silent truncation.
- Required context exceeds the budget while optional context does not; required material must refuse with an expansion request and optional material may be counted as dropped.
- A human renderer receives a refusal, missing reference, or unknown field; it must derive the same status and never invent a success or expose raw secrets/paths.

---

### Task 1: Bind structured caller inputs to the development envelope

**Files:**
- Modify: `core/protocols/src/development.rs:191-623` (`DevelopmentScope`, `ArtifactBinding`, `SnapshotBinding`, `DevelopmentEnvelope`, `verify_binding`)
- Create: `core/schema/src/development.rs` (`validate_structured_context_request`)
- Create: `apps/cli/tests/fixtures/structured-context-request.json` (shared by Tasks 1 and 3)
- Modify: `core/schema/src/lib.rs` to export the schema-first loader
- Create: `extensions/builtin/graphhelm-development-contracts/schemas/structured-context-request.schema.json`
- Modify: `extensions/builtin/graphhelm-development-contracts/extension.json` to register the schema and refresh its package digest/inventory entry
- Test: `core/protocols/src/development.rs` contract tests and `apps/cli/tests/development_contract_schemas.rs`

**Interfaces:**
- Consumes: existing `ArtifactBinding`, `SnapshotBinding`, `DevelopmentScope`, canonical JSON and `verify_binding`.
- Produces: `#[derive(Clone, Debug, Serialize, Deserialize)] #[serde(rename_all = "camelCase")] pub struct StructuredContextRequest { pub envelope: DevelopmentEnvelope, pub required_refs: Vec<ArtifactBinding>, pub optional_refs: Vec<ArtifactBinding> }`; the envelope must be an existing `DevelopmentKind::RetrievalPlan`, while the request wrapper has its own schema; `pub fn validate_structured_context_request(schemas: &OfflineSchemaSet, value: &serde_json::Value) -> Result<StructuredContextRequest, Vec<Diagnostic>>` in `core/schema`.

- [ ] **Step 1: Write the failing contract tests.** Add this test shape to `core/schema/src/development.rs` tests:

```rust
#[test]
fn unsupported_major_refuses_before_deserialize() {
    let mut value: serde_json::Value = serde_json::from_str(
        include_str!("../../../apps/cli/tests/fixtures/structured-context-request.json"),
    ).unwrap();
    value["envelope"]["apiVersion"] = serde_json::json!("p50.dev/development/v9");
    let schemas = OfflineSchemaSet::compile(test_schema_resources()).unwrap();
    let errors = validate_structured_context_request(&schemas, &value).unwrap_err();
    assert!(errors.iter().any(|d| d.code == graphhelm_protocols::DevelopmentRefusalCode::UnknownMajorVersion.wire_name()));
}
```

Also add `binding_digest_and_snapshot_mismatch_refuse` and `structured_request_preserves_scope_and_versions`; both construct the complete JSON object and assert the exact diagnostic code and preserved typed fields.
- [ ] **Step 2: Run the focused tests and verify they fail.** Run `$env:CARGO_TARGET_DIR='E:\_agent-scratch\graphhelm\methodology-adoption-context\target'; cargo +1.97.1 test -p graphhelm-schema development --locked` and `$env:CARGO_TARGET_DIR='E:\_agent-scratch\graphhelm\methodology-adoption-context\target'; cargo +1.97.1 test -p graphhelm-cli --test development_contract_schemas --locked`. Expected: the new loader and schema fields are absent.
- [ ] **Step 3: Add the schema-first loader.** Keep `DevelopmentEnvelope` and `StructuredContextRequest` data types in `core/protocols`; create a separate `structured-context-request.schema.json` with full `$id: "https://p50.dev/extensions/graphhelm-development-contracts/schemas/structured-context-request.schema.json"` and wire shape `{ "envelope": <existing DevelopmentEnvelope>, "requiredRefs": [...], "optionalRefs": [...] }`. Require the nested envelope to have `kind: "RetrievalPlan"`; do not add fields to or alter the required fields of `development-envelope.schema.json`. In `core/schema/src/development.rs`, precheck the nested `envelope.apiVersion` with `development_api_version_major`, validate the wrapper with `OfflineSchemaSet::validate`, then deserialize and verify bindings:

```rust
use graphhelm_protocols::Severity;
use std::collections::BTreeMap;

#[cfg(test)]
fn test_schema_resources() -> BTreeMap<String, serde_json::Value> {
    let directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../extensions/builtin/graphhelm-development-contracts/schemas");
    std::fs::read_dir(directory).unwrap().map(|entry| {
        let path = entry.unwrap().path();
        (path.file_name().unwrap().to_string_lossy().into_owned(),
         serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap())
    }).collect()
}

pub fn validate_structured_context_request(
    schemas: &OfflineSchemaSet,
    value: &serde_json::Value,
) -> Result<StructuredContextRequest, Vec<Diagnostic>> {
    let envelope = value.get("envelope").ok_or_else(|| vec![fixed("/envelope", "structured context envelope is required")])?;
    if graphhelm_protocols::development_api_version_major(envelope.get("apiVersion").and_then(serde_json::Value::as_str).unwrap_or("")) != Some(graphhelm_protocols::DEVELOPMENT_API_MAJOR) {
        return Err(vec![Diagnostic::error("unknown_major_version", "unsupported development API major", "/envelope/apiVersion", "structured-context-request")]);
    }
    let diagnostics = schemas.validate("https://p50.dev/extensions/graphhelm-development-contracts/schemas/structured-context-request.schema.json", value, "structured-context-request");
    if diagnostics.iter().any(|d| d.severity == Severity::Error) { return Err(diagnostics); }
    let request: StructuredContextRequest = serde_json::from_value(value.clone())
        .map_err(|_| vec![fixed("/", "structured context request is not valid JSON for its schema")])?;
    // Recompute SHA-256 from digest_input with the existing workspace hash helper;
    // compare to envelope.digest before returning the validated request.
    verify_request_bindings(&request)?;
    Ok(request)
}

fn fixed(pointer: &'static str, message: &'static str) -> Diagnostic {
    Diagnostic::error("schema_invalid", message, pointer, "structured-context-request")
}

fn verify_request_bindings(request: &StructuredContextRequest) -> Result<(), Vec<Diagnostic>> {
    for candidate in request.required_refs.iter().chain(&request.optional_refs) {
        let Some(reference) = request.envelope.bindings.iter().find(|binding| binding.artifact_id == candidate.artifact_id) else {
            return Err(vec![fixed("/references", "context reference has no matching retrieval plan binding")]);
        };
        if candidate.document_version != reference.document_version {
            return Err(vec![fixed("/references", "context reference document version does not match its retrieval plan binding")]);
        }
        if let Err(code) = graphhelm_protocols::verify_binding(candidate, reference) {
            return Err(vec![Diagnostic::error(code.wire_name(), "context reference does not match its retrieval plan binding", "/references", "structured-context-request")]);
        }
    }
    Ok(())
}
```

Create the shared request fixture in Task 1, not Task 3: clone `extensions/builtin/graphhelm-development-contracts/fixtures/contracts/valid/code-rule-minimal.json`, set `apiVersion` to `p50.dev/development/v1`, set `kind` to `RetrievalPlan`, set `bindings` to an empty array and `spec` to an empty object for this no-source baseline. Recompute its digest from `DevelopmentEnvelope::digest_input()` using the checked-in SHA-256 helper; wrap it in `{ "envelope": envelope, "requiredRefs": [], "optionalRefs": [] }`. The source-bearing fixtures use the existing retrieval-plan contract and real source locators; an empty baseline never proves lazy retrieval.

Add the wrapper schema with closed properties and explicit required/optional reference arrays. Register the new schema in `extension.json`, then regenerate only the package inventory digest with the existing inventory tool.
- [ ] **Step 4: Run the focused tests and verify they pass.** Repeat both commands. Run the schema catalog/conformance command used by `ci/gate.ps1` and verify the extension manifest digest change covers only the intended files.
- [ ] **Step 5: Commit the contract slice.** Use `git add core/protocols/src/development.rs core/schema/src/development.rs core/schema/src/lib.rs extensions/builtin/graphhelm-development-contracts/schemas/structured-context-request.schema.json extensions/builtin/graphhelm-development-contracts/extension.json apps/cli/tests/development_contract_schemas.rs` and commit with `feat(protocols): bind structured context requests`.

### Task 2: Compile real caller inputs with bounded lazy references

**Files:**
- Modify: `core/runtime/src/context.rs:438-775` (`CompiledContext`, `compile_for_node`, `retrieve_and_compile`, `compile_items`)
- Modify: `core/runtime/src/retrieval.rs:206-1243` (`validate_retrieval_receipt`, plan composition and source fallback)
- Modify: `core/runtime/src/ports.rs:86-197` (`SourceReader`, `StructuralIndex`, declared limits)
- Test: `core/runtime/tests/context_chain.rs`, `core/runtime/tests/context_compiler.rs`, `core/runtime/tests/retrieval.rs`

**Interfaces:**
- Consumes: `StructuredContextRequest`, `SourceReader`, `StructuralIndexRequest`, `DeclaredLimits`, `fit_within_budget`, `verify_citations`, and `SnapshotBinding`.
- Produces: `pub fn compile_structured_context(request: &StructuredContextRequest, identity: &dyn SourceReader, excerpts: &dyn BoundedSourceReader, limits: &DeclaredLimits) -> Result<CompiledContext, DevelopmentRefusalCode>`; `pub struct LazyContextRef { pub binding: ArtifactBinding, pub locator: String, pub byte_range: Option<(u64, u64)> }`.

- [ ] **Step 1: Write failing runtime tests.** Add tests with fully defined identity and excerpt recorders:

```rust
struct RecordingReader { snapshot: OpaqueId }
impl SourceReader for RecordingReader {
    fn current_snapshot(&self) -> OpaqueId { self.snapshot.clone() }
}
struct RecordingExcerptReader { calls: std::sync::Mutex<Vec<String>>, bytes: Vec<u8> }
impl BoundedSourceReader for RecordingExcerptReader {
    fn read_prefix(&self, path: &str, max_bytes: u64) -> Result<SourceExcerpt, SourceReadError> {
        self.calls.lock().unwrap().push(path.to_owned());
        let end = usize::try_from(max_bytes).unwrap_or(usize::MAX).min(self.bytes.len());
        Ok(SourceExcerpt { bytes: self.bytes[..end].to_vec(), file_len: self.bytes.len() as u64 })
    }
}
```

Use it in `stale_snapshot_refuses_without_read`, `page_and_byte_limits_are_aggregate`, `required_ref_over_budget_returns_expansion`, and `optional_ref_drop_is_recorded`; assert stale/cross-scope refusal, aggregate index-plus-source limits, expansion budget, and counted optional drops.
- [ ] **Step 2: Run the focused runtime tests.** Run `$env:CARGO_TARGET_DIR='E:\_agent-scratch\graphhelm\methodology-adoption-context\target'; cargo +1.97.1 test -p graphhelm-runtime --test context_chain --test context_compiler --test retrieval --locked`. Expected: the structured entry point and lazy reference behavior are missing.
- [ ] **Step 3: Implement the smallest composition path.** Add `pub fn compile_structured_context(request: &StructuredContextRequest, identity: &dyn SourceReader, excerpts: &dyn BoundedSourceReader, limits: &DeclaredLimits) -> Result<CompiledContext, DevelopmentRefusalCode>`. Validate the request before resolving any ref; compare the plan snapshot with `identity.current_snapshot()` and refuse stale/cross-scope bindings. Resolve refs only when selected through `excerpts.read_prefix`, pass remaining aggregate limits to the existing retrieval compiler, and call `fit_within_budget` so required items refuse with `ContextBudgetInsufficient` while optional drops are counted.
- [ ] **Step 4: Prove the runtime behavior.** Repeat the focused tests and add deterministic assertions for byte-identical compiled capsules, content-derived item IDs, citation refusal, and no secret-shaped source content in the compiled result.
- [ ] **Step 5: Commit the runtime slice.** Commit with `feat(runtime): compile bounded structured context references`.

### Task 3: Expose the real caller path through CLI and derived human output

**Files:**
- Modify: `apps/cli/src/args.rs:207-231` in the existing `DevelopmentCommand::CompileContext` definition, preserving the current no-argument defaults
- Modify: `apps/cli/src/commands/development.rs:134-192` (`run_compile_context`, `compile_context_decision`, refusal mapping)
- Modify: `core/runtime/src/owner_output.rs:85-200` (`OwnerPresentationPlan`, `OwnerPresentation`, `validate_and_build`)
- Modify: `apps/cli/src/commands/serve/routes.rs:2890-2920` (`development_compile_context`)
- Modify: `apps/cli/src/commands/mcp/tools.rs` and its generated/allowlist parity tests for the public `compile_context` tool
- Test: `apps/cli/tests/development_cli.rs`, `apps/cli/tests/development_surface_parity.rs`, `apps/cli/tests/development_redaction.rs`

**Interfaces:**
- Consumes: `StructuredContextRequest`, `compile_structured_context`, `DevelopmentEnvelope`, and `OwnerPresentation`.
- Produces: additive `DevelopmentCommand::CompileContext { budget, require, request: Option<PathBuf> }`; `pub fn run_compile_context(budget: usize, require: &[String], request: Option<&Path>) -> Outcome`; `pub fn render_structured_context(envelope: &DevelopmentEnvelope) -> OwnerPresentation`. With `request == None`, existing argument-free behavior remains byte-compatible.

- [ ] **Step 1: Write failing CLI and projection tests.** Add tests with the existing `CompileContext` invocation shape and a complete temporary request helper:

```rust
fn write_request(temp: &tempfile::TempDir) -> std::path::PathBuf {
    let path = temp.path().join("structured-request.json");
    std::fs::copy(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/structured-context-request.json"), &path).unwrap();
    path
}

#[test]
fn cli_accepts_structured_request() {
let temp = tempfile::tempdir().unwrap();
let request_path = write_request(&temp);
let output = std::process::Command::new(assert_cmd::cargo::cargo_bin!("graphhelm"))
    .args(["development", "compile-context", "--request", request_path.to_str().unwrap(), "--budget", "128"])
    .output().unwrap();
let output: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
assert_eq!(output["command"], "development.compile-context");
assert_eq!(output["data"]["envelope"]["apiVersion"], "p50.dev/development/v1");
}
```

Also add `argument_free_compile_context_keeps_existing_digest`, `mcp_and_http_accept_the_same_request`, `presentation_cannot_upgrade_refusal`, and `normal_output_redacts_paths_and_secret_shapes`.
- [ ] **Step 2: Run the focused CLI tests.** Run `$env:CARGO_TARGET_DIR='E:\_agent-scratch\graphhelm\methodology-adoption-context\target'; cargo +1.97.1 test -p graphhelm-cli --test development_cli --test development_surface_parity --test development_redaction --locked`. Expected: the additive request path is absent while the old no-argument tests remain green.
- [ ] **Step 3: Wire all three public adapters.** Add `request: Option<PathBuf>` with no default behavior change. `run_compile_context` loads the file only when present and otherwise calls the existing `compile_context_decision`; `serve::routes::development_compile_context` accepts the same optional JSON request; MCP `compile_context` exposes the same optional field. All call `core/schema::validate_structured_context_request` and the same runtime function. Do not add provider calls, network retrieval, host skill conversion, or a second human-only status model.
- [ ] **Step 4: Prove parity and redaction.** Repeat the tests, then run the canonical CLI, MCP, and HTTP fixtures with a checked-in request. Compare JSON and rendered output from the same envelope and assert stable refusal codes, no absolute paths, and no secret values.
- [ ] **Step 5: Commit the surface slice.** Commit with `feat(cli): expose structured context compilation`.

### Task 4: Measure representation correctness and end-to-end cost

**Files:**
- Modify: `tools/development-benchmark/src/main.rs` and `tools/development-benchmark/tests/paired_contexts.rs` in the `graphhelm-development-benchmark` crate
- Modify: `apps/cli/tests/context_journey.rs` for the end-to-end journey
- Create: `tools/development-benchmark/corpus/structured-context/equivalent.json`
- Create: `tools/development-benchmark/corpus/structured-context/equivalent.yaml`
- Create: `tools/development-benchmark/corpus/structured-context/equivalent.md`
- Test: `tools/development-benchmark/tests/paired_contexts.rs`, `apps/cli/tests/context_journey.rs`

**Interfaces:**
- Consumes: the CLI structured request, deterministic source fixtures, compiled envelope digest, and existing benchmark comparison helpers.
- Produces: `pub struct MeasurementCounters { pub bytes_read: u64, pub retrieval_pages: u64, pub compiled_input_tokens: Option<u64> }`; `pub struct RepresentationMeasurement { pub format: &'static str, pub digest: String, pub correct: bool, pub bytes_read: u64, pub retrieval_pages: u64, pub compiled_input_tokens: Option<u64> }`; and `pub fn measure_representation(format: &'static str, output: &DevelopmentEnvelope, expected: &DevelopmentEnvelope, counters: &MeasurementCounters) -> RepresentationMeasurement`. Wall-clock timing is collected by the external benchmark runner; unavailable token/cost fields remain unavailable. It does not make economic or spending decisions.

- [ ] **Step 1: Write the failing journey and comparison tests.** Add these concrete tests:

```rust
#[test]
fn unavailable_tokens_remain_none_in_representation_measurement() {
    let envelope: DevelopmentEnvelope = serde_json::from_str(include_str!("../corpus/structured-context/equivalent.json")).unwrap();
    let measurement = measure_representation("markdown", &envelope, &envelope, &MeasurementCounters { bytes_read: 0, retrieval_pages: 0, compiled_input_tokens: None });
    assert_eq!(measurement.compiled_input_tokens, None);
}

#[test]
fn equivalent_formats_compare_digest_and_measured_counters() {
    let envelope: DevelopmentEnvelope = serde_json::from_str(include_str!("../corpus/structured-context/equivalent.json")).unwrap();
    let json = measure_representation("json", &envelope, &envelope, &MeasurementCounters { bytes_read: 4, retrieval_pages: 1, compiled_input_tokens: None });
    let yaml = measure_representation("yaml", &envelope, &envelope, &MeasurementCounters { bytes_read: 4, retrieval_pages: 1, compiled_input_tokens: Some(12) });
    assert_eq!(json.digest, yaml.digest);
    assert!(json.correct && yaml.correct);
    assert_eq!(json.bytes_read, yaml.bytes_read);
}
```

The two small tests above only characterize measurement bookkeeping; they do not establish equivalence across parsers. Add a negative control by changing `output.spec` while retaining the expected envelope and require `correct == false`. Add independent parser tests reading each of the three files. Also add `journey_reads_lazy_refs_only_when_required`; assert correctness by canonical envelope/output equality and cost by existing measured counters, without declaring one format universally superior.
- [ ] **Step 2: Run the focused benchmark tests.** Run `$env:CARGO_TARGET_DIR='E:\_agent-scratch\graphhelm\methodology-adoption-context\target'; cargo +1.97.1 test -p graphhelm-development-benchmark --test paired_contexts --locked` and `$env:CARGO_TARGET_DIR='E:\_agent-scratch\graphhelm\methodology-adoption-context\target'; cargo +1.97.1 test -p graphhelm-cli --test context_journey --locked`. Expected: no structured-context corpus or journey exists.
- [ ] **Step 3: Add the bounded fixtures and measurement adapter.** Generate equivalent JSON and YAML from the same validated envelope and put that exact JSON in one fenced `json` block in the Markdown fixture. The Markdown reader accepts exactly that single fenced block and refuses extra semantic content; this is a controlled representation benchmark, not a general Markdown parser. Parse each file independently, validate its schema and recomputed digest, then compare against a separately frozen expected envelope; implement the declared measurement types with this pure function:

```rust
pub fn measure_representation(
    format: &'static str,
    output: &DevelopmentEnvelope,
    expected: &DevelopmentEnvelope,
    counters: &MeasurementCounters,
) -> RepresentationMeasurement {
    RepresentationMeasurement {
        format,
        digest: output.digest.as_str().to_owned(),
        correct: output.digest_input() == expected.digest_input(),
        bytes_read: counters.bytes_read,
        retrieval_pages: counters.retrieval_pages,
        compiled_input_tokens: counters.compiled_input_tokens,
    }
}
```

Keep fixture data offline and free of secrets. The external paired runner owns wall-clock timing; this function reports no fabricated duration or cost.
- [ ] **Step 4: Verify the end-to-end proof.** Repeat both tests and inspect that each result names its input format and measured fields, that correctness failures remain visible, unavailable values are not rendered as zero, and the benchmark does not emit financial recommendations or mutate a memory registry.
- [ ] **Step 5: Commit the measurement slice.** Commit with `test(benchmark): compare structured context representations`.

## Self-review checklist

- Spec coverage: caller inputs and envelope binding are Task 1; lazy bounded refs and snapshot safety are Task 2; CLI and derived human output are Task 3; JSON/YAML/Markdown correctness and cost measurement are Task 4. Host `SKILL.md` conversion and economic decisions are explicitly excluded.
- Plan review: every task names files, interfaces, tests, commands, and expected outcomes; implementation must perform the final code-level review before claiming completion.
- Type consistency: Task 1 produces `StructuredContextRequest`; Task 2 consumes it and produces `compile_structured_context`; Task 3 consumes that function; Task 4 consumes the CLI result and existing benchmark counters.
- Review focus coverage: unsupported/mismatched bindings are Task 1; stale/cross-scope and aggregate bounds are Task 2; refusal/render parity and redaction are Task 3; representation correctness and cost are Task 4.
