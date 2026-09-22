# Journey-Proven Development Pilot Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Prove the real `graphhelm init` journey, retain its earlier promises and failed attempts, and demonstrate that deliberate regressions are caught by an independent observer.

**Architecture:** A CLI pilot runs init in its own temporary project. An adapter observes filesystem facts after the child exits; pure evaluators check frozen obligations and conservative regression selection. The existing retry and quality gates remain authoritative for their present contracts; the new observer produces evidence through the Runtime gate path without publishing graph mutations.

**Tech Stack:** Rust `1.97.1`, `assert_cmd`, `tempfile`, SHA-256, existing JPD schemas, `pathogens`, `core/policy`, `adapters/tool-host`, local PowerShell gate.

**Spec:** [Methodology adoption design](../specs/2026-09-21-graphhelm-methodology-adoption-design.md), especially sections 5 and 9; [existing JPD contract](../../harness/JOURNEY_PROVEN_DEVELOPMENT.md).

## Global Constraints

- All repository documentation must be written in English.
- Only the Graph Governor may publish operational graph mutations.
- Unknown, stale, forged, or unbound evidence is never proven.
- Offline tests must not require network access, a browser, credentials, Docker, provider accounts, or production infrastructure.
- Use the existing public CLI, MCP, HTTP, and Runtime gate contracts.
- Run the authoritative local `ci/gate.ps1`; GitHub Actions remains disabled.
- Every Cargo command uses Rust `1.97.1` and `$env:CARGO_TARGET_DIR='E:/_agent-scratch/graphhelm/methodology-adoption-jpd/target'; cargo ...` after checking `Get-PSDrive E` shows more than 30 GB free. At most two local gate launches may coexist.
- An in-process type boundary or same-user file is not OS-level anti-forgery. The pilot proves the observer's bounded filesystem claims; hostile same-user code requires isolation outside this slice.
- Preserve the existing geometry, retry-lineage, and journey-contract gates. Do not silently redefine an existing gate version.

## Review Focus

1. Already-working init behavior remains preserved through a failed/recovered run: Task 1 independently reads old-token equality and another MCP entry.
2. Missing observation, self-observation, and caller-supplied receipts cannot become success: Task 2 tests capture custody and exact run bindings.
3. A successful retry cannot erase the original failure: Task 3 retains root evidence and rejects missing lineage.
4. Stale or partial impact coverage cannot omit old behavior: Task 4 falls back to the complete baseline.
5. Sabotage must fail for its intended reason; a broken schema registry is a harness error: Task 5 pairs each negative with a passing control.

---

### Task 1: Freeze a real journey and its preservation obligations

**Files:** Create `apps/cli/tests/jpd_pilot_journey.rs`; create `extensions/builtin/graphhelm-jpd/fixtures/pilot/init-promises.json`; modify `extensions/builtin/graphhelm-jpd/extension.json` for the fixture inventory/digest.

**Interfaces:** Consumes the existing `init --project PATH --harness claude-code` public CLI. Produces an offline characterization test and a frozen fixture listing the observable promises. No production type is defined in a test module.

- [ ] **Step 1: Write the complete baseline characterization.** This should already pass; do not claim it is a new failing requirement.

```rust
use std::path::Path;
use serde_json::Value;

fn git_project() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join(".git")).unwrap();
    dir
}
fn run_init(project: &Path) -> Value {
    let output = assert_cmd::Command::new(assert_cmd::cargo::cargo_bin!("graphhelm"))
        .args(["init", "--project"]).arg(project)
        .args(["--harness", "claude-code"]).output().unwrap();
    serde_json::from_slice(&output.stdout).unwrap()
}
fn read_json(path: &Path) -> Value {
    serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
}
#[test]
fn init_preserves_prior_behavior_through_refusal_and_recovery() {
    let project = git_project();
    let mcp = project.path().join(".mcp.json");
    let original = br#"{"mcpServers":{"other":{"command":"other-server"}},"unrelated":1}"#;
    std::fs::write(&mcp, original).unwrap();
    assert_eq!(run_init(project.path())["ok"], true);
    let token_path = project.path().join(".graphhelm/events.token");
    let before = std::fs::read(&token_path).unwrap();
    assert!(!before.is_empty());
    assert_eq!(read_json(&mcp)["mcpServers"]["other"]["command"], "other-server");
    std::fs::write(&mcp, b"[1,2,3]").unwrap();
    let refused = run_init(project.path());
    assert_eq!(refused["ok"], false);
    assert_eq!(refused["diagnostics"][0]["code"], "GHCLI027_INIT_REFUSED");
    assert_eq!(std::fs::read(&mcp).unwrap(), b"[1,2,3]");
    assert_eq!(std::fs::read(&token_path).unwrap(), before);
    std::fs::write(&mcp, original).unwrap();
    assert_eq!(run_init(project.path())["ok"], true);
    assert_eq!(std::fs::read(&token_path).unwrap(), before);
    assert_eq!(read_json(&mcp)["mcpServers"]["other"]["command"], "other-server");
    assert_eq!(read_json(&mcp)["unrelated"], 1);
}
```

- [ ] **Step 2: Run characterization.** `$env:CARGO_TARGET_DIR='E:/_agent-scratch/graphhelm/methodology-adoption-jpd/target'; cargo +1.97.1 test -p graphhelm-cli --test jpd_pilot_journey --test init_cli --locked`. Expect PASS. If existing behavior fails, record a defect and repair within its own issue before using it as the baseline.
- [ ] **Step 3: Freeze the promise fixture.** Its entire initial body is:

```json
{"version":1,"journey":"init","preserved":["token_unchanged","other_mcp_preserved","unrelated_fields_preserved"],"required":["init_creates_local_runtime","malformed_mcp_refused_without_rewrite","recovery_succeeds"]}
```

Keep this pilot manifest distinct from the normative journey-contract schema. Compile its six IDs into schema-valid observation obligations in Task 2 using the existing contract fields; never mislabel this small manifest as a full JourneyContract. Freeze the previous manifest alongside each future revision and require every removed promise to have an explicit approved contract change.
- [ ] **Step 4: Update the extension inventory and validate.** `$env:CARGO_TARGET_DIR='E:/_agent-scratch/graphhelm/methodology-adoption-jpd/target'; cargo +1.97.1 run --locked -p graphhelm-cli -- extension validate extensions/builtin/graphhelm-jpd`. Expect PASS.
- [ ] **Step 5: Commit the passing characterization and fixture.** `git add apps/cli/tests/jpd_pilot_journey.rs extensions/builtin/graphhelm-jpd/fixtures/pilot/init-promises.json extensions/builtin/graphhelm-jpd/extension.json`; commit subject `test(jpd): freeze observable init promises` with the required session/head identity body.

### Task 2: Capture independent facts through a bounded observer

**Files:** Create `core/protocols/src/journey_observation.rs` and export from `core/protocols/src/lib.rs`; create `adapters/tool-host/src/journey_observer.rs` and export from `adapters/tool-host/src/lib.rs`; create `adapters/tool-host/tests/journey_observer.rs`; create `tools/pathogens/src/jpd_pilot.rs` and export from `tools/pathogens/src/lib.rs`.

**Interfaces:** Protocol types carry `ObservationKey` below and a serializable observation reference. The adapter owns capture and lookup; the pure evaluator receives resolved captured facts. `pub fn capture_init(&mut self, key: ObservationKey, root: &Path, token_before: Option<&[u8]>, mcp_before: &[u8]) -> Result<ObservationRef, CaptureError>` captures only the allowlisted init artifacts. `pub fn resolve(&self, reference: &ObservationRef, expected: &ObservationKey) -> Result<&CapturedInitFacts, CaptureError>` accepts only a reference issued by this recorder. `ObservationRef` has a private ID; `CapturedInitFacts` contains the six promise booleans, redacted evidence digests, and the key. `CaptureError` is the closed enum `Missing, BindingMismatch, SelfObservation, UnsafePath, TooLarge, Unreadable`.

- [ ] **Step 1: Add the binding type and failing unit test in the adapter module.** The structural key is shared protocol data; capture storage and validation stay private to the adapter.

```rust
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ObservationKey {
    pub run: String,
    pub revision: String,
    pub configuration_digest: String,
    pub contract_digest: String,
    pub actor: String,
    pub observer: String,
}
fn validate_key(captured: &ObservationKey, expected: &ObservationKey) -> Result<(), CaptureError> {
    if expected.actor == expected.observer { return Err(CaptureError::SelfObservation); }
    if captured != expected { return Err(CaptureError::BindingMismatch); }
    Ok(())
}
#[test]
fn capture_is_bound_to_run_and_independent_observer() {
    let key = ObservationKey { run: "run-1".into(), revision: "rev-1".into(),
        configuration_digest: "config-1".into(), contract_digest: "contract-1".into(),
        actor: "init-child".into(), observer: "filesystem-observer".into() };
    assert_eq!(validate_key(&key, &key), Ok(()));
    let mut other = key.clone();
    other.run = "run-2".into();
    assert_eq!(validate_key(&key, &other), Err(CaptureError::BindingMismatch));
    other = key.clone(); other.observer = other.actor.clone();
    assert_eq!(validate_key(&key, &other), Err(CaptureError::SelfObservation));
}
```

`CaptureError` derives `Debug, PartialEq, Eq`. Run `$env:CARGO_TARGET_DIR='E:/_agent-scratch/graphhelm/methodology-adoption-jpd/target'; cargo +1.97.1 test -p graphhelm-tool-host journey_observer --locked`; first expect missing implementation, then PASS after the code is added.
- [ ] **Step 2: Implement capture custody.** The recorder opens only `.mcp.json`, `.graphhelm/events.token`, and runtime layout entries under a retained project root. Reject link/reparse escapes and files above 1 MiB before parsing. Compare the token locally without exporting its bytes or digest; emit only equality/presence facts. Never read unrestricted project instructions. Digest the redacted capture, store it in a private append-only run ledger, and bind the ledger entry to all key fields. A caller-provided reference resolves by ledger lookup, never by trusting attached JSON or a self-reported digest. Capture the pre-init values before the child runs; post-init observation happens only after it exits.
- [ ] **Step 3: Exercise custody beyond the key unit test.** Use `tempfile` projects and the Task 1 child command. Mutate run, revision, configuration and contract fields one at a time; resolve must return `BindingMismatch`. An unknown ledger ID returns `Missing`; an artifact changed after capture cannot replace the immutable captured bytes. Use platform-specific link fixtures to require `UnsafePath`. Assert serialized facts contain neither token bytes nor absolute home paths.
- [ ] **Step 4: Compile observations into the existing result schema.** Use `observer-capability.schema.json`, `observation-obligation.schema.json`, and `journey-verification-result.schema.json` without adding alternate status words. Missing capture maps to `gate.status=capability_missing`, `gate.reasonCode=OBSERVER_MISSING`, `proposedResultStatus=unresolved`. A deterministic evaluator checks every frozen obligation against recorder facts. Generated results must pass `OfflineSchemaSet::validate` against the actual `$id`; schema lookup failure is a harness error. Contract version/digest must match the capture.
- [ ] **Step 5: Commit after observer tests pass.** Stage only this task's protocol, adapter, and pathogens files; subject `feat(jpd): capture bound init observations`. Include exact scope and custody limitations in the commit/PR evidence.

### Task 3: Retain retries without changing the established lineage contract

**Files:** Modify `tools/pathogens/src/retry_lineage.rs` only for additional tests or a demonstrated missing check; modify `apps/cli/tests/jpd_retry_semantics.rs`; create `extensions/builtin/graphhelm-jpd/fixtures/pilot/init-recovered-retry-chain.json`; modify `extensions/builtin/graphhelm-jpd/extension.json`.

**Interfaces:** Consumes existing `retry_lineage::failing_checks(&serde_json::Value) -> Vec<LineageCheck>` and the schema fields `rootAttempt`, `retries`, `firstFailure`, `successfulAttemptId`. Produces the same contract, populated with actual capture references; no invented `attempts` array or `JpdEvidence::document()` method.

- [ ] **Step 1: Add a concrete anti-laundering characterization beside the existing retry evaluator tests.**

```rust
#[test]
fn changing_the_root_does_not_erase_the_first_failure() {
    let mut document: serde_json::Value = serde_json::from_str(include_str!(
        "../../../extensions/builtin/graphhelm-jpd/fixtures/positive/recovered-retry-chain.json"
    )).unwrap();
    assert!(failing_checks(&document).is_empty());
    document["rootAttemptId"] = serde_json::json!("attempt/replacement");
    assert!(!failing_checks(&document).is_empty());
}
```

This existing guarantee should pass; missing capture-to-lineage joining is the new behavior. Run `$env:CARGO_TARGET_DIR='E:/_agent-scratch/graphhelm/methodology-adoption-jpd/target'; cargo +1.97.1 test -p pathogens retry_lineage --locked`.
- [ ] **Step 2: Create the pilot fixture by copying the checked-in recovered fixture's full schema shape.** Replace run/attempt identities, fixed test timestamps, summary and evidence references with init refusal/recovery captures. Retain the refused MCP attempt as the root; the repaired MCP input produces a material evidence delta. Use the existing accepted recovery cause tag. Never classify recovery as first-pass success.
- [ ] **Step 3: Add missing-join tests in `jpd_retry_semantics.rs`.** Resolve both root and retry evidence through Task 2's ledger; reject an absent root capture, reference from another run, unchanged evidence after a claimed repair, and `flaky_pass`. Assert the existing retry classifier's blocker status rather than assuming lineage validity alone proves a retry's quality. Keep a recovered positive control with its first failure still present.
- [ ] **Step 4: Run `$env:CARGO_TARGET_DIR='E:/_agent-scratch/graphhelm/methodology-adoption-jpd/target'; cargo +1.97.1 test -p graphhelm-cli --test jpd_retry_semantics --locked`**, update fixture digests, and commit `test(jpd): preserve captured retry failures`. A new runtime join is delivered with Task 5; fixture checks alone do not claim production integration.

### Task 4: Select old promises conservatively

**Files:** Create `core/policy/src/journey_regressions.rs`; modify `core/policy/src/lib.rs` to export it. Tests live beside this pure implementation.

**Interfaces:** `select_regressions(baseline: &[RegressionCase], changed: &BTreeSet<String>, coverage: Coverage) -> Vec<RegressionCase>`; baseline contains previous and new obligations. `FreshComplete` may only be supplied after the caller checks graph/index generation and bounded scope coverage; all uncertainty uses `Incomplete`.

- [ ] **Step 1: Add the failing test followed by the minimal pure implementation.**

```rust
use std::collections::BTreeSet;
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RegressionCase { pub id: String, pub dependencies: BTreeSet<String>, pub critical: bool }
#[derive(Clone, Copy)]
pub enum Coverage { FreshComplete, Incomplete }
pub fn select_regressions(baseline: &[RegressionCase], changed: &BTreeSet<String>, coverage: Coverage) -> Vec<RegressionCase> {
    baseline.iter().filter(|case| matches!(coverage, Coverage::Incomplete)
        || case.critical || case.dependencies.is_empty()
        || !case.dependencies.is_disjoint(changed)).cloned().collect()
}
#[test]
fn incomplete_coverage_preserves_the_entire_baseline() {
    let baseline = vec![RegressionCase { id: "preserve-token".into(),
        dependencies: BTreeSet::from(["init".into()]), critical: true },
        RegressionCase { id: "unrelated-flow".into(), dependencies: BTreeSet::from(["other".into()]), critical: false }];
    let changed = BTreeSet::from(["init".into()]);
    assert_eq!(select_regressions(&baseline, &changed, Coverage::Incomplete), baseline);
    assert_eq!(select_regressions(&baseline, &changed, Coverage::FreshComplete).len(), 1);
}
```

- [ ] **Step 2: Run `$env:CARGO_TARGET_DIR='E:/_agent-scratch/graphhelm/methodology-adoption-jpd/target'; cargo +1.97.1 test -p graphhelm-policy journey_regressions --locked`.** First test before implementation must fail; after implementation expect PASS.
- [ ] **Step 3: Add critical, unbound dependency, and changed shared-dependency cases to the same test module.** Critical obligations always run. Unknown dependency sets always run. A stale index is mapped to `Incomplete` by the caller, not relabeled complete because a graph query returned no matches. The pilot starts with full-baseline selection; narrowing is enabled only when a real coverage provider satisfies this contract.
- [ ] **Step 4: Commit `feat(policy): select journey regressions conservatively`.** Stage only the policy files. No CLI business rules or graph mutations are introduced.

### Task 5: Ship the bounded pilot command and sabotage acceptance

**Files:** Create `apps/cli/src/commands/jpd_pilot.rs`; modify `apps/cli/src/commands/mod.rs`, `apps/cli/src/args.rs`, and `apps/cli/src/commands/quality.rs`; create `apps/cli/tests/jpd_pilot_sabotage.rs`; create `docs/harness/JPD_PILOT_OBSERVER_HOST.md`; modify `docs/harness/JOURNEY_PROVEN_DEVELOPMENT.md`; modify `extensions/builtin/graphhelm-jpd/extension.json` for a new versioned pilot gate contribution.

**Interfaces:** Add `graphhelm quality pilot-init --output DIRECTORY`. `pub fn run_pilot_init(output: &Path) -> Outcome` is the CLI adapter; it creates a fresh temporary git project, invokes current-executable init, calls the observer, joins retries and renders the existing JSON envelope. Output must be a new empty directory, never an arbitrary source project. Refuse preexisting output rather than overwrite evidence. The pure pilot evaluator is registered as a new `gate-jpd-init-pilot` through the existing quality registry and Runtime gate path.

- [ ] **Step 1: Write the public command test.**

```rust
#[test]
fn pilot_runs_the_real_child_and_emits_an_observed_result() {
    let temp = tempfile::tempdir().unwrap();
    let destination = temp.path().join("proof");
    let output = assert_cmd::Command::new(assert_cmd::cargo::cargo_bin!("graphhelm"))
        .args(["quality", "pilot-init", "--output"]).arg(&destination)
        .output().unwrap();
    assert!(output.status.success());
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["ok"], true);
    assert!(destination.join("verification.json").is_file());
    assert!(destination.join("retry-chain.json").is_file());
}
```

Run `$env:CARGO_TARGET_DIR='E:/_agent-scratch/graphhelm/methodology-adoption-jpd/target'; cargo +1.97.1 test -p graphhelm-cli --test jpd_pilot_sabotage --locked`; expect unknown command before implementation.
- [ ] **Step 2: Compose the command from Tasks 1-4.** Freeze contract and pre-state before executing; derive facts from captured files, not stdout. Capture every child attempt including failures. Emit redacted verification and retry-chain files validated against checked-in schemas. Publish any execution signals only through existing typed Runtime/Governor contracts. Missing observers yield unresolved evidence and a non-success envelope; never manufacture a proven result to make the command green.
- [ ] **Step 3: Add a sabotage matrix using private test injection into the adapter.** Delete the other MCP entry, rotate the old token, remove the root attempt, substitute contract/revision/run identity, make the observer equal the actor, omit observation, and use a missing schema `$id`. Each case must have an unsabotaged positive control. First cases fail their named obligation; missing observer uses `OBSERVER_MISSING`; broken schema lookup is harness-broke, never a successful product refusal. Do not expose sabotage switches as user commands.
- [ ] **Step 4: Document the proof boundary.** Update stale JPD text listing only geometry: record existing three gates and the newly registered pilot. A local filesystem proof does not prove browser rendering, focus, keyboard reachability, provider delivery, or fresh Claude/Codex activation. The host recipe requires a separately enabled observer, frozen revision/config/contract, immutable redacted capture references and semantic selectors where a UI is involved. Absent capability remains `OBSERVER_MISSING`. Setup host activation is owned by the setup plan, not inferred from init's files. No documentation keyword test is needed.
- [ ] **Step 5: Run all focused pilot/observer/policy/retry tests and `extension validate`, then commit `feat(cli): expose observed init journey pilot`.** Review the entire generated result for absence of token bytes, token digests and private paths.

## Execution preflight and release

- [ ] Create/link an assigned scoped issue with exactly one permitted label before code; use `issue-<N>-journey-proof-pilot`, replacing N with that issue. Every commit body carries measured session/head identity. Do not reuse the historical Foundation-only branch instruction for this later capability.
- [ ] Run the complete local gate on final source, after checking disk floors and gate slots. Capture the exit code of the gate itself without a pipe: `$env:CARGO_TARGET_DIR='E:/_agent-scratch/graphhelm/methodology-adoption-jpd/target'; ./ci/gate.ps1 > E:/_agent-scratch/graphhelm/methodology-adoption-jpd/gate.log 2>&1; echo $LASTEXITCODE`.
- [ ] Obtain required two independent nonauthor lane reviews and third-lane merge. Run closing-keyword guard and main-owned `merge-proof-from-main.ps1` against the candidate. Verify the pilot on merged source before claiming delivery; record host-observer coverage separately.
