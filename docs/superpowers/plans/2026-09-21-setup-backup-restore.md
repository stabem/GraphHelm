# Setup, Backup, and Restore Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Adopt GraphHelm into an existing Claude/Codex environment with a complete compatibility report, reviewed changes, automatic private backups, verified activation, and conflict-aware restoration.

**Architecture:** Keep adoption contracts in protocols, pure approval/classification/restore policy in core policy, and host/filesystem effects in a focused host-adoption adapter. The CLI composes the existing init and Extension lifecycle with a durable migration journal; no host file is edited until the reviewed plan is still current and its backup is verified. A new-session observer supplies activation evidence separately from file installation.

**Tech Stack:** Rust 1.97.1, edition 2024, existing Serde/JSON/TOML, SHA-256, fs2, platform retained-handle filesystem APIs, Clap, assert_cmd, tempfile and existing Extension/public MCP contracts.

**Spec:** [GraphHelm methodology adoption and validation](../specs/2026-09-21-graphhelm-methodology-adoption-design.md), especially sections 3, 4 and 9. Also read D-039, D-041, D-054 and D-056 in `docs/DECISION_REGISTER.md`.

## Global Constraints

- Rust 1.97.1; edition 2024.
- English repository documentation.
- One Extension model; host wrappers do not create a second package authority.
- Only the Governor publishes operational graph mutations.
- Core modules depend on interfaces, never concrete adapters.
- Unknown evidence never becomes proven evidence.
- Subscription exhaustion pauses; no automatic paid fallback.
- Offline tests require no network, browser session, credentials, or provider account.
- `ci/gate.ps1` is the authoritative local gate; GitHub Actions stays disabled.
- Issue-first implementation; no production code before an assigned GitHub issue exists.
- Preserve unrelated user changes; no blanket replacement of existing configuration.
- No secrets in prompts, logs, public plans, receipts, fixtures, or exported manifests.
- All newly introduced bounds are explicit, enforced, and tested at both sides.

## Review Focus

1. Inherited, managed, duplicate, disabled and unreadable skills/configuration must not disappear into an apparently complete inventory: Task 1.
2. A malicious skill, ambiguous mixed rule, secret-bearing setting or confident classifier must not obtain permission to disable a security control: Task 2.
3. Links, file swaps, access metadata and a backup failure must not expose secrets or overwrite the wrong file: Task 3.
4. Crash, concurrent edit, changed package or repeated setup must not lose the original baseline or silently leave a partially active installation: Tasks 4 and 5.
5. Later user edits, overlapping adoptions and missing host observation must survive restore and must not be reported as verified success: Tasks 6 and 7.

---

## Starting evidence and execution preflight

Planning inspected `026718e51ffdc20611c97fd8705ca56052c7c7d6`; rerun source discovery on the execution revision. Existing entry points are `apps/cli/src/args.rs:24`, `apps/cli/src/commands/mod.rs:1`, `apps/cli/src/commands/init.rs:154`, `core/extension-host/src/install.rs:294`, and `adapters/tool-host/src/documents.rs:110`. The existing document adapter is not a complete backup API: it only replaces bounded, existing UTF-8 documents.

Before implementation, create/link an assigned issue with exactly one allowed label, use its `issue-N-description` branch, record the session identity required by AGENTS.md, and read `.factory/MERGE-CHECKLIST.md`. This planning change does not create a code branch, issue, PR, host mutation or paid judge call. Do not reuse the historical Foundation branch/issue for this new feature.

Use the existing checked-in dependency versions. Adding an adapter crate is justified by actual executable host/backup behavior in Task 1; do not create empty future modules. A formatting-preserving TOML editor is not currently a workspace dependency: this plan initially preserves TOML semantics and retains original bytes for exact restore, while showing formatting changes in the preview. Do not claim comment-preserving edits from `toml::Value` serialization.

Before any Cargo invocation, verify `Get-PSDrive E` has more than 30 GB free, inspect the live gate launches using the merge checklist, and ensure at most two gates total. Every command below sets its own target on the same line. Logs go under `D:/_agent-scratch/graphhelm/methodology-adoption/`; remove only owned scratch when the lane closes.

## File map and ownership

| Files | Responsibility |
|---|---|
| `core/protocols/src/adoption.rs`, `core/protocols/src/lib.rs` | Versioned adoption document types and closed refusal/state vocabulary |
| `core/policy/src/adoption.rs`, `core/policy/src/lib.rs` | Pure approval checks and three-way restore decisions |
| `adapters/host-adoption/Cargo.toml`, `adapters/host-adoption/src/lib.rs` | Real adapter entry points and exports |
| `adapters/host-adoption/src/inventory.rs` | Bounded discovery, precedence and coverage |
| `adapters/host-adoption/src/hosts/mod.rs`, `claude.rs`, `codex.rs` | Host-specific configuration parsing, capabilities and supported operations |
| `adapters/host-adoption/src/classification.rs` | Redacted classification proposals; optional judge port |
| `adapters/host-adoption/src/storage.rs`, `storage/unix.rs`, `storage/windows.rs` | Anchored filesystem access and preserved security metadata |
| `adapters/host-adoption/src/backup.rs` | Private immutable snapshots and backup verification |
| `adapters/host-adoption/src/journal.rs`, `apply.rs`, `restore.rs` | Durable transition journal, recovery and restoration |
| `adapters/host-adoption/src/observation.rs` | Activation receipt verification |
| `apps/cli/src/commands/adoption.rs` | Thin setup/backup/restore adapters |
| `apps/cli/src/args.rs`, `commands/mod.rs`, `error_codes.rs` | Public CLI, dispatch and registered diagnostic |
| `apps/cli/src/commands/init.rs` | Extract the provisioning plan so adoption does not call an unjournaled writer |
| `Cargo.toml`, `Cargo.lock`, `apps/cli/Cargo.toml` | Add the implemented adapter and existing workspace dependencies |
| `schemas/adoption.schema.json`, `core/schema/src/registry.rs` | Offline schema registration |
| `adapters/host-adoption/tests/*.rs`, `apps/cli/tests/adoption_*.rs` | Boundary, recovery and public CLI tests |
| `docs/install/GETTING_STARTED.md`, `docs/acceptance/adoption-rehearsal.md` | User commands and host observation recipe |

Do not move unrelated CLI/runtime code. No Studio, marketplace service, browser engine or new memory store is included. No MCP mutation endpoint is added for personal host migration: setup runs as the local owner CLI, not as a remote agent privilege escalation.

## Public command and document contracts

Terminal `graphhelm setup` runs inventory and review, then asks to apply the exact digest shown. Nonterminal `graphhelm setup` defaults to a JSON preview without mutations. Explicit scripted operations:

```text
graphhelm setup --project PATH --home PATH --dry-run --out PLAN
graphhelm setup --project PATH --home PATH --scope user --dry-run --out PLAN
graphhelm setup --project PATH --home PATH --apply PLAN --accept SHA256
graphhelm setup --project PATH --home PATH --recover TRANSACTION_ID
graphhelm setup --project PATH --home PATH --verify --receipt RECEIPT
graphhelm backup --project PATH --home PATH
graphhelm backup --project PATH --home PATH --list
graphhelm restore --project PATH --home PATH --original --dry-run --out PLAN
graphhelm restore --project PATH --home PATH --backup BACKUP_ID --dry-run --out PLAN
graphhelm restore --project PATH --home PATH --apply PLAN --accept SHA256
```

`--home` is an explicit host-root override for portable installations and tests, not permission to change that root. Project is the default write scope; user scope must be in the accepted plan. `--state-root` is an optional explicit private-state location, validated against project/source roots. Paths and permissions are reviewed, not inferred from a model. Backup IDs are opaque identifiers, never arbitrary paths.

All documents have `apiVersion: p50.dev/adoption/v1`, a closed `kind`, `id`, and `spec`. Kinds: `Inventory`, `AdoptionPlan`, `BackupReceipt`, `ApplyReceipt`, `RestorePlan`, `ActivationReceipt`. Schemas define all required fields, reject unknown major versions, cap arrays, and preserve documented compatible minor fields. Canonical hashes exclude only the enclosing `digest`; hash unknown payload fields as well. Use the existing canonicalizer rather than JSON formatting or filesystem order.

Required Inventory spec: `roots`, `hosts`, `items`, `coverage`. Each item records `id`, opaque root ID + relative path, host, scope, kind, installed/enabled/loaded states, content digest, origin, and protected status. Coverage records include scanned roots, accessible/unsupported/truncated state and reason. Never emit source contents by default.

Required AdoptionPlan spec: inventory digest, source hashes, exact changed ranges/keys, before/after hashes, package IDs/digests, effects, decisions with reason/evidence, scopes, coverage and unresolved items. The private plan payload holds reviewed after-bytes separately from the redacted public view; never serialize credentials through the public JSON renderer. An edited plan must be revalidated and re-digested before approval.

Initial enforced limits: 1 MiB per config/instruction file, 64 MiB aggregate inventory bytes, 10,000 entries, import depth 16, 256 host operations per transaction, 1 GiB snapshot total, 10 MiB per journal, and 10 seconds per host subprocess including output collection and termination. Limit exhaustion records incomplete coverage or a typed refusal. Tests hit N and N+1. No filesystem walk of an entire user home.

Register `GHCLI029_ADOPTION_REFUSED` in `apps/cli/src/error_codes.rs` and its `ALL` list, provided rebase-time registry inspection confirms 029 remains free. A collision requires allocating the next free number and updating every plan-owned assertion consistently. Closed reason tags live in protocols: `coverage_incomplete`, `review_required`, `protected_rule`, `plan_stale`, `path_unsafe`, `backup_unverified`, `backup_corrupt`, `busy`, `host_unsupported`, `host_action_required`, `managed_conflict`, `restore_conflict`, `recovery_required`, `observer_missing`, `receipt_invalid`, `limit_exceeded`. Put the reason in the diagnostic pointer `/adoption/<reason>` and use safe fixed messages.

## Task 1: Deliver a complete, read-only compatibility inventory

**Files:** Create protocols `adoption.rs`, adapter crate `lib.rs`, `inventory.rs`, `hosts/mod.rs`, `hosts/claude.rs`, `hosts/codex.rs`, `schemas/adoption.schema.json`, CLI `commands/adoption.rs`, `apps/cli/tests/adoption_inventory.rs`. Modify the workspace manifest, lock, owning module exports, schema registry, CLI manifest/args/dispatch/error registry listed above.

**Interfaces:** Consumes explicit `std::path::Path` roots and existing package manifests. Produces `pub fn inventory(project: &Path, home: &Path) -> Result<serde_json::Value, AdoptionError>` in `graphhelm_host_adoption`, returning a validated Inventory envelope. Define `AdoptionError { pub reason: AdoptionReason }` in protocols, with the reason enum above and safe `Display`; do not retain source bytes in errors. Host code exposes `pub fn inspect(project: &Path, home: &Path) -> Result<serde_json::Value, AdoptionError>` in each host module. `inventory` combines host records in stable order and enforces aggregate limits.

- [ ] **Step 1: Add this real CLI regression test.**

```rust
#[test]
fn preview_keeps_existing_settings_byte_identical() {
    let project = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    std::fs::create_dir(home.path().join(".claude")).unwrap();
    let path = home.path().join(".claude/settings.json");
    let original = br#"{"permissions":{"deny":["Read(.env)"]},"unknown":7}"#;
    std::fs::write(&path, original).unwrap();
    let output = assert_cmd::Command::new(assert_cmd::cargo::cargo_bin!("graphhelm"))
        .args(["setup", "--dry-run", "--project"])
        .arg(project.path()).arg("--home").arg(home.path())
        .output().unwrap();
    assert!(output.status.success());
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["data"]["inventory"]["kind"], "Inventory");
    assert_eq!(std::fs::read(path).unwrap(), original);
}
```

- [ ] **Step 2: Run the test and record the red CLI result.**

```powershell
$env:CARGO_TARGET_DIR='E:/_agent-scratch/graphhelm/methodology-adoption/target'; cargo +1.97.1 test -p graphhelm-cli --test adoption_inventory --locked
```

Expected: setup is not recognized before implementation. A compile failure in test syntax is not the behavioral red.

- [ ] **Step 3: Implement bounded inventory and preview.** Add a real `Setup(SetupArgs)` branch and route it to `adoption::run_setup`. The first slice supports preview only; do not expose apply/backup/restore flags until their working task lands. Use host parsers for `.claude/settings.json`, `.claude/settings.local.json`, project `.mcp.json`, user `.claude.json` MCP metadata, `.codex/config.toml`, configured Codex profile files, `CLAUDE.md`, `AGENTS.md`, overrides/imports, and supported skill/plugin discovery roots. Resolve actual host precedence using captured version/capability data. Treat managed roots as read-only observations. Parse JSON with serde_json and TOML with the existing workspace toml crate; never execute a discovered hook to inspect it.

The central completeness predicate in `core/protocols/src/adoption.rs` is:

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Coverage { Complete, Inaccessible, Unsupported, Truncated }

pub fn coverage_complete(scopes: &[Coverage]) -> bool {
    !scopes.is_empty() && scopes.iter().all(|s| *s == Coverage::Complete)
}
```

File discovery records disabled skills and multiple instances separately by origin/root/path. A linked skill outside allowed roots is listed as a coverage limitation; do not follow it silently. Recursively imported instructions use a visited identity set and the depth bound. Never claim every machine installation was scanned when a host API is unavailable.

- [ ] **Step 4: Add and run controls for empty inventory, malformed JSON/TOML, duplicate skill names from different roots, inherited instructions, managed restrictions, cyclic imports, non-UTF-8 paths, Windows case aliases, unreadable directories and each size boundary.** For the pure boundary add:

```rust
#[test]
fn incomplete_scope_is_not_complete_inventory() {
    use graphhelm_protocols::adoption::{coverage_complete, Coverage};
    assert!(!coverage_complete(&[]));
    assert!(!coverage_complete(&[Coverage::Complete, Coverage::Unsupported]));
    assert!(coverage_complete(&[Coverage::Complete]));
}
```

Run the Step 2 command plus `cargo +1.97.1 test -p graphhelm-host-adoption --locked` with the same target prefix. Confirm all fixtures use explicit temporary homes, not the operator's real configuration.

- [ ] **Step 5: Review and commit only Task 1 paths.** Use `feat(adoption): inventory host compatibility without mutations`; put the measured session/head identity first in the commit body, followed by `Refs #N` for the assigned issue. Every subsequent commit follows this same identity rule; N is obtained from the issue-first preflight, not invented by this plan.

## Task 2: Produce reviewable methodology decisions without granting model authority

**Files:** Create `core/policy/src/adoption.rs`, `adapters/host-adoption/src/classification.rs`, `adapters/host-adoption/tests/classification.rs`; modify their owning exports, adoption schema, CLI preview and `docs/DECISION_REGISTER.md` only after an explicit decision entry is accepted. Record proposed Jev adoption-classification scope in `docs/rfcs/2026-09-21-adoption-classification.md` before enabling its production consumer.

**Interfaces:** `pub enum Decision { Keep, Disable, Replace, Unresolved }`; `pub fn decision_allowed(protected: bool, decision: Decision) -> bool` in core policy. Adapter `pub fn propose(inventory: &Value, classifications: &Value) -> Result<Value, AdoptionError>` produces an AdoptionPlan. `pub trait RuleClassifier { fn classify(&self, redacted_rules: &Value) -> Result<Value, AdoptionError>; }` accepts only source-bound excerpts and returns decisions with confidence and evidence references. Define its recorded implementation with a `Value` field and an offline implementation that returns unresolved for ambiguous text. A live implementation uses the existing typed System One transport after the RFC consumer boundary is approved; no mandatory provider dependency for setup.

- [ ] **Step 1: Add the protected-category regression.**

```rust
#[test]
fn classifier_cannot_disable_security_even_with_high_confidence() {
    use graphhelm_policy::adoption::{decision_allowed, Decision};
    assert!(!decision_allowed(true, Decision::Disable));
    assert!(!decision_allowed(true, Decision::Replace));
    assert!(decision_allowed(true, Decision::Keep));
    assert!(decision_allowed(false, Decision::Disable));
}
```

- [ ] **Step 2: Run `cargo +1.97.1 test -p graphhelm-policy classifier_cannot_disable_security --locked` with the target prefix.** Record the missing/new-policy red, then test the same behavior through a recorded malicious classification input in the adapter suite.

- [ ] **Step 3: Implement the pure decision boundary and proposal builder.**

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Decision { Keep, Disable, Replace, Unresolved }

pub fn decision_allowed(protected: bool, decision: Decision) -> bool {
    !protected || matches!(decision, Decision::Keep | Decision::Unresolved)
}
```

Protection comes from deterministic host key categories, managed scope and existing enforced policy, not the classifier's category label. Unknown prose stays unresolved unless the owner approves exact replacement ranges. Keep preferences and project facts as separate source-bound records. Render decisions with before/after effects, host and scope; never replace the entire instruction file merely because it contains a factory keyword. Jev receives only redacted instruction excerpts, never full settings, hooks, environment values or backup files. Recorded results bind inventory/source digest and model version; stale results refuse.

- [ ] **Step 4: Add recorded cases for mixed security/methodology paragraphs, prompt injection, missing/low-confidence classification, unknown keys, inherited conflicting method, global write proposed under project scope, and a secret marker.** Assert the fake classifier receives no marker; assert unresolved items block apply. Add approval hashing tests where changing one accepted operation invalidates its digest. Run policy and adapter suites with the target prefix and inspect the exact pointers, not prose only.

- [ ] **Step 5: Commit Task 2 paths with `feat(adoption): compile reviewed methodology proposals`.** Keep RFC acceptance and model calibration evidence distinct from test fixtures; fixtures do not prove classification accuracy.

## Task 3: Ship protected, verifiable manual backups

**Files:** Create adapter `storage.rs`, `storage/unix.rs`, `storage/windows.rs`, `backup.rs`, `tests/backup.rs`, `tests/storage_races.rs`, CLI `tests/adoption_backup.rs`; extend protocols/schema/CLI with backup only when functional.

**Interfaces:** `pub fn backup(project: &Path, home: &Path, state_root: &Path) -> Result<Value, AdoptionError>` returns BackupReceipt. `pub fn verify_backup(state_root: &Path, id: &str) -> Result<Value, AdoptionError>` verifies bytes and metadata. Storage owns `pub struct AnchoredRoot` and methods `open(path: &Path)`, `read(relative: &str) -> Result<Vec<u8>, AdoptionError>`, and `replace(relative: &str, expected: Option<&str>, bytes: Option<&[u8]>) -> Result<(), AdoptionError>`; `None` expected means create-only, `None` bytes means delete only an expected owned file. The concrete platform code must preserve metadata, compare identity and use retained parent handles. It is not a wrapper around `canonicalize` plus `std::fs::write`.

- [ ] **Step 1: Add the byte-preserving backup test.**

```rust
#[test]
fn manual_backup_is_verified_without_changing_the_source() {
    let project = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    let path = project.path().join("AGENTS.md");
    let bytes = b"\xef\xbb\xbfKeep this exact file.\r\n";
    std::fs::write(&path, bytes).unwrap();
    let receipt = graphhelm_host_adoption::backup(project.path(), home.path(), state.path()).unwrap();
    let id = receipt["id"].as_str().unwrap();
    let checked = graphhelm_host_adoption::verify_backup(state.path(), id).unwrap();
    assert_eq!(checked["spec"]["verified"], true);
    assert_eq!(std::fs::read(path).unwrap(), bytes);
}
```

- [ ] **Step 2: Run `cargo +1.97.1 test -p graphhelm-host-adoption --test backup --locked` with target prefix.** Record the red before implementing backup.

- [ ] **Step 3: Implement snapshots with access restrictions applied before plaintext is written.** Use private root descriptors, create-new files, copied bytes and platform metadata, durable sync, then publish the completed snapshot directory. Restrict manifest contents as well as blobs. SHA-256 binds bytes; filesystem custody/access control protects same-user state, and the threat model must not claim protection from a fully compromised owner account. Do not use a repository file as the trust anchor. Reject path escapes, volume aliases, reparse points and hard-linked files before writing; register retained-handle race tests matching both supported platforms.

Backup ID validation is a pure code path, defined and tested in `backup.rs`:

```rust
pub fn valid_backup_id(id: &str) -> bool {
    id.len() == 64 && id.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

#[test]
fn a_backup_id_is_never_a_path() {
    assert!(!valid_backup_id("../../.ssh"));
    assert!(!valid_backup_id("C:\\outside"));
    assert!(valid_backup_id(&"a".repeat(64)));
}
```

IDs are the digest of the canonical complete manifest; explicit absence records distinguish files created later from pre-existing empty files. Snapshot selected installed package identity/activation state and original instruction bytes. Include the required local package content in the recovery capsule where restore needs it offline. Do not recursively copy unrelated package caches or the backup root itself.

- [ ] **Step 4: Add platform tests for ACL/security xattr preservation, link/path swap between read and publish, non-ASCII roots, corrupt manifests/blobs, disk-write failure, absent files, 1 GiB boundary, and secret canaries absent from stdout/stderr.** Corrupt one stored blob and require `backup_corrupt`; a positive unmodified control must still verify. Run backup/storage tests plus CLI backup test; inspect temporary artifacts' permissions before cleanup.

- [ ] **Step 5: Commit with `feat(adoption): create verified private configuration backups`.** Manual backup is independently usable at this point; setup still cannot mutate configuration.

## Task 4: Apply accepted plans through a recoverable journal

**Files:** Create adapter `journal.rs`, `apply.rs`, `tests/apply.rs`, `tests/recovery.rs`, CLI `tests/adoption_apply.rs`; modify `init.rs` to expose a side-effect-free provisioning description consumed by both init and adoption, preserving existing init behavior and tests.

**Interfaces:** `pub fn apply(project: &Path, home: &Path, state_root: &Path, plan: &Value, accepted_digest: &str) -> Result<Value, AdoptionError>` produces ApplyReceipt. `pub fn recover(state_root: &Path, transaction_id: &str) -> Result<Value, AdoptionError>` reconciles interrupted operations. `pub fn approval_matches(actual: &str, accepted: &str) -> bool` is a pure policy helper. `pub enum TransactionState { Planned, BackedUp, Applying, InstalledUnverified, Verified, Restoring, Restored, RecoveryRequired }` is defined in protocols and serialized snake_case. Journal entries carry monotonically increasing sequence, transaction ID, operation index, expected before/after digest and state; no secret content.

- [ ] **Step 1: Add and run the approval-binding unit test.**

```rust
#[test]
fn approval_is_for_one_nonempty_exact_plan() {
    use graphhelm_policy::adoption::approval_matches;
    assert!(approval_matches("sha256:abc", "sha256:abc"));
    assert!(!approval_matches("sha256:abc", "sha256:def"));
    assert!(!approval_matches("", ""));
}
```

Run `cargo +1.97.1 test -p graphhelm-policy approval_is_for_one --locked` with target prefix. Digest syntax is validated separately by the envelope schema; this helper checks consent identity only.

- [ ] **Step 2: Implement consent and preflight before acquiring write authority.**

```rust
pub fn approval_matches(actual: &str, accepted: &str) -> bool {
    !actual.is_empty() && actual == accepted
}
```

Validate schema, digest, coverage, no unresolved decisions, source identity, target scope, package digests, protected categories and available host capabilities. Acquire the user-state lock before the project lock in a fixed order, nonblocking; a second adoption touching global state returns `busy`. Re-read all sources under their anchored handles. Create and verify the immutable original baseline if none exists, then write the journal intent. Never call the existing init writer before this boundary.

- [ ] **Step 3: Implement one durable state transition at a time.** Record each intended operation before publication, recheck source identity, publish through retained handles, verify after-state, and sync the operation receipt. Linux uses atomic exchange. Windows uses journaled, anchored no-replace renames and can expose a name-absence interval while the host is quiescent: protocol steps are bounded, but the interval has no wall-clock bound. Failure before any write leaves originals unchanged. Failure after a write attempts compensating restore only if the current file still matches this transaction's after-state. Otherwise retain all evidence and return `recovery_required`. Repeating a completed identical plan returns the prior receipt; a new plan cannot replace the original baseline. Host processes must be quiescent before activation changes. Owner-only sibling `.graphhelm-adoption-<uuid>` guards retain source bytes; exclude them from Git/archive sharing and retain them for recovery. The 16-record limit is per journal entry, not a total disk or pre-intent-guard bound. See the [guard handling recipe](../../acceptance/adoption-rehearsal.md#retained-guards-and-portability-limits).

- [ ] **Step 4: Add deterministic fault injection in the adapter test harness, not a production environment flag.** A constructor accepting a test-only failpoint callback invokes it before/after journal sync and before/after each file publication. For a two-file plan, interrupt at each boundary, reopen in a fresh process, recover and compare both originals and access metadata. Add source-edited-after-preview, disk failure after backup, duplicate apply, journal truncation, corrupt intent, second-process contention and unrelated writer controls. Corrupt journals refuse; they do not guess the missing intent. Run:

```powershell
$env:CARGO_TARGET_DIR='E:/_agent-scratch/graphhelm/methodology-adoption/target'; cargo +1.97.1 test -p graphhelm-host-adoption --test apply --test recovery --locked
$env:CARGO_TARGET_DIR='E:/_agent-scratch/graphhelm/methodology-adoption/target'; cargo +1.97.1 test -p graphhelm-cli --test init_cli --test adoption_apply --locked
```

- [ ] **Step 5: Commit with `feat(adoption): journal approved configuration migrations`.** Document that recovery is journaled across multiple files; do not call it an atomic multi-file transaction.

## Task 5: Install and activate extensions through supported host surfaces

**Files:** Extend adapter `hosts/mod.rs`, `hosts/claude.rs`, `hosts/codex.rs`, `apply.rs`; create `tests/host_operations.rs` and `apps/cli/tests/adoption_hosts.rs`; modify existing extension-host code only if a measured integration boundary requires it and add those exact files to issue scope first.

**Interfaces:** `pub struct HostOperation { pub program: std::path::PathBuf, pub args: Vec<String>, pub timeout_ms: u64 }`; `pub fn plugin_install(program: &Path, plugin_id: &str, scope: &str) -> HostOperation` in the Claude module. `pub enum HostCapability { Supported, ActionRequired, Unsupported }`; `pub fn installation_capability(host: &str, capabilities: &Value) -> HostCapability` in hosts. A process runner consumes HostOperation without a shell and returns captured bounded output plus exit and timeout state; define `pub struct HostReply { pub success: bool, pub timed_out: bool, pub stdout: Vec<u8>, pub stderr: Vec<u8> }` and `pub fn run_host(operation: &HostOperation) -> Result<HostReply, AdoptionError>`.

- [ ] **Step 1: Add the argument-safety regression.**

```rust
#[test]
fn a_plugin_identifier_is_one_argument() {
    use std::path::Path;
    let op = graphhelm_host_adoption::hosts::claude::plugin_install(
        Path::new("claude"), "graphhelm-jpd@local;echo marker", "project");
    assert_eq!(op.args, vec!["plugin", "install", "graphhelm-jpd@local;echo marker", "--scope", "project"]);
    assert_eq!(op.timeout_ms, 10_000);
}
```

Input validation must additionally reject malformed identifiers before running the operation; the control above proves no shell string interpolation even for hostile input.

- [ ] **Step 2: Run `cargo +1.97.1 test -p graphhelm-host-adoption --test host_operations --locked` with the target prefix, then implement the adapter operation.**

```rust
pub fn plugin_install(program: &std::path::Path, plugin_id: &str, scope: &str) -> HostOperation {
    HostOperation {
        program: program.to_owned(),
        args: ["plugin", "install", plugin_id, "--scope", scope]
            .into_iter().map(str::to_owned).collect(),
        timeout_ms: 10_000,
    }
}
```

Use the existing extension package validator/install/switch APIs; pin package bytes before host installation and verify them afterward. Provide both JPD and development-contract packages from a versioned local release bundle; explicit `--package` paths override the bundle for tests. Never fetch an unpinned latest package. Host views remain wrappers of the authoritative Extension identity.

- [ ] **Step 3: Implement reversible Claude and Codex configuration operations.** Claude uses verified scoped plugin commands or supported local plugin loading, preserves unrelated settings/MCPs and disables only accepted plugin IDs. Codex uses documented TOML MCP entries and `[[skills.config]]` entries for skill disabling where the detected version supports them. Do not equate `disable-model-invocation` with disabling a Claude skill. For standalone skills without a host disable switch, an accepted operation may move the whole owned skill directory to the private quarantine only after complete backup and verification; shared/global moves require user scope and unsupported/link targets refuse. No individual skill file is destroyed. Inventory hook effects but never execute unknown hooks during inspection.

Codex plugin installation requires a verified supported host interface. If only the interactive plugin browser is available, fail preflight with `host_action_required` and a concrete instruction to install the named package in that browser, then rescan. Do not partially disable the old methodology before learning installation is unavailable. A host package cache may have internal mutations; enumerate and capture every supported affected state through the host adapter, or refuse automatic rollback guarantees. Never edit undocumented plugin registries.

- [ ] **Step 4: Prove inert fake-host behavior for successful install, rejected scope, incompatible version, timeout, unbounded output, package drift, managed force-enable, missing plugin API and rollback of activation state.** Use a small fixture executable controlled by test input files; do not use the machine's real Claude/Codex. The timeout includes process termination and pipe collection; a pre-call clock check is insufficient. Run adapter host tests and CLI adoption-host tests. Confirm applying a plan with unavailable plugin support changes zero host configuration files.

- [ ] **Step 5: Commit with `feat(adoption): integrate reversible host plugin activation`.** File/host installation reports `installed_unverified`; it must not report verified methodology adoption.

## Task 6: Restore the original setup while preserving subsequent user edits

**Files:** Create adapter `restore.rs`, `tests/restore.rs`, CLI `tests/adoption_restore.rs`; extend core policy adoption helpers, schema and restore CLI.

**Interfaces:** `pub fn restore_value(base: Option<&Value>, installed: Option<&Value>, current: Option<&Value>) -> Result<Option<Value>, &'static str>` in core policy handles a single owned semantic key. Adapter `pub fn plan_restore(state_root: &Path, backup_id: &str) -> Result<Value, AdoptionError>` creates RestorePlan; `pub fn apply_restore(state_root: &Path, plan: &Value, accepted_digest: &str) -> Result<Value, AdoptionError>` creates ApplyReceipt with restored/recovery state. Whole-file exact restore uses original bytes if current equals installed; semantic merging applies only to parsed JSON/TOML keys. Arbitrary overlapping prose changes remain conflicts; do not auto-merge them with an LLM.

- [ ] **Step 1: Write and run the three-way restore test.**

```rust
#[test]
fn restore_does_not_overwrite_a_later_user_choice() {
    use graphhelm_policy::adoption::restore_value;
    let old = serde_json::json!("factory");
    let installed = serde_json::json!("graphhelm");
    let current = serde_json::json!("my-new-method");
    assert_eq!(restore_value(Some(&old), Some(&installed), Some(&current)), Err("restore_conflict"));
    assert_eq!(restore_value(Some(&old), Some(&installed), Some(&installed)), Ok(Some(old.clone())));
    assert_eq!(restore_value(Some(&old), Some(&old), Some(&current)), Ok(Some(current)));
}
```

Run `cargo +1.97.1 test -p graphhelm-policy restore_does_not_overwrite --locked` with target prefix and retain the initial red.

- [ ] **Step 2: Implement the leaf decision.**

```rust
pub fn restore_value(
    base: Option<&serde_json::Value>,
    installed: Option<&serde_json::Value>,
    current: Option<&serde_json::Value>,
) -> Result<Option<serde_json::Value>, &'static str> {
    if current == installed || current == base { return Ok(base.cloned()); }
    if base == installed { return Ok(current.cloned()); }
    Err("restore_conflict")
}
```

Enumerate the union of owned keys, preserving unrelated keys from current state. Array edits require stable element identity from the host contract; without it, treat the array as one conflicting value. Source imports and free text use exact reviewed byte ranges with surrounding digest bindings; drift produces an explicit conflict.

- [ ] **Step 3: Wire offline restore through the same storage/journal engine.** Verify the selected backup before mutation, snapshot the current environment, resolve original versus checkpoint identity, and preview effects. Restore plugin enablement, MCP/configuration keys, original instructions and access metadata. Remove only unchanged adoption-owned artifacts. Shared user-scope entries retain references from other active adoptions; reverting one project cannot disable another project's GraphHelm. A second overlapping adoption must either reference the first owner or be refused before creating conflicting ownership. Runtime keys/tokens created by setup may be removed only if unchanged and unused; never delete user execution data during a configuration restore.

- [ ] **Step 4: Test exact byte restoration, disjoint later edit retained, same-key conflict, original backup after an upgrade/manual checkpoint, missing/corrupt backup, restore interruption, original absent files, externally modified created files and two projects sharing a user plugin.** Include an end-to-end fresh temporary home where setup then restore reproduces the original file tree bytes and metadata, apart from the explicitly retained private recovery history. Run:

```powershell
$env:CARGO_TARGET_DIR='E:/_agent-scratch/graphhelm/methodology-adoption/target'; cargo +1.97.1 test -p graphhelm-host-adoption --test restore --test recovery --locked
$env:CARGO_TARGET_DIR='E:/_agent-scratch/graphhelm/methodology-adoption/target'; cargo +1.97.1 test -p graphhelm-cli --test adoption_restore --locked
```

- [ ] **Step 5: Commit with `feat(adoption): restore reviewed baselines without losing later edits`.** Restore requires no provider call or running host/Runtime.

## Task 7: Prove activation, ship honest output, and verify the complete journey

**Files:** Create adapter `observation.rs`, `tests/observation.rs`, CLI `tests/adoption_journey.rs`, `docs/acceptance/adoption-rehearsal.md`; modify CLI renderer/dispatch, `docs/install/GETTING_STARTED.md` and relevant stale JPD/Extension implementation-status paragraphs only.

**Interfaces:** `pub fn verify_activation(plan: &Value, receipt: &Value) -> Result<Value, AdoptionError>` validates an ActivationReceipt and returns ApplyReceipt. Receipt spec requires transaction ID, accepted plan digest, host/version, observed config/package digests, fresh session ID, observer identity/custody, MCP tool observation, methodology loading evidence, environment identity and observation timestamps supplied by a trusted boundary. Define `pub fn adoption_verified(required: &[bool]) -> bool` as the pure all-required-facts predicate; never substitute a process exit code or an agent's statement for receipt validation.

- [ ] **Step 1: Write and run the missing-evidence control.**

```rust
#[test]
fn successful_file_installation_is_not_verified_adoption() {
    use graphhelm_policy::adoption::adoption_verified;
    assert!(!adoption_verified(&[]));
    assert!(!adoption_verified(&[true, true, false]));
    assert!(adoption_verified(&[true, true, true]));
}
```

Run `cargo +1.97.1 test -p graphhelm-policy successful_file_installation --locked` with target prefix, then implement:

```rust
pub fn adoption_verified(required: &[bool]) -> bool {
    !required.is_empty() && required.iter().all(|observed| *observed)
}
```

- [ ] **Step 2: Implement exact receipt bindings and human rendering from the same envelope.** Reject stale sessions, changed config/package bytes, wrong transaction, missing evidence and untrusted self-asserted observer input. File-state proof remains useful and is reported separately. `--verify` cannot elevate a user-authored JSON claim without trusted observation custody. Extend the terminal renderer through the existing two-face output mechanism; pipes still get JSON only. Terminal setup presents the actual keep/disable/replace/unresolved table and confirms the exact plan once. No automatic confirmation in a pipe.

- [ ] **Step 3: Add offline full-journey and sabotage tests.** The fixture starts with a factory instruction plus a personal preference and protected deny rule, two compatible skills and one conflicting skill. Preview identifies changes; apply backs up first; fake-host observation is explicitly fixture-scoped; repeat apply preserves original baseline; restoration returns originals while preserving an added user key. Alter the receipt digest, omit MCP proof and re-enable the old methodology in separate attacks; each must prevent verified status. These tests prove boundary behavior only, never real host activation.

- [ ] **Step 4: Write the separate host rehearsal recipe.** In a disposable project and disposable host profile, record host versions, close affected sessions, apply the reviewed plan, open a fresh session, independently capture loaded skills/effective instructions, execute one read-only GraphHelm MCP operation and verify its Runtime observation, and perform a reversible sample task under the new method. Reopen after restore and verify the old configuration. No managed-policy bypass, real-user-home migration or paid model run without explicit scope/budget authorization. Record `observer_missing` when the host provides no adequate observation. Keep this recipe out of the offline gate.

- [ ] **Step 5: Run focused suites, fmt, then the full local gate from the final code state.**

```powershell
$env:CARGO_TARGET_DIR='E:/_agent-scratch/graphhelm/methodology-adoption/target'; cargo +1.97.1 test -p graphhelm-cli --test adoption_inventory --test adoption_backup --test adoption_apply --test adoption_hosts --test adoption_restore --test adoption_journey --test init_cli --locked
$env:CARGO_TARGET_DIR='E:/_agent-scratch/graphhelm/methodology-adoption/target'; cargo +1.97.1 fmt --all -- --check
git diff --check
$env:CARGO_TARGET_DIR='E:/_agent-scratch/graphhelm/methodology-adoption/target'; & ./ci/gate.ps1 > D:/_agent-scratch/graphhelm/methodology-adoption/gate.log 2>&1
$gateExit = $LASTEXITCODE
Get-Content D:/_agent-scratch/graphhelm/methodology-adoption/gate.log -Tail 60
if ($gateExit -ne 0) { throw "Local gate failed with exit $gateExit" }
```

Create the owned log directory before running. Do not pipe the live gate. Do not report a `-SkipPostgres` run as a full gate. Preserve every failed run and evidence delta.

- [ ] **Step 6: Commit with `feat(adoption): verify reversible host adoption journeys`, obtain required exact-head peer reviews and merge proof.** Use the main-owned verifier recipe in AGENTS.md, `ci/closing-keywords.ps1 -Number $pr -Closes $issue`, and the repository's two-reviewer/third-lane merge protocol. Do not simplify it to one plan reviewer. Merge only with valid evidence; verify merged CLI journeys afterward. A published PR or a fake-host test does not establish delivery or real-host compatibility.

## Self-review and handoff checklist

- [ ] Map every spec section 4 requirement to Tasks 1-7; inspect the original/manual checkpoint distinction, supported host limitations and global conflicts explicitly.
- [ ] Check all new names against the public contracts above; keep emitted kind/state/reason spellings identical in schema, Rust and test assertions.
- [ ] Verify every Review Focus line has a negative case and a positive control; review crash recovery on Windows and Linux separately.
- [ ] Confirm all test examples and implementation interfaces are defined, all paths are scoped, and no host mutations are executed while writing this plan.
- [ ] Ask the owner to review the saved plans and choose the execution method before implementation, as required by writing-plans.

Recommended execution: subagent-driven for this slice because a mistaken filesystem, backup or restore change can lose private user configuration. Preserve the repository's stronger independent review and final-head gate requirements regardless of execution method. Other plans can use a cheaper native execution path once their contracts are reviewed.
