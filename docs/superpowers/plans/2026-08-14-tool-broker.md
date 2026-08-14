# Milestone 05c — Tool Broker and Tier 0/1 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The Tool Broker slice: repository, shell and tests tools as brokered local processes — every call passing schema validation, a capability lease, path and argv discipline, and tier routing — with Tier 0 (read-only, no workspace) for cognitive access and Tier 1 (ephemeral git-worktree workspace, scrubbed environment, resource caps, cleanup) for execution, and credentials structurally outside the workspace, proven by test.

**Architecture:** Two new workspace crates mirroring 05b's split. `core/tool-broker` is pure (no I/O, no clock, no randomness, no process): the typed tool-call vocabulary, the effect taxonomy, relative-path and argv rules, the capability lease, and `authorize` — the pure decision pipeline that maps a call to a tier or a typed refusal. `adapters/tool-host` is impure: Tier 1 workspace provisioning over `git worktree`, process execution with `env_clear` plus a fixed allowlist (host `HOME`/`USERPROFILE` never enter; they are redirected into the workspace), deadline kill, output caps, symlink-escape-safe path resolution, and the three builtin tools. The CLI gains `tool invoke` and a `tool_cli` gate stage. **No new third-party dependency**: process spawning is `std::process`, digests are the workspace's `sha2`/`hex`.

**Tech Stack:** Rust 1.97.1 (pinned), serde/serde_json, sha2, hex, thiserror (all already workspace-pinned), `std::process`. Test-side: tempfile, assert_cmd (workspace-pinned).

**Design sources:** `docs/superpowers/specs/2026-08-13-runtime-design.md` (§5 boundary, §6.3 credentials, §7 bullet 05c, §8 acceptance, §9 risks); `docs/agents/AGENTS_SKILLS_PLUGINS.md` §11 (tool categories, the ten-step broker pipeline, effects); `docs/security/SECURITY_ISOLATION_THREAT_MODEL.md` §6 (Tier 0/1 controls), §8 (leases), §11 (the twelve broker checks, argv not shell), §13 (repository threats); `docs/DECISION_REGISTER.md` hard constraints ("Model credentials cannot remain accessible in the same sandbox that runs untrusted code") and D-013; `docs/graph-engineer/GRAPH_ENGINEER_GUIDE.md` §5.2 (tool rules). Issue: #25.

---

## Binding process rules (every task, no exceptions)

1. **The code wins over the plan.** If a signature, type, path or failure-code number in this plan disagrees with the repository as merged, the repository is right: adapt, and report the discrepancy in your task summary. This plan was written against main at `0e1f095` *plus* the expectation that Milestone 05b (#24) has merged — GHCLI009–011 taken by the gateway, `gateway_cli` in the gate's suite list, `adapters/model-gateway` present. **Do not start implementation until 05b is on main.** If 05b landed differently than its plan (different code numbers, different suite list), renumber here accordingly and report.
2. **This plan adds exactly ONE event kind — `ReuseDecision`, in Task 9b, through the full D-037 ritual — and touches no other schema surface.** (Amended after the context-economy elevation debate, owner-approved on issue #33; the original rule said zero.) The ritual is not optional ceremony: envelope schema corrected in place, `schemas/releases/1.0.0/` mirrored byte-for-byte, both catalog digests recomputed, `checked_in_1_0_0_release_is_complete_and_raw_byte_identical` passing unmodified — exactly as 04b/04d/04e did. Outside Task 9b, nothing under `core/schema/`, `schemas/`, the catalog, the baseline, or `core/protocols` wire enums may change (reading them is fine). The authoring `node.schema.json` already carries `isolation.minimum` and `core/graph::is_valid_isolation_tier` (`core/graph/src/persistence.rs:1145`) closes the tier vocabulary — 05c consumes both, changes neither. If any other task believes it needs a schema or event change, STOP and report NEEDS_CONTEXT.
3. **Workspace clippy is the bar:** `cargo +1.97.1 clippy --workspace --all-targets --all-features --locked -- -D warnings` clean at every commit, not just the touched crate.
4. **Every new guard must be observed failing once.** Sabotage it or feed it the bad input, watch the named test fail the way you claimed, restore. Back up with `cp`, restore from the backup, never `git checkout --`.
5. **New CLI test binaries must be added to `ci/gate.ps1` by name.** The CLI stage lists suites explicitly (`ci/gate.ps1:106`; after 05b: `('cli_smoke','schema_cli','event_store_cli','execution_cli','api_http','gateway_cli')`). This plan adds `tool_cli`.
6. **No secret may reach a child process, a workspace file, an error message, or CLI output.** Tests plant sentinel values in the parent environment (`GRAPHHELM_EVENTS_KEY`, `GRAPHHELM_GATEWAY_KEY`, and a fake) and assert captured child environments, workspace file scans, and formatted errors never contain them. This is the register's hard constraint made executable; treat every one of these assertions as load-bearing.
7. **Argv, never shell strings.** No tool call ever passes through `cmd /C`, `sh -c`, or any interpreter the caller names implicitly. `std::process::Command::new(program).args(...)` only. (Tests themselves may spawn what they like — the discipline binds the broker and host, not the harness.)
8. **Run `cargo +1.97.1 fmt --all` before every commit.** Commit after every task: `type(scope): description`, body explains the why, trailer `Co-Authored-By: Claude Code <noreply@anthropic.com>`.

## Coordination note (Agent A / Milestone 05b in flight)

While this plan is under review, its branch touches exactly one file: this document. The workspace `Cargo.toml`, `ci/gate.ps1`, `docs/milestones/runtime.md` and `CHANGELOG.md` are 05b conflict surfaces and are modified only during implementation, which begins after #24 merges and the owner authorizes.

## What already exists (do not rebuild)

- `graphhelm_execution::NodeExecutor` — the seam this slice's output ultimately serves: `fn execute(&self, node_id: &str, attempt: u32) -> Result<NodeOutcome, ExecutionError>` (`core/execution/src/transition.rs:29`). **05c does not implement it** — the real executor is 05d; 05c builds the broker 05d will call, testable standalone through the CLI.
- `NodeOutcome` (closed, `core/protocols/src/simulation.rs:35`) — this plan maps tool results into a `ToolDisposition` of its own; translation into `NodeOutcome` is 05d's, stated in Task 4.
- The authoring isolation vocabulary: `node.schema.json` (`isolation: {minimum: tier_0|tier_1|tier_2|tier_3}`), `is_valid_isolation_tier` (`core/graph/src/persistence.rs:1145`), and the example graph `examples/graphs/software-feature.yaml` (the `implement` node carries `isolation.minimum: tier_1`, the `tests` node is `type: tool`).
- The CLI's redaction-safe `Failure { code, message, pointer }` pattern and four-key envelope (`apps/cli/src/commands/execution/mod.rs:50`); failure codes GHCLI001–008 taken on main, GHCLI009–011 taken by 05b. 05c starts at `GHCLI012`.
- The purity-test pattern: `core/execution/tests/source_invariants.rs` (`production_dependencies` slice of the manifest, `code_only` comment stripping, exact dependency pin). Copy and adapt, as `core/gateway` did.
- Test spawn pattern: `Command::new(assert_cmd::cargo::cargo_bin!("graphhelm"))` (`apps/cli/tests/execution_cli.rs:11`); fixture binaries via `CARGO_BIN_EXE_<name>` (05b's `fake_runtime` precedent).
- 05b's `adapters/model-gateway/src/runtime.rs` env-allowlist precedent — the gateway's allowlist includes `HOME`/`APPDATA` because official host CLIs own their own auth. **Tier 1's allowlist is deliberately stricter** (no host `HOME`, no `APPDATA`): a tool workspace has no auth of its own to keep. That asymmetry is the design, not an inconsistency.

## The broker pipeline, mapped honestly

`AGENTS_SKILLS_PLUGINS.md` §11.2 names ten steps. What each one is in this slice:

| §11.2 step | 05c | Where |
|---|---|---|
| 1. schema validation | serde with `deny_unknown_fields` into the closed `ToolCall` vocabulary | Task 2 |
| 2. identity check | required, validated `actor`; lease must name the same actor | Task 3 |
| 3. capability lease | `ToolLease { actor, capabilities, programs }`, deny by default | Task 3 |
| 4. policy | **deferred, stated** — no Policy Engine exists; the broker applies its fixed structural rules (effect→tier, program allowlist) and nothing dynamic | — |
| 5. path/network/secret validation | pure `RelativePath` rules + host canonicalization and containment; network structurally absent (no tool declares egress); secrets structurally absent (env scrub) | Tasks 2, 6, 8 |
| 6. read cache | snapshot-keyed, provably-exact-only (`AGENTS_SKILLS_PLUGINS.md` §11.2-3 as amended by #33); `drifting` uncached; erasure-invalidated | Task 9b |
| 7. sandbox routing | `authorize` returns the tier; host routes Tier 0 in-process read / Tier 1 workspace | Tasks 3, 7 |
| 8. execution | `run_in_workspace` — argv, scrubbed env, deadline, caps | Task 5 |
| 9. redaction | output byte caps + structurally empty child env; a content-aware secret *scanner* is **deferred, stated** | Task 5 |
| 10. artifact persistence | the `ToolCallRecord` carries digests; bytes stay operator-side (`--capture-out`); Evidence externalization is **05d's** | Tasks 4, 9 |
| 11. event emission | the `ReuseDecision` **kind** enters the closed set here (Task 9b, D-037 ritual); its **producer** is 05d's executor — the broker CLI appends nothing | Task 9b |

## File map

| Path | Responsibility |
|---|---|
| `core/tool-broker/Cargo.toml`, `src/lib.rs` | New pure crate `graphhelm-tool-broker` |
| `core/tool-broker/src/effect.rs` | `ToolEffect`, `IsolationTier`, `required_tier` |
| `core/tool-broker/src/path.rs` | `RelativePath` — the pure path rules |
| `core/tool-broker/src/call.rs` | `ToolCall` / per-tool action enums / per-action effects |
| `core/tool-broker/src/lease.rs` | `ToolLease`, `Capability`, `authorize`, `BrokerRefusal` |
| `core/tool-broker/src/record.rs` | `ToolCallRecord`, `ToolDisposition`, digest helper |
| `core/tool-broker/tests/effect_contract.rs` | Effect→tier rule and tier-name alignment tests |
| `core/tool-broker/tests/path_rules.rs` | Path-rule properties |
| `core/tool-broker/tests/authorize_contract.rs` | Lease/pipeline decision tests |
| `core/tool-broker/tests/record_contract.rs` | Record shape and digest tests |
| `core/tool-broker/tests/source_invariants.rs` | Purity: pinned dependency table, no I/O/clock/process tokens |
| `adapters/tool-host/Cargo.toml`, `src/lib.rs` | New impure crate `graphhelm-tool-host` |
| `adapters/tool-host/src/process.rs` | `run_in_workspace` — argv spawn, env allowlist, deadline, caps |
| `adapters/tool-host/src/workspace.rs` | `Tier1Workspace` — git-worktree provision/resolve/remove |
| `adapters/tool-host/src/tools.rs` | The three builtin tools over the two primitives |
| `adapters/tool-host/src/host.rs` | `ToolHost::invoke` — authorize → cache → route → execute → record |
| `adapters/tool-host/src/cache.rs` | Snapshot-keyed read cache, erasure-invalidated (Task 9b) |
| `adapters/tool-host/src/bin/fake_tool.rs` | Test-fixture binary (env-dump, write-file, sleep, big-output, echo) |
| `adapters/tool-host/tests/process_isolation.rs` | Env scrub, deadline, caps |
| `adapters/tool-host/tests/workspace_containment.rs` | Worktree lifecycle, path escape, junction escape |
| `adapters/tool-host/tests/broker_end_to_end.rs` | Tier 0/1 routing, the three tools, the hard-constraint proof |
| `apps/cli/src/commands/tool/mod.rs` (+ `invoke.rs`) | `graphhelm tool invoke` |
| `apps/cli/tests/tool_cli.rs` | CLI suite (new gate stage) |
| `Cargo.toml` (workspace), `ci/gate.ps1` | Members + `tool_cli` stage *(implementation phase only — see coordination note)* |
| `docs/milestones/runtime.md`, `CHANGELOG.md` | 05c record *(implementation phase only)* |

**Deferred, stated (do not build):** caching for `drifting` tools, divergence sampling and refresh floors (queued with triggers on issue #35 — v1 caches only the provably exact); the Policy Engine step (§11.2 step 4) and dynamic policies; a content-aware output secret scanner (§11.2 step 8 beyond caps and structural env emptiness); kernel-enforced network deny (no tool in this slice declares network egress, so nothing needs it; enforcement arrives with containers); ephemeral containers, restricted users, seccomp — Tier 1 here is the threat model's "Git worktree or snapshot" control plus process-level scrubbing, and the milestone doc must say exactly that; Tiers 2/3 and dynamic tier elevation (D-013's escalation needs the Governor loop); lease expiry, `max_uses`, revocation-on-pause (need the runtime clock/lifecycle — 05d); tool manifests as installable artifacts and every plugin runtime model (§12 — builtin tools only); MCP/container/HTTP tool transports; `repository.push`/`pull` and anything with `NetworkEgress` or `ProductionEffect`; compensation/idempotency declarations (§5.2's remaining bullets); rate limiting beyond output-byte caps; translation of `ToolCallRecord` into `NodeOutcome`/Evidence (05d).

---

### Task 1: `core/tool-broker` skeleton — effects, tiers, and the tier rule

**Files:**
- Create: `core/tool-broker/Cargo.toml`, `core/tool-broker/src/lib.rs`, `core/tool-broker/src/effect.rs`
- Create: `core/tool-broker/tests/source_invariants.rs`
- Modify: `Cargo.toml` (workspace members — add `"core/tool-broker"` after `"core/gateway"`)

- [ ] **Step 1: Crate skeleton.** `core/tool-broker/Cargo.toml` mirrors `core/gateway/Cargo.toml`'s shape (workspace package/lints inheritance). `[dependencies]`: `serde` (workspace, derive), `serde_json` (workspace), `sha2` (workspace), `hex` (workspace), `thiserror` (workspace). `[dev-dependencies]`: `graphhelm-graph` (path — Task 1 Step 2's alignment test only), `proptest` (workspace). Nothing else.

- [ ] **Step 2: Write the failing tests** in a new `core/tool-broker/tests/effect_contract.rs`:

```rust
use graphhelm_tool_broker::effect::{IsolationTier, ToolEffect, required_tier};

#[test]
fn read_only_is_tier_0_and_reversible_write_is_tier_1() {
    assert_eq!(required_tier(ToolEffect::ReadOnly), Ok(IsolationTier::Tier0));
    assert_eq!(required_tier(ToolEffect::ReversibleWrite), Ok(IsolationTier::Tier1));
}

#[test]
fn every_effect_beyond_this_slice_is_a_typed_refusal_not_a_guess() {
    use graphhelm_tool_broker::effect::EffectUnsupported;
    for effect in [
        ToolEffect::IrreversibleWrite,
        ToolEffect::ExternalSideEffect,
        ToolEffect::ProductionEffect,
        ToolEffect::SecretUse,
        ToolEffect::NetworkEgress,
    ] {
        assert_eq!(required_tier(effect), Err(EffectUnsupported { effect }));
    }
}

#[test]
fn tier_wire_names_agree_with_the_authoring_vocabulary() {
    // The authoring contract closed the tier vocabulary in core/graph; this crate must never
    // drift from it. Alignment by test, not by dependency: graphhelm-graph is dev-only.
    for tier in [
        IsolationTier::Tier0,
        IsolationTier::Tier1,
        IsolationTier::Tier2,
        IsolationTier::Tier3,
    ] {
        let name = serde_json::to_value(tier).unwrap();
        let name = name.as_str().unwrap();
        assert!(graphhelm_graph::is_valid_isolation_tier(name), "{name} drifted");
    }
}
```

Check the export path of `is_valid_isolation_tier` in `core/graph/src/lib.rs:14` before writing the import — the code wins.

- [ ] **Step 3: Run to see them fail** (crate doesn't exist): `cargo +1.97.1 test -p graphhelm-tool-broker --locked` → compile error.

- [ ] **Step 4: Implement `effect.rs`.**

```rust
/// The declared effect classes of AGENTS_SKILLS_PLUGINS.md §11.3, complete so that adding a
/// tool later forces a deliberate classification rather than a default.
#[derive(Clone, Copy, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolEffect {
    ReadOnly,
    ReversibleWrite,
    IrreversibleWrite,
    ExternalSideEffect,
    ProductionEffect,
    SecretUse,
    NetworkEgress,
}

/// Serialized names match `core/graph::is_valid_isolation_tier`'s closed vocabulary exactly.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, serde::Serialize, serde::Deserialize)]
pub enum IsolationTier {
    #[serde(rename = "tier_0")] Tier0,
    #[serde(rename = "tier_1")] Tier1,
    #[serde(rename = "tier_2")] Tier2,
    #[serde(rename = "tier_3")] Tier3,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, thiserror::Error)]
#[error("the {effect:?} effect is not supported by the 05c broker")]
pub struct EffectUnsupported { pub effect: ToolEffect }

/// One exhaustive match, no wildcard arm: a new effect variant breaks compilation here on
/// purpose, so its tier is decided, never defaulted. SecretUse is refused structurally — the
/// register's hard constraint is that the workspace never sees a credential, so no tool in this
/// broker may even declare wanting one.
pub const fn required_tier(effect: ToolEffect) -> Result<IsolationTier, EffectUnsupported> {
    match effect {
        ToolEffect::ReadOnly => Ok(IsolationTier::Tier0),
        ToolEffect::ReversibleWrite => Ok(IsolationTier::Tier1),
        ToolEffect::IrreversibleWrite
        | ToolEffect::ExternalSideEffect
        | ToolEffect::ProductionEffect
        | ToolEffect::SecretUse
        | ToolEffect::NetworkEgress => Err(EffectUnsupported { effect }),
    }
}
```

- [ ] **Step 5: Purity invariants** in `core/tool-broker/tests/source_invariants.rs` — copy `core/execution/tests/source_invariants.rs`'s pattern (the `production_dependencies` manifest slice, `code_only` comment stripping, `include_str!` per source file, the exact-dependency pin). Pinned `[dependencies]` set: exactly `["serde", "serde_json", "sha2", "hex", "thiserror"]`. Token scan over `src/`: forbid `std::fs`, `std::net`, `std::process`, `std::time`, `std::env`, `SystemTime`, `Instant`, `rand`, `getrandom`, `tokio`. (`sha2` and `hex` are pure functions over bytes; they are the point of allowing them.)

- [ ] **Step 6: Run to green, fmt, workspace clippy.**

- [ ] **Step 7: Sabotage (rule 4):** make `required_tier` return `Ok(IsolationTier::Tier0)` for `ReversibleWrite`; watch `read_only_is_tier_0_and_reversible_write_is_tier_1` fail; restore from backup; re-run green.

- [ ] **Step 8: Commit** `feat(tool-broker): effect taxonomy and tier rule in a new pure crate`.

---

### Task 2: the pure path and argv rules

**Files:**
- Create: `core/tool-broker/src/path.rs`
- Create: `core/tool-broker/tests/path_rules.rs`

- [ ] **Step 1: Failing tests** in `core/tool-broker/tests/path_rules.rs`:

```rust
use graphhelm_tool_broker::path::{PathRuleError, RelativePath};

#[test]
fn plain_relative_paths_parse_and_normalize_to_forward_slashes() {
    let path = RelativePath::parse("src/lib.rs").unwrap();
    assert_eq!(path.as_str(), "src/lib.rs");
    // Backslashes are refused, not silently converted: one canonical spelling only, so two
    // spellings of one file can never pass two different checks.
    assert!(matches!(
        RelativePath::parse("src\\lib.rs").unwrap_err(),
        PathRuleError::BackslashSeparator
    ));
}

#[test]
fn escape_shapes_are_refused_by_form_alone() {
    for bad in [
        "../outside.txt",          // parent traversal
        "src/../../outside.txt",   // embedded traversal
        "/etc/passwd",             // absolute
        "C:/Windows/system32",     // drive-absolute
        "C:relative",              // drive-relative
        "//server/share/x",        // UNC
        "",                        // empty
        ".",                       // no-op self
        "src/./lib.rs",            // dot component — one spelling only
    ] {
        assert!(RelativePath::parse(bad).is_err(), "{bad:?} must be refused");
    }
}

#[test]
fn nul_control_bytes_and_oversize_are_refused() {
    assert!(RelativePath::parse("a\0b").is_err());
    assert!(RelativePath::parse("a\nb").is_err());
    let long = "a/".repeat(3000);
    assert!(matches!(
        RelativePath::parse(&long).unwrap_err(),
        PathRuleError::TooLong { .. }
    ));
}

proptest::proptest! {
    // Review finding 4 (the declared proptest dev-dependency now earns its place): whatever
    // string parses, the accepted form contains no traversal, no backslash, no control byte,
    // and round-trips through as_str unchanged — the "one spelling only" rule as a property
    // rather than a case list.
    #[test]
    fn an_accepted_path_is_always_in_canonical_form(candidate in ".{0,128}") {
        if let Ok(path) = RelativePath::parse(&candidate) {
            let text = path.as_str();
            proptest::prop_assert_eq!(text, candidate.as_str());
            proptest::prop_assert!(!text.contains('\\'));
            proptest::prop_assert!(!text.starts_with('/'));
            proptest::prop_assert!(!text.split('/').any(|c| c.is_empty() || c == "." || c == ".."));
            proptest::prop_assert!(!text.bytes().any(|b| b < 0x20));
        }
    }
}

#[test]
fn program_names_are_bare_names_never_paths() {
    use graphhelm_tool_broker::path::validate_program_name;
    assert!(validate_program_name("git").is_ok());
    assert!(validate_program_name("cargo").is_ok());
    // A path smuggles the workspace's own content (or anything on disk) into the "program"
    // position, bypassing the lease's program allowlist by construction. Names only; the OS
    // PATH does the resolution in the host.
    for bad in ["./git", "bin/git", "C:/tools/git.exe", "git.exe/", "", "gi t"] {
        assert!(validate_program_name(bad).is_err(), "{bad:?} must be refused");
    }
}
```

- [ ] **Step 2: Run to see them fail** (module missing) — quote the error.

- [ ] **Step 3: Implement `path.rs`.**

```rust
pub const MAX_PATH_BYTES: usize = 4096;

/// A workspace-relative path in exactly one spelling: forward slashes, no empty/dot/dotdot
/// components, no absolute or drive or UNC form, no control bytes. Purely lexical — symlink
/// resolution needs the filesystem and lives in the host (`workspace.rs`), which re-checks
/// containment after canonicalization. Both layers exist on purpose: form here, truth there.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Debug, serde::Serialize)]
#[serde(transparent)]
pub struct RelativePath(String);

#[derive(Clone, Copy, PartialEq, Eq, Debug, thiserror::Error)]
pub enum PathRuleError {
    #[error("the path is empty")] Empty,
    #[error("the path exceeds {MAX_PATH_BYTES} bytes")] TooLong { bytes: usize },
    #[error("backslash separators are refused; use forward slashes")] BackslashSeparator,
    #[error("absolute, drive or UNC paths are refused")] NotRelative,
    #[error("`.` and `..` components are refused")] DotComponent,
    #[error("control bytes are refused")] ControlByte,
}

impl RelativePath {
    pub fn parse(candidate: &str) -> Result<Self, PathRuleError> { /* rules above, in order:
        empty → length → control bytes (any c < 0x20 or NUL) → backslash → drive (second byte
        b':') / leading '/' / leading "//" → split on '/', refuse empty, ".", ".." components */ }
    pub fn as_str(&self) -> &str { &self.0 }
}

// Deserialize goes through parse so a record read back cannot smuggle an unvalidated path.
impl<'de> serde::Deserialize<'de> for RelativePath { /* String → parse → map_err */ }

/// A program is a bare name resolved by the host OS's PATH: 1..=64 bytes of [a-z0-9_-],
/// nothing else. No slashes (a path would bypass the lease's allowlist by naming anything on
/// disk), no dots (Windows resolution appends `.exe`/`.cmd` itself via PATHEXT; accepting an
/// explicit extension would give one program two allowlist spellings).
pub fn validate_program_name(candidate: &str) -> Result<(), PathRuleError> { /* charset
    [a-z0-9_-], 1..=64, else a refusal — reuse PathRuleError or a dedicated ProgramRuleError
    if clearer; the tests above are the contract */ }
```

- [ ] **Step 4: Run to green, fmt, workspace clippy.**

- [ ] **Step 5: Sabotage:** delete the `..` component check; `escape_shapes_are_refused_by_form_alone` fails; restore; re-run green.

- [ ] **Step 6: Commit** `feat(tool-broker): lexical path and program rules`.

---

### Task 3: the call vocabulary, the lease, and `authorize`

**Files:**
- Create: `core/tool-broker/src/call.rs`, `core/tool-broker/src/lease.rs`
- Create: `core/tool-broker/tests/authorize_contract.rs`

- [ ] **Step 1: Failing tests** in `core/tool-broker/tests/authorize_contract.rs`:

```rust
use graphhelm_tool_broker::call::{RepositoryAction, ShellAction, TestsAction, ToolCall};
use graphhelm_tool_broker::effect::IsolationTier;
use graphhelm_tool_broker::lease::{
    BrokerRefusal, Capability, ToolLease, authorize,
};
use graphhelm_tool_broker::path::RelativePath;

fn full_lease(actor: &str) -> ToolLease {
    ToolLease {
        actor: actor.to_owned(),
        capabilities: [
            Capability::RepositoryRead,
            Capability::RepositoryWrite,
            Capability::ShellExecute,
            Capability::TestsExecute,
        ]
        .into_iter()
        .collect(),
        programs: ["git", "cargo"].map(str::to_owned).into_iter().collect(),
    }
}

fn read_call() -> ToolCall {
    ToolCall::Repository(RepositoryAction::ReadFile {
        path: RelativePath::parse("src/lib.rs").unwrap(),
    })
}

fn shell_call(program: &str) -> ToolCall {
    ToolCall::Shell(ShellAction {
        program: program.to_owned(),
        arguments: vec!["status".to_owned()],
    })
}

#[test]
fn a_read_call_routes_to_tier_0_and_a_write_call_to_tier_1() {
    let lease = full_lease("agent-builder");
    let read = authorize(&read_call(), &lease, "agent-builder").unwrap();
    assert_eq!(read.tier, IsolationTier::Tier0);

    let write = authorize(
        &ToolCall::Repository(RepositoryAction::ApplyPatch { patch: "diff".into() }),
        &lease,
        "agent-builder",
    )
    .unwrap();
    assert_eq!(write.tier, IsolationTier::Tier1);
}

#[test]
fn the_actor_must_match_the_lease() {
    let lease = full_lease("agent-builder");
    assert!(matches!(
        authorize(&read_call(), &lease, "agent-impostor").unwrap_err(),
        BrokerRefusal::ActorMismatch { .. }
    ));
}

#[test]
fn a_malformed_actor_is_refused_before_any_other_check() {
    // Review finding 5: ActorInvalid existed with no test. Charset is [a-z][a-z0-9-]{0,63};
    // the empty string, uppercase, and separators are all outside it. The lease NAMES the
    // same bad actor, so the only refusal that can fire is the validity check itself —
    // pinning that it runs before the ActorMismatch comparison.
    for bad in ["", "Agent-Builder", "agent_builder", "agent builder", "1agent"] {
        let lease = full_lease(bad);
        assert!(
            matches!(
                authorize(&read_call(), &lease, bad).unwrap_err(),
                BrokerRefusal::ActorInvalid
            ),
            "{bad:?} must be ActorInvalid"
        );
    }
}

#[test]
fn a_missing_capability_is_denied_by_default() {
    let mut lease = full_lease("agent-builder");
    lease.capabilities.remove(&Capability::ShellExecute);
    assert!(matches!(
        authorize(&shell_call("git"), &lease, "agent-builder").unwrap_err(),
        BrokerRefusal::CapabilityMissing { .. }
    ));
}

#[test]
fn a_program_outside_the_lease_allowlist_is_denied() {
    let lease = full_lease("agent-builder");
    assert!(matches!(
        authorize(&shell_call("curl"), &lease, "agent-builder").unwrap_err(),
        BrokerRefusal::ProgramDenied { .. }
    ));
}

#[test]
fn shell_and_tests_are_always_tier_1_even_for_an_innocent_looking_program() {
    // A process can write; the broker cannot know less. Deny-by-default means classifying
    // every spawned program as ReversibleWrite, so no shell call ever lands in Tier 0.
    let lease = full_lease("agent-builder");
    let shell = authorize(&shell_call("git"), &lease, "agent-builder").unwrap();
    assert_eq!(shell.tier, IsolationTier::Tier1);
    let tests = authorize(
        &ToolCall::Tests(TestsAction { arguments: vec!["--lib".into()] }),
        &lease,
        "agent-builder",
    )
    .unwrap();
    assert_eq!(tests.tier, IsolationTier::Tier1);
}

#[test]
fn refusals_never_echo_call_arguments() {
    // A refusal names the rule and the tool, never content: a denied patch or argument list
    // must not travel into logs through the error path.
    let lease = full_lease("agent-builder");
    let sentinel = "SENTINEL-argument-value";
    let refusal = authorize(
        &ToolCall::Shell(ShellAction {
            program: "curl".to_owned(),
            arguments: vec![sentinel.to_owned()],
        }),
        &lease,
        "agent-builder",
    )
    .unwrap_err();
    let rendered = format!("{refusal} {refusal:?}");
    assert!(!rendered.contains(sentinel));
}

#[test]
fn an_unknown_field_in_a_serialized_call_is_refused() {
    let json = r#"{"tool":"repository","action":"read_file","path":"src/lib.rs","extra":true}"#;
    assert!(serde_json::from_str::<ToolCall>(json).is_err());
}
```

- [ ] **Step 2: Run to see them fail** — quote the compile error.

- [ ] **Step 3: Implement `call.rs`.** Serialized form is internally tagged twice (`tool`, then `action`), flat for operator ergonomics:

```rust
/// The closed builtin vocabulary of this slice: repository, shell, tests — the three the
/// runtime design's §7 bullet names. Every variant's effect is declared here, in one place.
#[derive(Clone, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
#[serde(tag = "tool", rename_all = "snake_case")]
pub enum ToolCall {
    Repository(RepositoryAction),
    Shell(ShellAction),
    Tests(TestsAction),
}

#[derive(Clone, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum RepositoryAction {
    /// Read one file's bytes. Tier 0: no workspace, no mutation possible by construction —
    /// the host's read path opens the file and does nothing else.
    ReadFile { path: crate::path::RelativePath },
    /// List files under an optional prefix (git ls-files semantics in the host).
    ListFiles {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        prefix: Option<crate::path::RelativePath>,
    },
    /// Worktree-vs-HEAD diff.
    Diff,
    /// Apply a unified diff inside the Tier 1 workspace.
    ApplyPatch { patch: String },
    /// Commit staged-and-unstaged workspace changes with a message.
    Commit { message: String },
}

#[derive(Clone, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ShellAction {
    /// A bare program name; `validate_program_name` at parse boundaries, the lease at authorize.
    pub program: String,
    #[serde(default)]
    pub arguments: Vec<String>,
}

#[derive(Clone, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TestsAction {
    /// Arguments to the host-configured runner. The runner program itself is host
    /// configuration, never caller input — a caller cannot rename its way around the lease.
    #[serde(default)]
    pub arguments: Vec<String>,
}

impl ToolCall {
    pub fn effect(&self) -> crate::effect::ToolEffect { /* ReadFile/ListFiles/Diff → ReadOnly;
        ApplyPatch/Commit → ReversibleWrite (the workspace is ephemeral; discarding it reverses
        the write); Shell(_) → ReversibleWrite; Tests(_) → ReversibleWrite */ }
    pub fn capability(&self) -> crate::lease::Capability { /* read actions → RepositoryRead;
        write actions → RepositoryWrite; Shell → ShellExecute; Tests → TestsExecute */ }
}
```

Add `deny_unknown_fields` on the tagged enums exactly as serde permits for internally-tagged representations — **verify with the unknown-field test**; if serde's internal tagging cannot enforce it on the enum itself, wrap deserialization in a checked helper (`ToolCall::from_json`) that round-trips through `serde_json::Value` and rejects unknown keys, and make the test call that. Report which shape the code took.

- [ ] **Step 4: Implement `lease.rs`.**

```rust
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Capability { RepositoryRead, RepositoryWrite, ShellExecute, TestsExecute }

/// What the caller was granted, by whom is 05d's concern (the node contract); here the lease
/// is an input. No implicit inheritance, deny by default (threat model §8.2).
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ToolLease {
    pub actor: String,
    pub capabilities: std::collections::BTreeSet<Capability>,
    /// Bare program names the Shell capability may spawn. Tests' runner is host config and
    /// deliberately not listed here.
    pub programs: std::collections::BTreeSet<String>,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct BrokerPlan {
    pub capability: Capability,
    pub effect: crate::effect::ToolEffect,
    pub tier: crate::effect::IsolationTier,
}

#[derive(Clone, PartialEq, Eq, Debug, thiserror::Error)]
pub enum BrokerRefusal {
    #[error("the caller is not the lease's actor")] ActorMismatch { lease_actor: String },
    #[error("the lease does not grant {capability:?}")] CapabilityMissing { capability: Capability },
    #[error("the program is not in the lease's allowlist")] ProgramDenied,
    #[error("the actor identifier is not valid")] ActorInvalid,
    #[error(transparent)] EffectUnsupported(#[from] crate::effect::EffectUnsupported),
}

/// The pure §11.2 pipeline: identity → capability → program allowlist → effect → tier.
/// Order is a contract (tests pin it indirectly through the refusal variants).
pub fn authorize(call: &ToolCall, lease: &ToolLease, actor: &str)
    -> Result<BrokerPlan, BrokerRefusal>
{ /* validate actor charset ([a-z][a-z0-9-]{0,63} — stricter than the wire ActorId pattern,
     deliberately); compare with lease.actor; capability lookup; if Shell, validate_program_name
     then lease.programs.contains; effect → required_tier */ }
```

- [ ] **Step 5: Run to green, fmt, workspace clippy.**

- [ ] **Step 6: Sabotage:** make `authorize` skip the capability lookup; `a_missing_capability_is_denied_by_default` fails; restore. Then plant the first shell argument into `ProgramDenied`'s `Display`; `refusals_never_echo_call_arguments` fails; restore.

- [ ] **Step 7: Commit** `feat(tool-broker): call vocabulary, capability lease and authorize pipeline`.

---

### Task 4: the tool-call record

**Files:**
- Create: `core/tool-broker/src/record.rs`
- Test: extend `core/tool-broker/tests/authorize_contract.rs` (a `record` section or sibling file `record_contract.rs`)

- [ ] **Step 1: Failing tests** (`core/tool-broker/tests/record_contract.rs`):

```rust
use graphhelm_tool_broker::record::{ToolCallRecord, ToolDisposition, digest_hex};

#[test]
fn the_digest_is_sha256_hex_of_the_bytes() {
    // The empty-input SHA-256 vector, pinned so the helper can never silently change algorithm.
    assert_eq!(
        digest_hex(b""),
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
}

#[test]
fn a_record_serializes_without_any_free_form_stream_content() {
    const STREAM: &[u8] = b"THE-STREAM-CONTENT-SENTINEL";
    let record = ToolCallRecord {
        tool: "shell".into(),
        action: "run".into(),
        actor: "agent-builder".into(),
        tier: graphhelm_tool_broker::effect::IsolationTier::Tier1,
        disposition: ToolDisposition::Completed { exit_code: 0 },
        stdout_sha256: digest_hex(STREAM),
        stdout_bytes: STREAM.len() as u64,
        stderr_sha256: digest_hex(b""),
        stderr_bytes: 0,
        truncated: false,
    };
    let json = serde_json::to_string(&record).unwrap();
    // The record is what 05d will externalize beside Evidence; the streams themselves must
    // never ride in it (D-036's discipline applied one layer early). A digest-only type has
    // nowhere to put the bytes — this test keeps it that way.
    assert!(!json.contains("THE-STREAM-CONTENT-SENTINEL"), "stream bytes leaked into the record");
    for key in ["stdoutSha256", "stderrSha256", "disposition", "tier"] {
        assert!(json.contains(key), "{key} missing");
    }
}

#[test]
fn dispositions_cover_refusal_timeout_and_host_error() {
    for disposition in [
        ToolDisposition::Completed { exit_code: 1 },
        ToolDisposition::Denied { rule: "capability_missing".into() },
        ToolDisposition::TimedOut,
        ToolDisposition::HostError { code: "GHTOOL001_WORKSPACE".into() },
    ] {
        let json = serde_json::to_value(&disposition).unwrap();
        assert!(json.get("kind").is_some(), "tagged form required: {json}");
    }
}
```

- [ ] **Step 2: Implement `record.rs`.** `ToolCallRecord` camelCase serde, fields as the test names them; `ToolDisposition` internally tagged `kind`, snake_case variants (`completed`, `denied`, `timed_out`, `host_error`). `digest_hex(bytes: &[u8]) -> String` over `sha2::Sha256` + `hex::encode`. Document on the type: **the record is the durable shape; the stream bytes are operator/Evidence material and never enter it. Translation into `NodeOutcome` and Evidence externalization are 05d's, deliberately absent here.**

- [ ] **Step 3: Run to green, fmt, workspace clippy.**

- [ ] **Step 4: Commit** `feat(tool-broker): tool call record with digest-only stream references`.

---

### Task 5: `adapters/tool-host` — the process primitive and the scrubbed environment

**Files:**
- Create: `adapters/tool-host/Cargo.toml`, `src/lib.rs`, `src/process.rs`, `src/bin/fake_tool.rs`
- Create: `adapters/tool-host/tests/process_isolation.rs`
- Modify: `Cargo.toml` (workspace members — add `"adapters/tool-host"` after `"adapters/model-gateway"`)

New crate `[dependencies]`: `graphhelm-tool-broker` (path), `serde`, `serde_json`, `sha2`, `hex`, `thiserror` (all workspace). `[dev-dependencies]`: `tempfile`. No async runtime: the host is synchronous like the command layer it serves (05d bridges with `spawn_blocking` when the async driver arrives — same posture ADR-025 recorded for `ureq`).

- [ ] **Step 1: The fixture binary.** `src/bin/fake_tool.rs`, doc-commented as a test fixture (built for integration tests via `CARGO_BIN_EXE_fake_tool`, 05b's `fake_runtime` precedent). Behavior via first argument:

```text
env-dump           → prints every environment variable as NAME=VALUE lines, exit 0
echo <words...>    → prints the remaining argv joined by spaces, exit 0
write-file <path>  → writes the bytes "written" to <path> resolved against the CURRENT
                     DIRECTORY, exit 0
cwd                → prints the current directory, exit 0
sleep              → sleeps 3600s (the host must kill it)
big-output         → writes 8 MiB of 'x' to stdout, exit 0
exit-code <n>      → exits with <n> parsed as i32
```

- [ ] **Step 2: Failing tests** in `adapters/tool-host/tests/process_isolation.rs`:

```rust
use std::collections::BTreeMap;
use std::path::Path;
use std::time::Duration;

use graphhelm_tool_host::process::{ProcessLimits, run_in_workspace};

fn fake_tool() -> String { env!("CARGO_BIN_EXE_fake_tool").to_owned() }

fn limits() -> ProcessLimits {
    ProcessLimits { timeout: Duration::from_secs(10), max_output_bytes: 1024 * 1024 }
}

fn run(root: &Path, args: &[&str], limits: &ProcessLimits)
    -> graphhelm_tool_host::process::CapturedProcess
{
    run_in_workspace(root, &fake_tool(), &args.iter().map(|a| a.to_string()).collect::<Vec<_>>(),
        &BTreeMap::new(), &[], limits).unwrap()
}

#[test]
fn the_child_environment_is_an_allowlist_and_never_carries_host_secrets() {
    // The register's hard constraint, observed from inside the child. The sentinels must sit
    // in the PARENT process environment (std::env::set_var is unsafe and cross-test-racy), so
    // this test is a two-piece wrapper — build exactly this shape:
    //
    //   OUTER (this #[test], runs when GH_TOOL_HOST_INNER is unset): re-executes
    //   std::env::current_exe() filtered to this test's own name, with
    //   GRAPHHELM_EVENTS_KEY/GRAPHHELM_GATEWAY_KEY/FAKE_SECRET set to sentinel values and
    //   GH_TOOL_HOST_INNER=1 in the child's environment; asserts the inner run passed.
    //
    //   INNER (same #[test] body, GH_TOOL_HOST_INNER=1): the parent env now genuinely
    //   carries the sentinels; run fake_tool env-dump through run_in_workspace and assert:
    let captured = run(tempdir_path, &["env-dump"], &limits());
    let dump = String::from_utf8_lossy(&captured.stdout);
    assert!(dump.lines().any(|l| l.starts_with("PATH=")), "PATH must survive");
    for forbidden in ["GRAPHHELM_EVENTS_KEY", "GRAPHHELM_GATEWAY_KEY", "FAKE_SECRET", "SENTINEL"] {
        assert!(!dump.contains(forbidden), "{forbidden} leaked into the Tier 1 child");
    }
    assert!(!dump.lines().any(|l| l.starts_with("APPDATA=")), "APPDATA is not allowlisted");
}

#[test]
fn home_and_temp_are_redirected_into_the_workspace() {
    let workspace = tempfile::tempdir().unwrap();
    let captured = run(workspace.path(), &["env-dump"], &limits());
    let dump = String::from_utf8_lossy(&captured.stdout);
    let expect = |name: &str| {
        let line = dump.lines().find(|l| l.starts_with(&format!("{name}="))).unwrap();
        assert!(
            Path::new(line.split_once('=').unwrap().1).starts_with(workspace.path()),
            "{name} must point inside the workspace, got {line}"
        );
    };
    for name in ["HOME", "USERPROFILE", "TEMP", "TMP"] { expect(name); }
    for fixed in [
        "GIT_CONFIG_NOSYSTEM=1",
        "GIT_TERMINAL_PROMPT=0",
        "GIT_OPTIONAL_LOCKS=0",
        // The synthetic commit identity (review finding 1): env_clear plus an empty redirected
        // HOME leaves git with no user.name/user.email anywhere, and `git commit` refuses with
        // "Please tell me who you are". A fixed identity in the child environment is the
        // config-free way to supply one, and it is deterministic across machines.
        "GIT_AUTHOR_NAME=GraphHelm Tool Broker",
        "GIT_AUTHOR_EMAIL=tools@graphhelm.invalid",
        "GIT_COMMITTER_NAME=GraphHelm Tool Broker",
        "GIT_COMMITTER_EMAIL=tools@graphhelm.invalid",
    ] {
        assert!(dump.contains(fixed), "{fixed} missing");
    }
}

#[test]
fn path_prepend_directories_lead_the_child_path() {
    // Host configuration (review finding 2): a test needs the fake_tool's directory — and an
    // operator may need a pinned toolchain directory — resolvable WITHOUT mutating the parent
    // process's PATH (racy across parallel tests) and without weakening the bare-name rule.
    // path_prepend is the host-side answer: directories joined ahead of the inherited PATH in
    // the CHILD only.
    let workspace = tempfile::tempdir().unwrap();
    let tool_dir = Path::new(&fake_tool()).parent().unwrap().to_path_buf();
    let captured = run_in_workspace(workspace.path(), &fake_tool(), &["env-dump".to_owned()],
        &BTreeMap::new(), &[tool_dir.clone()], &limits()).unwrap();
    let dump = String::from_utf8_lossy(&captured.stdout);
    let path_line = dump.lines().find(|l| l.starts_with("PATH=")).unwrap();
    assert!(
        path_line["PATH=".len()..].starts_with(&tool_dir.display().to_string()),
        "prepended directory must lead PATH, got {path_line}"
    );
}

#[test]
fn extra_env_is_validated_and_recorded_shape_only() {
    use graphhelm_tool_host::process::HostError;
    // Declared extras exist for a tests runner that needs CARGO_HOME/RUSTUP_HOME pointing at a
    // credential-free toolchain home. GRAPHHELM_* names are structurally refused so the host's
    // own passphrases can never be handed back in — and (review finding 6) so is EVERY name
    // the host itself defines: the INHERITED allowlist (PATH, PATHEXT, ...), the redirected
    // names (HOME, USERPROFILE, TEMP, TMP) and the fixed GIT_* set. An extra_env PATH would
    // otherwise swap program resolution out from under the lease's allowlist.
    let workspace = tempfile::tempdir().unwrap();
    for denied in ["GRAPHHELM_EVENTS_KEY", "PATH", "PATHEXT", "HOME", "GIT_CONFIG_NOSYSTEM"] {
        let mut extra = BTreeMap::new();
        extra.insert(denied.to_owned(), "x".to_owned());
        let refused = run_in_workspace(workspace.path(), &fake_tool(),
            &["env-dump".to_owned()], &extra, &[], &limits());
        assert!(
            matches!(refused.unwrap_err(), HostError::ExtraEnvDenied { .. }),
            "{denied} must be refused as an extra_env name"
        );
    }

    let mut ok = BTreeMap::new();
    ok.insert("CARGO_HOME".to_owned(), workspace.path().join("ch").display().to_string());
    let captured = run_in_workspace(workspace.path(), &fake_tool(),
        &["env-dump".to_owned()], &ok, &[], &limits()).unwrap();
    assert!(String::from_utf8_lossy(&captured.stdout).contains("CARGO_HOME="));
}

#[test]
fn a_hung_child_is_killed_at_the_deadline() {
    let workspace = tempfile::tempdir().unwrap();
    let limits = ProcessLimits { timeout: Duration::from_secs(2), max_output_bytes: 1024 };
    let started = std::time::Instant::now();
    let captured = run(workspace.path(), &["sleep"], &limits);
    assert!(captured.timed_out);
    assert!(started.elapsed() < Duration::from_secs(30));
}

#[test]
fn oversize_output_is_capped_and_marked_truncated_without_deadlock() {
    let workspace = tempfile::tempdir().unwrap();
    let limits = ProcessLimits { timeout: Duration::from_secs(30), max_output_bytes: 64 * 1024 };
    let captured = run(workspace.path(), &["big-output"], &limits);
    assert!(captured.truncated);
    assert!(captured.stdout.len() <= 64 * 1024);
    assert_eq!(captured.exit_code, Some(0), "the child still ran to completion");
}

#[test]
fn the_child_runs_in_the_workspace_directory() {
    let workspace = tempfile::tempdir().unwrap();
    let captured = run(workspace.path(), &["cwd"], &limits());
    let reported = String::from_utf8_lossy(&captured.stdout);
    let reported = Path::new(reported.trim());
    assert_eq!(
        reported.canonicalize().unwrap(),
        workspace.path().canonicalize().unwrap()
    );
}
```

The env-dump test's subprocess wrapper is fiddly; the comment block above fixes the final shape — build the wrapper exactly as described (outer test re-executes `std::env::current_exe()` with the sentinels and an inner-marker variable, inner body gated on the marker). If `cargo test` filtering across the re-execution misbehaves, fall back to `#[ignore]`-plus-explicit-name invocation and report.

- [ ] **Step 3: Implement `process.rs`.**

```rust
pub struct ProcessLimits { pub timeout: std::time::Duration, pub max_output_bytes: usize }

pub struct CapturedProcess {
    pub exit_code: Option<i32>,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub truncated: bool,
    pub timed_out: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum HostError {
    #[error("the program failed to spawn")] Spawn { source: std::io::Error },
    #[error("a declared environment name is refused: {name}")] ExtraEnvDenied { name: String },
    /* workspace variants arrive in Task 6 */
}

/// The fixed inheritance allowlist. Everything else the parent holds — passphrases, tokens,
/// profile paths — is structurally absent from the child. HOME/USERPROFILE/TEMP/TMP are not
/// inherited but REDIRECTED into the workspace so git/tools that insist on a home write inside
/// the sandbox and read no host config (GIT_CONFIG_NOSYSTEM closes the /etc side).
const INHERITED: &[&str] = &["PATH", "PATHEXT", "SYSTEMROOT", "SYSTEMDRIVE", "COMSPEC", "WINDIR"];

pub fn run_in_workspace(
    root: &std::path::Path,
    program: &str,
    arguments: &[String],
    extra_env: &std::collections::BTreeMap<String, String>,
    path_prepend: &[std::path::PathBuf],
    limits: &ProcessLimits,
) -> Result<CapturedProcess, HostError> {
    // 1. refuse extra_env names (case-insensitive) that start with "GRAPHHELM_" or that the
    //    host itself defines — anything in INHERITED, the redirected names (HOME, USERPROFILE,
    //    TEMP, TMP), or the fixed GIT_* set below — ExtraEnvDenied (an extra PATH would swap
    //    program resolution out from under the lease);
    // 2. create root/.home and root/.tmp;
    // 3. child PATH = path_prepend entries joined with the OS separator, ahead of the parent's
    //    PATH (path_prepend is host configuration, never caller input);
    // 4. Command::new(program).args(arguments).current_dir(root)
    //        .env_clear()
    //        .envs(INHERITED present in parent, PATH replaced by the composed value)
    //        .env("HOME", root/.home).env("USERPROFILE", root/.home)
    //        .env("TEMP", root/.tmp).env("TMP", root/.tmp)
    //        .env("GIT_CONFIG_NOSYSTEM", "1").env("GIT_TERMINAL_PROMPT", "0")
    //        .env("GIT_OPTIONAL_LOCKS", "0")
    //        .env("GIT_AUTHOR_NAME", "GraphHelm Tool Broker")
    //        .env("GIT_AUTHOR_EMAIL", "tools@graphhelm.invalid")
    //        .env("GIT_COMMITTER_NAME", "GraphHelm Tool Broker")
    //        .env("GIT_COMMITTER_EMAIL", "tools@graphhelm.invalid")
    //        .envs(extra_env)
    //        .stdin(Stdio::null()).stdout(piped).stderr(piped);
    // 5. spawn; one reader thread per pipe accumulating up to max_output_bytes then draining
    //    and discarding (a full pipe with no reader deadlocks the child — drain always);
    // 6. poll try_wait every 50ms against the deadline (the 05b runtime-adapter pattern);
    //    on deadline: kill(), wait(), timed_out = true;
    // 7. join readers, return CapturedProcess.
}
```

- [ ] **Step 4: Run to green, fmt, workspace clippy.**

- [ ] **Step 5: Sabotage (the load-bearing one):** replace `env_clear()` + allowlist with plain inheritance; `the_child_environment_is_an_allowlist_and_never_carries_host_secrets` fails; restore from backup; re-run green. Quote both runs.

- [ ] **Step 6: Commit** `feat(tool-host): scrubbed argv process primitive with deadline and caps`.

---

### Task 6: the Tier 1 workspace — provisioning and containment

**Files:**
- Create: `adapters/tool-host/src/workspace.rs`
- Create: `adapters/tool-host/tests/workspace_containment.rs`

- [ ] **Step 1: Failing tests** in `adapters/tool-host/tests/workspace_containment.rs`. Test scaffolding: build a throwaway git repository (`git init`, one commit with `src/lib.rs`) in a tempdir via `std::process::Command` — the harness may spawn what it likes (rule 7 note).

```rust
use graphhelm_tool_host::workspace::{Tier1Workspace, WorkspaceConfig};

fn scratch_repo() -> (tempfile::TempDir, std::path::PathBuf) { /* git init + commit helper */ }

#[test]
fn provision_creates_a_detached_worktree_and_remove_cleans_it_up() {
    let (_dir, project) = scratch_repo();
    let staging = tempfile::tempdir().unwrap();
    let config = WorkspaceConfig::validated(&project, staging.path(), &[]).unwrap();
    let workspace = Tier1Workspace::provision(&config, "call-1").unwrap();
    assert!(workspace.root().join("src/lib.rs").is_file(), "the worktree carries the project");
    assert!(workspace.root().starts_with(staging.path().canonicalize().unwrap()));
    let root = workspace.root().to_path_buf();
    workspace.remove().unwrap();
    assert!(!root.exists(), "the workspace outlives nothing");
    // And the project repository holds no stale worktree registration:
    // `git -C project worktree list` names exactly one tree (the project itself).
}

#[test]
fn a_staging_directory_inside_the_project_is_refused() {
    let (_dir, project) = scratch_repo();
    assert!(WorkspaceConfig::validated(&project, &project.join(".ghtool"), &[]).is_err());
}

#[test]
fn provisioning_never_runs_repository_hooks() {
    // Threat model §13: "disable hooks by default". `git worktree add` runs the repository's
    // post-checkout hook; a hostile project must not get code execution out of being
    // provisioned. Plant a post-checkout hook in the scratch repo that writes a marker file;
    // provision; assert the marker does not exist.
}

#[test]
fn resolve_contains_paths_and_refuses_a_junction_escape() {
    let (_dir, project) = scratch_repo();
    let staging = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let config = WorkspaceConfig::validated(&project, staging.path(), &[]).unwrap();
    let workspace = Tier1Workspace::provision(&config, "call-1").unwrap();

    let fine = workspace.resolve(&rel("src/lib.rs")).unwrap();
    assert!(fine.starts_with(workspace.root().canonicalize().unwrap()));

    // A directory junction does not need Windows privileges (symlinks do): create
    // workspace/escape -> outside with `cmd /c mklink /J`, then ask for escape/x.txt.
    let junction = workspace.root().join("escape");
    let status = std::process::Command::new("cmd")
        .args(["/c", "mklink", "/J"])
        .arg(&junction).arg(outside.path())
        .status().unwrap();
    assert!(status.success(), "junction creation is the test's own precondition");
    assert!(workspace.resolve(&rel("escape/x.txt")).is_err(), "junction escape must be refused");

    workspace.remove().unwrap();
}

#[test]
fn a_workspace_never_contains_or_equals_a_sensitive_directory() {
    // WorkspaceConfig::validated(project, staging, protected: &[PathBuf]) refuses a staging
    // area that equals, contains, or is contained by any protected path (the caller passes
    // its keyring/broker/events directories). Structural: the constructor is the only door.
    let (_dir, project) = scratch_repo();
    let keyring = tempfile::tempdir().unwrap();
    let inside = keyring.path().join("staging");
    assert!(WorkspaceConfig::validated(&project, &inside, &[keyring.path().into()]).is_err());
}
```

`rel(...)` is `graphhelm_tool_broker::path::RelativePath::parse(...).unwrap()`.

- [ ] **Step 2: Implement `workspace.rs`.**

```rust
pub struct WorkspaceConfig { pub project: std::path::PathBuf, pub staging: std::path::PathBuf }

impl WorkspaceConfig {
    /// The only constructor the host's public API accepts downstream: canonicalizes project
    /// and staging(-parent), refuses staging inside project (a worktree inside the repo
    /// confuses git and every containment rule), refuses overlap with any protected path in
    /// either direction. Credentials are kept out of the workspace structurally: the type
    /// has no credential field, and the directories that hold sealed material never overlap
    /// the workspace tree by this rule.
    pub fn validated(project: &Path, staging: &Path, protected: &[std::path::PathBuf])
        -> Result<Self, HostError>;
}

pub struct Tier1Workspace { root: std::path::PathBuf, project: std::path::PathBuf }

impl Tier1Workspace {
    /// `git -c core.hooksPath=<fresh empty dir> -C <project> worktree add --detach
    /// <staging>/ghtool-<id> HEAD`, spawned directly with std::process::Command (argv, piped
    /// output, the Task 5 deadline pattern — run_in_workspace itself needs an existing root,
    /// which provisioning is creating). `core.hooksPath` pointed at an empty directory is the
    /// threat model §13 "disable hooks by default" control: the repository's own
    /// post-checkout hook must never run during provisioning. Detached: no branch leaks into
    /// the project's ref namespace; commits made inside are reachable by the worktree HEAD
    /// until remove, which is exactly the ephemeral contract.
    pub fn provision(config: &WorkspaceConfig, call_id: &str) -> Result<Self, HostError>;

    pub fn root(&self) -> &std::path::Path;

    /// Lexically-validated RelativePath → real path: join to root, then walk each existing
    /// ancestor with symlink_metadata; any symlink/junction component is refused; finally the
    /// deepest existing ancestor is canonicalized and must start_with the canonical root.
    /// (canonicalize alone cannot vet a not-yet-existing file's parent chain; the walk can.)
    pub fn resolve(&self, path: &graphhelm_tool_broker::path::RelativePath)
        -> Result<std::path::PathBuf, HostError>;

    /// Removal with Windows honesty (review finding 3; the #19 flake class): a freshly
    /// touched tree can hold transient Permission-denied locks (indexer, antivirus), and a
    /// single-shot `git worktree remove` WILL flake. Sequence:
    ///   1. `git -C <project> worktree remove --force <root>` — retry up to 3 times with
    ///      50/250/1000ms backoff on failure;
    ///   2. still failing: `std::fs::remove_dir_all(root)` with the same backoff, then
    ///      `git -C <project> worktree prune` to drop the stale registration;
    ///   3. the tree still exists after all of it: HostError — a leaked workspace is a
    ///      leaked write cap, so the error semantics survive the retries; only transient
    ///      lock friction is absorbed.
    pub fn remove(self) -> Result<(), HostError>;
}
```

Add `HostError` variants: `Workspace { code: &'static str }`-style with stable, content-free messages (`GHTOOL001_WORKSPACE`, `GHTOOL002_ESCAPE`, `GHTOOL003_CONFIG` as internal codes — the CLI maps onto GHCLI codes in Task 9; keep the host's own codes distinct so the CLI mapping is explicit).

- [ ] **Step 3: Run to green, fmt, workspace clippy.** (Windows note: compare canonicalized paths against canonicalized roots so `\\?\` prefixes agree on both sides.)

- [ ] **Step 4: Sabotage:** skip the per-component symlink walk in `resolve` (leave only the join); the junction test fails; restore; re-run green. Quote both.

- [ ] **Step 5: Commit** `feat(tool-host): tier 1 worktree workspace with containment and cleanup`.

---

### Task 7: the three tools and the composed host

**Files:**
- Create: `adapters/tool-host/src/tools.rs`, `adapters/tool-host/src/host.rs`
- Create: `adapters/tool-host/tests/broker_end_to_end.rs`
- Modify: `core/tool-broker/src/call.rs` (+ a test in `authorize_contract.rs`) — the commit-message bound below

- [ ] **Step 1: Failing tests** in `adapters/tool-host/tests/broker_end_to_end.rs` (scaffolding reuses Task 6's `scratch_repo`):

```rust
use graphhelm_tool_broker::call::{RepositoryAction, ShellAction, TestsAction, ToolCall};
use graphhelm_tool_broker::effect::IsolationTier;
use graphhelm_tool_broker::record::ToolDisposition;
use graphhelm_tool_host::host::{HostConfig, ToolHost};

fn host(project: &Path, staging: &Path) -> ToolHost {
    // tests_runner stays a bare name ("fake_tool"); its directory reaches the child through
    // HostConfig::path_prepend (the Task 5 mechanism) — no parent-PATH mutation, no bare-name
    // exception.
    let tool_dir = Path::new(env!("CARGO_BIN_EXE_fake_tool")).parent().unwrap().to_path_buf();
    ToolHost::new(HostConfig {
        workspace: WorkspaceConfig::validated(project, staging, &[]).unwrap(),
        limits: ProcessLimits { timeout: Duration::from_secs(30), max_output_bytes: 1024 * 1024 },
        tests_runner: "fake_tool".to_owned(),
        tests_runner_env: BTreeMap::new(),
        path_prepend: vec![tool_dir],
        keep_workspace: false,
    })
}

#[test]
fn a_tier_0_read_touches_the_project_and_never_provisions_a_workspace() {
    // invoke ReadFile on src/lib.rs with a read lease; assert record.tier == tier_0,
    // disposition Completed, stdout digest equals digest_hex of the file bytes, and the
    // staging directory REMAINS EMPTY (no worktree was created — Tier 0 is workspace-free
    // by construction, not by cleanup).
}

#[test]
fn a_write_call_runs_in_an_ephemeral_worktree_and_the_project_is_untouched() {
    // invoke ApplyPatch (a one-hunk diff adding a line to src/lib.rs) with a write lease;
    // assert disposition Completed; assert the PROJECT's src/lib.rs is byte-identical to
    // before (the write landed in the worktree, which was removed after capture); assert
    // staging is empty again after the call (provision → execute → capture → remove).
}

#[test]
fn commit_inside_the_workspace_never_moves_the_project_head() {
    // ApplyPatch + Commit in one invoke each (two calls, same host); after both, `git -C
    // project rev-parse HEAD` equals the pre-call HEAD. The worktree was detached; its
    // commit died with it. (The record of WHAT was committed is the patch artifact story —
    // 05d externalizes; here the digests prove capture happened.)
}

#[test]
fn the_shell_tool_respects_the_lease_allowlist_end_to_end() {
    // Shell { program: "curl" } under a lease allowing only git/cargo → record.disposition
    // Denied { rule } with the BrokerRefusal::ProgramDenied name; NOTHING was provisioned
    // (staging empty — authorize refuses before the host routes).
}

#[test]
fn the_tests_tool_reports_pass_and_fail_by_exit_code() {
    // tests_runner "fake_tool" resolves through path_prepend (see the host() helper);
    // TestsAction { arguments: ["exit-code", "0"] } → Completed { exit_code: 0 };
    // ["exit-code", "101"] → Completed { exit_code: 101 }. The broker does not interpret
    // runner output in this slice — exit code IS the contract, stated on the type.
}

#[test]
fn tier_0_structurally_rejects_a_write_action() {
    // Defense in depth: hand the host a forged BrokerPlan (tier_0 + ApplyPatch) through the
    // internal routing fn (pub(crate) — test via a #[cfg(test)] unit test inside host.rs
    // instead if visibility bites; report which). Expect HostError::TierViolation, proving
    // the host refuses even if authorize were bypassed.
}
```

- [ ] **Step 2: Implement `tools.rs` + `host.rs`.**

`tools.rs` — each tool is a thin, explicit composition of the two primitives:

```rust
/// Repository reads are in-process file reads under resolve() — Tier 0 runs no child at all
/// for ReadFile, and ListFiles is a bounded directory walk; Diff spawns
/// `git diff --no-ext-diff` (read-only under GIT_OPTIONAL_LOCKS=0) in the project for Tier 0
/// or the workspace for Tier 1.
/// Writes: ApplyPatch spawns `git apply --index` with the patch bytes written to the child's
/// piped stdin and then dropped (stdin is the one per-tool exception to Stdio::null — thread
/// it through run_in_workspace as an Option<Vec<u8>> input added in this task, defaulting to
/// null). Commit spawns `git add -A` then `git commit -m <message>`; the message travels in
/// argv, so call.rs REFUSES messages over 512 bytes or containing control bytes (add the
/// validation and its test now) — the argv stays bounded and printable, and no temp-file
/// write precedes the commit.
pub struct RepositoryTool;
/// Shell: run_in_workspace(root, program, arguments, no extra env).
pub struct ShellTool;
/// Tests: the runner program and its optional declared env come from HostConfig, never the
/// caller; TestsAction.arguments append after the runner's fixed base args.
pub struct TestsTool;
```

`host.rs`:

```rust
pub struct HostConfig {
    pub workspace: WorkspaceConfig,
    pub limits: ProcessLimits,
    pub tests_runner: String,                      // bare program name, validated
    pub tests_runner_env: std::collections::BTreeMap<String, String>, // e.g. CARGO_HOME → a
                                                   // credential-free toolchain home; flows
                                                   // through run_in_workspace's validated
                                                   // extra_env (GRAPHHELM_* and host-defined
                                                   // names still refused)
    /// Directories joined ahead of the child's inherited PATH (run_in_workspace's
    /// path_prepend). Host configuration for pinned toolchains and the test fixture; a
    /// caller can never reach it.
    pub path_prepend: Vec<std::path::PathBuf>,
    /// Skip workspace removal after capture. Its only clients are Task 8's scan-while-alive
    /// assertion and the CLI's --keep-workspace debug flag. A kept workspace is the
    /// operator's to delete. The record stays digest-only either way; the CLI prints the
    /// kept path as its own output field, never inside the record.
    pub keep_workspace: bool,
}

pub struct ToolHost { /* config */ }

// HostError gains TierViolation in this task: the routing layer refuses a write-effect call
// carrying a Tier 0 plan even though authorize can never produce one — defense in depth, and
// the Task 7 sabotage proves the refusal is real.

impl ToolHost {
    pub fn new(config: HostConfig) -> Self;
    /// authorize (pure) → route by plan.tier → execute → digest → remove workspace → record.
    /// A BrokerRefusal becomes ToolDisposition::Denied and NO filesystem action of any kind
    /// happens after it. Every path through this function ends in a ToolCallRecord — denial,
    /// timeout and host error are records too, because 05d must be able to externalize what
    /// happened without a side channel.
    pub fn invoke(&self, call: &ToolCall, lease: &ToolLease, actor: &str)
        -> (graphhelm_tool_broker::record::ToolCallRecord, CapturedStreams);
}

/// The free-form bytes, separated from the record on purpose (D-036 discipline): the record
/// is durable material, the streams are Evidence/operator material.
pub struct CapturedStreams { pub stdout: Vec<u8>, pub stderr: Vec<u8> }
```

- [ ] **Step 3: Run to green, fmt, workspace clippy.**

- [ ] **Step 4: Sabotage:** route `plan.tier == Tier0` write calls through the Tier 1 path anyway by deleting the `TierViolation` guard; `tier_0_structurally_rejects_a_write_action` fails; restore. Then make `invoke` skip workspace `remove()` on the success path; `a_write_call_runs_in_an_ephemeral_worktree_and_the_project_is_untouched` fails on the staging-empty assertion; restore. Quote all four runs.

- [ ] **Step 5: Commit** `feat(tool-host): repository, shell and tests tools behind the composed broker`.

---

### Task 8: the hard-constraint proof

**Files:**
- Modify: `adapters/tool-host/tests/broker_end_to_end.rs` (one more test, the milestone's §8 criterion)

- [ ] **Step 1: Write the failing acceptance test.** This is the register's "Model credentials cannot remain accessible in the same sandbox that runs untrusted code" and the design's "Credentials are demonstrably absent from every Tier 1 workspace" (§8), as one test:

```rust
#[test]
fn credentials_are_demonstrably_absent_from_the_tier_1_workspace() {
    // Arrange — a parent that looks like a real operator machine, worst case:
    //   * GRAPHHELM_EVENTS_KEY + GRAPHHELM_GATEWAY_KEY sentinels in the parent env (via the
    //     Task 5 subprocess wrapper);
    //   * a keyring directory containing a file whose CONTENT is a third sentinel, passed to
    //     WorkspaceConfig::validated as protected;
    //   * the project repo containing a benign file.
    //
    // Act — through the full ToolHost::invoke path, in one Tier 1 workspace lifetime, run
    // the fake_tool via the shell tool (lease allows it) with:
    //   1. env-dump          → captured streams;
    //   2. write-file        → proves the workspace accepts writes (the sandbox is real, not
    //                          broken-by-permission-error, so the absence below is meaningful);
    //
    // Assert:
    //   A. no sentinel value and no GRAPHHELM_* name appears in either captured stream;
    //   B. a recursive scan of the workspace root, taken while the workspace still exists,
    //      finds no file whose bytes contain any sentinel. Integration tests cannot see
    //      #[cfg(test)] items, so the knob is ordinary API kept deliberately ugly:
    //      HostConfig { keep_workspace: bool } marked #[doc(hidden)] with a doc-comment
    //      naming this test as its only client; the CLI's --keep-workspace (Task 9) reuses
    //      it, which is honest — an operator debugging a workspace is this same act;
    //   C. the keyring directory canonicalized shares no prefix with the workspace root
    //      canonicalized (structural separation, both directions);
    //   D. the CLI-visible ToolCallRecord for both calls contains no sentinel (format! the
    //      serialized JSON and scan).
}
```

- [ ] **Step 2: Make it pass** (it should already, if Tasks 5–7 were honest — the test's job is to say so from the outside). Any failure here is a real finding, not test friction: fix the host, never the assertion.

- [ ] **Step 3: Sabotage, the one that matters most:** add `GRAPHHELM_EVENTS_KEY` to `INHERITED` transiently; assertion A fails. Then write a sentinel-bearing file into the workspace from the host code path (simulating a leaked copy); assertion B fails. Restore both; three consecutive green runs. Quote everything.

- [ ] **Step 4: Commit** `test(tool-host): prove credentials are structurally absent from tier 1`.

---

### Task 9: the CLI surface and the gate stage

**Files:**
- Create: `apps/cli/src/commands/tool/mod.rs`, `apps/cli/src/commands/tool/invoke.rs`
- Modify: `apps/cli/src/commands/mod.rs`, `apps/cli/src/args.rs` (register the subcommand), `apps/cli/Cargo.toml` (add `graphhelm-tool-broker`, `graphhelm-tool-host`)
- Create: `apps/cli/tests/tool_cli.rs`
- Modify: `ci/gate.ps1` — CLI suites gain `'tool_cli'` (rule 5)

The command (four-key envelope like every sibling; codes `GHCLI012_TOOL_INVALID`, `GHCLI013_TOOL_DENIED`, `GHCLI014_TOOL_HOST` — renumber per rule 1 if 05b shifted):

```text
graphhelm tool invoke
    --project <dir>            the target repository (Tier 0 reads; worktree source for Tier 1)
    --staging <dir>            where Tier 1 workspaces are provisioned (validated non-overlap)
    --protected <dir>          repeatable; directories the workspace must never overlap
                               (keyring, broker, events) — fed to WorkspaceConfig::validated
    --request <file>           the ToolCall as JSON
    --actor <id>               the caller
    --capability <cap>         repeatable; builds the lease (repository.read,
                               repository.write, shell.execute, tests.execute)
    --allow-program <name>     repeatable; the lease's shell allowlist
    --tests-runner <name>      default: cargo
    --capture-out <dir>        REQUIRED on every invoke, refused with GHCLI012 before anything
                               runs if absent (review finding 7 closed the ambiguity): every
                               call produces captured bytes — a Tier 0 read's file content is
                               its stdout — and they are written there as files (the signal
                               --evidence-out precedent: free-form bytes go to the operator,
                               never into the envelope)
    --keep-workspace           debug only: skip cleanup, print the workspace path
```

Reply `data`: the `ToolCallRecord` serialized as-is (it is digest-only by Task 4's test) plus `capturedTo` paths. `BrokerRefusal` → `GHCLI013` with the refusal's rule name; `HostError` → `GHCLI014` with the host's stable internal code; argument/JSON problems → `GHCLI012`. **No stream content, no absolute path from inside an error, no environment value ever reaches the envelope** — the `Failure` pattern from `execution/mod.rs:50` verbatim.

- [ ] **Step 1: Failing CLI tests** in `apps/cli/tests/tool_cli.rs` (spawn pattern from `execution_cli.rs:11`; scratch repo helper duplicated locally — test crates do not share helpers across binaries):
  - happy Tier 0 `read_file` → `ok: true`, `data.tier == "tier_0"`, digest matches the file bytes hashed in the test;
  - happy Tier 1 `apply_patch` → project untouched afterward, staging empty afterward;
  - `shell` with a non-allowlisted program → exit nonzero, `GHCLI013`, staging untouched;
  - a request file with an unknown field → `GHCLI012`;
  - sentinel discipline: run the Tier 1 case with `GRAPHHELM_EVENTS_KEY=SENTINEL` in the CLI process's env and assert stdout+stderr of the CLI never contain the sentinel (end-to-end restatement of Task 8's assertion D through the real binary);
  - `--capture-out` writes stdout/stderr files whose digests equal the record's.
- [ ] **Step 2: Implement**, run to green: `cargo +1.97.1 test -p graphhelm-cli --test tool_cli --locked`.
- [ ] **Step 3: Add `'tool_cli'` to the gate's suite list** and run the CLI stage block locally.
- [ ] **Step 4: Sabotage:** misspell the suite name in the gate list transiently, watch the stage fail to resolve (the 05b Task 6 pattern proving the stage can go red), restore. fmt + workspace clippy.
- [ ] **Step 5: Commit** `feat(cli): tool invoke over the broker with gate stage`.

---

### Task 9b: the read cache, the `ReuseDecision` kind, and erasure invalidation

*(Amendment from the context-economy elevation debate — owner-approved, issue #33. Numbered 9b
to keep the reviewed Task 1–10 numbering stable; executes between Tasks 9 and 10.)*

**Files:**
- Modify: `core/tool-broker/src/call.rs` (freshness accessor), `core/tool-broker/src/record.rs` (reused flag)
- Create: `adapters/tool-host/src/cache.rs`
- Modify: `adapters/tool-host/src/host.rs` (cache consult between authorize and routing)
- Modify: `core/protocols/src/event.rs` / `simulation.rs` (the `ReuseDecision` kind and its closed enums, `FreshnessClass` included — wire enums live in protocols and `graphhelm-tool-broker` re-exports, the `SignalSeverity` precedent), `core/events/src/projection.rs` (the fold arm), `schemas/event-envelope.schema.json` + `schemas/releases/1.0.0/` + both catalogs (the D-037 ritual)
- Test: `core/tool-broker/tests/authorize_contract.rs`, `adapters/tool-host/tests/broker_end_to_end.rs`, `core/events/tests/execution_projection.rs`, the existing schema conformance suites

- [ ] **Step 1: Freshness classes in the pure crate (failing test first):**

```rust
#[test]
fn freshness_is_declared_per_action_and_only_reads_have_one() {
    use graphhelm_tool_broker::call::FreshnessClass;
    // Repository reads are exact within a source snapshot — the project HEAD pins them:
    for call in [read_call(), list_call(), diff_call()] {
        assert_eq!(call.freshness(), Some(FreshnessClass::SnapshotClosed));
    }
    // Anything that writes or spawns caller-shaped work is never cache-eligible:
    assert_eq!(apply_patch_call().freshness(), None);
    assert_eq!(shell_call("git").freshness(), None);
    assert_eq!(tests_call().freshness(), None);
}
```

`FreshnessClass { ImmutableByInput, SnapshotClosed, Drifting }` is **declared in
`graphhelm-protocols`** (serde snake_case, matching the amended `AGENTS_SKILLS_PLUGINS.md`
§11.3 vocabulary) and re-exported by `graphhelm-tool-broker` — it travels on the wire inside
`ReuseDecision` (Step 3), and a wire enum lives in protocols, the `SignalSeverity` precedent.
This adds `graphhelm-protocols` to the pure crate's pinned dependency set; update the Task 1
source-invariants pin in the same commit and report it. Implement `ToolCall::freshness()
-> Option<FreshnessClass>` — one exhaustive match, no wildcard, so a new action must declare
its class deliberately. Nothing in this slice returns `ImmutableByInput` or `Drifting`; the
variants exist because the vocabulary is the spec's, not this crate's.

- [ ] **Step 2: The host cache (failing tests first):**

```rust
#[test]
fn an_identical_snapshot_closed_read_is_served_from_cache_without_executing() {
    // Two identical Diff invokes against an unchanged, CLEAN project tree: the second must
    // not spawn git (assert the second record carries reused: true and byte-identical
    // digests), and the cache key must include the HEAD commit — amend the project (new
    // commit), invoke again, and the third call is a MISS (reused: false, fresh digests).
}

#[test]
fn a_dirty_working_tree_is_never_served_from_cache() {
    // The re-review's critical finding: Tier 0 reads touch the LIVE working tree, and HEAD
    // does not pin uncommitted changes — a HEAD-keyed hit on a dirty tree would serve stale
    // bytes under the "provably exact" flag. Prime the cache on a clean tree; modify a
    // tracked file WITHOUT committing; invoke identically: the call must bypass the cache
    // (decision forced_fresh, reason dirty_tree), and the returned digests must reflect the
    // dirty bytes, not the cached ones. Revert the file (tree clean again): a hit is legal
    // once more.
}

#[test]
fn cache_entries_die_with_their_evidence() {
    // invalidate_evidence(id) on an entry tagged with that evidence ref: next identical
    // call is a miss. The hard-constraint framing: a cache must never serve
    // cryptographically erased evidence; the broker's cache learns erasure through this
    // API, and 05d's executor is the caller that wires it to EvidenceErasureCompleted.
}
```

Implement `adapters/tool-host/src/cache.rs`: a directory-backed `ReadCache` under the host's
staging area, key = `digest_hex(tool_id_version ++ canonical_call_json ++ lease_scope ++
head_commit)` — a declared subset of `SYSTEM_ARCHITECTURE.md` §7.2's dependency-hash
components (tool versions, input, source snapshot), stated in a doc comment. **Cache
eligibility requires a clean working tree**: before consulting or storing, the host runs
`git status --porcelain` (scrubbed argv spawn, cheap) and any output at all makes the call
`forced_fresh` with reason `dirty_tree` — a dirty tree means the live bytes are not HEAD's
bytes, and the HEAD key is only exact when tree == HEAD. This trades a status call per lookup
for the "provably exact" claim actually being true; the saving survives for the repeated-read
case that motivated the cache (clean trees dominate brokered read traffic). Entries hold the
`ToolCallRecord` + captured stream bytes + an optional evidence ref tag; **no TTL anywhere**
(the snapshot key carries the freshness; the amended spec deleted TTLs deliberately).
`ToolHost::invoke` consults the cache after `authorize` and before routing, only when
`freshness() == Some(SnapshotClosed)` and the plan's tier is Tier 0; a hit returns the stored
record with `reused: true` (new field on `ToolCallRecord`, `#[serde(default)]`-free — the
record is new this milestone). `--capture-out` writes the cached bytes exactly as fresh ones.

- [ ] **Step 3: The `ReuseDecision` kind, by the ritual.** Add to `EventKind` (closed set 25 →
26). The payload was revised by the Task 9b re-review: **closed enums everywhere the house
already closes vocabularies** (the `NodeOutcome`/`SignalSeverity` precedent — no stringly
fields on the wire), **call identity and a key digest** so a decision is auditable and
joinable, and **no cost fields at all**: a unit-less cost on the wire is the retrofit trap
inverted, so cost fields arrive later through the additive-optional path WITH a unit
discriminator, once the gateway's graded unit exists (queued, #35). All enums live in
`graphhelm-protocols` and `graphhelm-tool-broker` re-exports `FreshnessClass` from there:

```rust
ReuseDecision {
    execution_id: OpaqueId,
    node_id: Option<OpaqueId>,        // None from a standalone broker call; Some under 05d
    plane: ReusePlane,                // closed enum: ToolBroker (only variant this milestone)
    decision: ReuseOutcome,           // closed enum: Hit | Miss | ForcedFresh | Excluded
    forced_reason: Option<ForcedFreshReason>, // closed enum: DirtyTree | OperatorForced;
                                      // required iff decision == ForcedFresh (validated)
    freshness_class: Option<FreshnessClass>,  // closed enum, protocols-owned
    key_components: Vec<ReuseKeyComponent>,   // closed enum: ToolVersion | CanonicalInput
                                      //   | LeaseScope | SourceSnapshot — the declared
                                      //   §7.2 subset, typed, never free strings
    key_digest: WireHash,             // digest of the composed key: auditable, content-free
    evidence_ref: Option<EvidenceId>,
    provenance_erased: bool,
}
```

Envelope schema corrected in place, `1.0.0` mirror byte-identical, both catalog digests
recomputed, `checked_in_1_0_0_release_is_complete_and_raw_byte_identical` green unmodified,
plus a serde round-trip test for the new payload. **No producer in this milestone appends it**
— the broker CLI appends nothing to any store; 05d's executor is the producer (04a's
precedent: the seam shipped before its implementor). Say exactly this in the payload's rustdoc.

- [ ] **Step 3b: The fold decision.** Adding a kind breaks `core/events`'s exhaustive
`EventKind` match by design — that is the closed set working. Add the explicit arm in
`apply_projection_event` (`core/events/src/projection.rs`): `ReuseDecision` is **ledger, not
state** — the arm changes no node state, no counter, and is written as an explicit no-op with
a comment saying the savings accounting arrives with the producer (05d), not silently via `_`.
Test in `core/events/tests/execution_projection.rs`: a stream containing a `ReuseDecision`
replays to a projection byte-identical to the same stream without it, twice (replay-stability
for the new kind).

- [ ] **Step 4: Sabotage, twice.** (a) Serve a hit after `invalidate_evidence` — the erasure
test fails; restore. (b) Drop `head_commit` from the key derivation — the new-commit-is-a-miss
assertion fails; restore. Quote all four runs.

- [ ] **Step 5: fmt, workspace clippy, full CLI suite, commit**
`feat(tool-broker): snapshot-keyed read cache with the ReuseDecision kind (D-037)`.

---

### Task 10: documentation and the full gate

**Files:**
- Modify: `docs/milestones/runtime.md` (05c section), `CHANGELOG.md`, `docs/superpowers/specs/2026-08-13-runtime-design.md` (status line for 05c), `docs/INDEX.md`/`DOCUMENTATION_MANIFEST.md` if they enumerate crates (check).

- [ ] **Step 1: Write the 05c section of `docs/milestones/runtime.md` from the code as built** — not from this plan. Cover: the two crates and the purity boundary; the effect→tier rule with `SecretUse` structurally refused; the lease and deny-by-default; the path double-layer (lexical form in the pure crate, symlink truth in the host); the scrubbed environment and the redirected HOME (and why Tier 1's allowlist is stricter than the 05b gateway's — a tool workspace has no auth of its own to keep); the detached-worktree lifecycle; the hard-constraint test by name. **Honest limits section** must name: Tier 1 here is worktree + process scrubbing, not a container — no kernel network deny, no restricted user, no seccomp; the Policy Engine step of §11.2 is fixed rules; redaction is caps + structural env emptiness, not a content scanner; the tests tool's exit-code-only contract; `tests_runner_env` as the declared, recorded concession for toolchain homes and why a credential-free `CARGO_HOME` is the operator's obligation; leases have no expiry until 05d; records are not yet Evidence (05d).
- [ ] **Step 2: Re-read the whole edited file top to bottom.** Every claim must match a test or a line of code you can name.
- [ ] **Step 3: CHANGELOG entry** (mirror the 05a/05b entry style). Update the design spec's status line (`05a implemented` → note 05c implemented, matching however 05b recorded itself).
- [ ] **Step 4: Full gate:** announce on issue #25 before running (machine-wide, never two gates concurrently — coordination protocol), then `./ci/gate.ps1` with `GRAPHHELM_PG_BIN` set → expect `[gate] GREEN`, both PostgreSQL passes included. A red on the tracked flakes (#19) is re-run once with the flake noted; any other red is a defect to fix before closing.
- [ ] **Step 5: Commit** `docs(tool-broker): record milestone 05c as built`.

---

## Definition of done

- `authorize` is a total pure decision: identity, capability, program allowlist, effect, tier — every refusal typed, none echoing content; sabotage-proven.
- `ReadOnly` routes to Tier 0 with **no workspace and no child process for file reads**; every write and every spawn routes to Tier 1; nothing else is representable.
- A Tier 1 call runs in a detached git worktree that did not exist before the call and does not exist after it; the project's files and HEAD are untouched by patches and commits made inside.
- The child environment is `env_clear` plus the fixed allowlist; `HOME`/`USERPROFILE`/`TEMP`/`TMP` point inside the workspace; `GRAPHHELM_*` extras are structurally refused; sabotage-proven.
- Path escape is refused twice: lexically (`RelativePath`) and physically (per-component symlink walk + canonical prefix), junction-escape-proven.
- `credentials_are_demonstrably_absent_from_the_tier_1_workspace` passes — the milestone §8 criterion and the register's hard constraint as one named test, three consecutive green runs after sabotage.
- The `ToolCallRecord` carries digests and dispositions only; stream bytes reach the operator exclusively through `--capture-out`.
- `graphhelm tool invoke` speaks the four-key envelope with GHCLI012–014, redaction-safe; `tool_cli` runs in the gate, proven able to go red; the full gate is green, PostgreSQL matrix included.
- An identical snapshot-closed read is served from the cache without executing, keyed on the project HEAD; a new commit is a miss; an erasure invalidation kills the entry — all sabotage-proven.
- Exactly one new event kind, `ReuseDecision`, added through the complete D-037 ritual (both baseline copies byte-identical, catalogs recomputed, the release-integrity test green unmodified), with no producer until 05d — and zero other schema changes, zero new third-party dependencies.

## What this plan deliberately excludes

The real executor and `NodeOutcome`/Evidence translation (05d — it consumes `ToolCallRecord`); the MCP surface (05e) and monitor (05f); containers, restricted users, kernel network deny, Tiers 2/3 and dynamic elevation (deferred with the milestone doc naming Tier 1's real composition); the Policy Engine; content-aware redaction scanning; tool manifests, plugins and non-local transports; `repository.push`/`pull` and every `NetworkEgress`/`ProductionEffect`/`SecretUse` effect (structurally refused, not absent by accident); lease lifecycle (expiry, revocation on pause); rate limiting beyond output caps.

## Self-review notes (already applied)

- Spec coverage against `runtime-design.md` §7's 05c bullet: repository/shell/tests as brokered local processes ✔ (T5–T7); Tier 0 cognitive ✔ (T1 rule, T7 routing); Tier 1 workspaces ✔ (T6); credentials structurally outside ✔ (T5 scrub, T6 non-overlap, T8 proof). §8 criterion "demonstrably absent" ✔ (T8, named test). §6.3 ✔ (no credential enters any tool API — `SecretUse` refused at `required_tier`). Threat-model §11 checks mapped: schema (T3 deny_unknown), identity (T3), lease (T3), path canonicalization + symlink escape (T2/T6), command allow/deny (T2/T3), secret reference (structurally none), rate/size (output caps T5), output scan (deferred, stated), argv-not-shell (rule 7, T5 by construction).
- The ten-step §11.2 pipeline is mapped step-by-step in its own section with deferrals stated, so the milestone doc cannot quietly claim more than was built.
- Type consistency: `RelativePath` defined once (T2), consumed by `call.rs` (T3), `resolve` (T6); `ToolCallRecord`/`ToolDisposition` defined once (T4), produced by `invoke` (T7), asserted in the CLI (T9); `Capability` names match the CLI's `--capability` strings (T9 parses into the T3 enum).
- Numbering honesty: GHCLI012–014 and the gate suite list assume 05b merged as planned; rule 1 owns the renumbering.

## Review findings applied (Agent A, issue #25)

All seven findings from the 05b owner's review were verified against the plan and applied:

1. **Synthetic git identity** — `env_clear` + empty redirected HOME left `git commit` with no
   `user.name`/`user.email` ("Please tell me who you are"). Fixed `GIT_AUTHOR_*`/`GIT_COMMITTER_*`
   values now ride in the child environment (Task 5), asserted by the env test.
2. **The phantom "Step 2 note"** — the tests-runner fixture wiring referenced a note that was
   never written. Replaced by a real mechanism: `path_prepend` on `HostConfig` flowing into
   `run_in_workspace`'s child-PATH composition (Task 5 test, Task 7 helper) — bare names hold,
   no parent-PATH mutation.
3. **Windows-honest `remove()`** — single-shot `git worktree remove` flakes on freshly-touched
   trees (the #19 class). Retry with backoff, `remove_dir_all` fallback, `worktree prune`,
   error semantics preserved (Task 6).
4. **proptest earned or dropped** — the declared dev-dependency now backs a canonical-form
   property test in `path_rules.rs` (Task 2).
5. **`ActorInvalid` untested** — a malformed-actor test now pins the guard and its position
   before the mismatch comparison (Task 3).
6. **PATH shadow via `extra_env`** — the deny-list now covers every host-defined name
   (INHERITED, redirected, fixed GIT_*), not just `GRAPHHELM_*` (Task 5).
7. **`--capture-out` ambiguity** — required on every invoke, refused with `GHCLI012` before
   anything runs; a Tier 0 read's content is its captured stdout (Task 9).
