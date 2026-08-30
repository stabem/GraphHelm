//! The composed host: authorize (pure) → route by tier → execute → digest → remove → record.
//! Every path through [`ToolHost::invoke`] ends in a [`ToolCallRecord`] — denial, timeout and
//! host error are records too, because 05d must be able to externalize what happened without a
//! side channel. The free-form bytes travel beside the record, never inside it (D-036).

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use graphhelm_tool_broker::call::FreshnessClass;
use graphhelm_tool_broker::call::{RepositoryAction, ToolCall};
use graphhelm_tool_broker::effect::{IsolationTier, ToolEffect, required_tier};
use graphhelm_tool_broker::lease::{BrokerPlan, BrokerRefusal, ToolLease, authorize};
use graphhelm_tool_broker::path::validate_program_name;
use graphhelm_tool_broker::record::{ToolCallRecord, ToolDisposition, digest_hex};

use crate::cache::ReadCache;
use crate::process::{CapturedProcess, HostError, ProcessLimits};
use crate::tools::{RepositoryTool, ShellTool, TestsTool};
use crate::workspace::{Tier1Workspace, WorkspaceConfig};

pub struct HostConfig {
    pub workspace: WorkspaceConfig,
    pub limits: ProcessLimits,
    /// The tests runner as a bare program name, validated at use — host configuration, never
    /// caller input (a caller cannot rename its way around the lease).
    pub tests_runner: String,
    /// Declared env for the runner (e.g. `CARGO_HOME` → a credential-free toolchain home).
    /// Flows through `run_in_workspace`'s validated `extra_env` — `GRAPHHELM_*` and every
    /// host-defined name still refused there.
    pub tests_runner_env: BTreeMap<String, String>,
    /// Directories joined ahead of the child's inherited PATH (`run_in_workspace`'s
    /// `path_prepend`). Host configuration for pinned toolchains and the test fixture; a
    /// caller can never reach it.
    pub path_prepend: Vec<PathBuf>,
    /// Skip workspace removal after capture. Its only clients are Task 8's scan-while-alive
    /// assertion and the CLI's `--keep-workspace` debug flag. A kept workspace is the
    /// operator's to delete. The record stays digest-only either way.
    pub keep_workspace: bool,
}

/// The free-form bytes, separated from the record on purpose (D-036 discipline): the record is
/// durable material, the streams are Evidence/operator material.
pub struct CapturedStreams {
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

pub struct ToolHost {
    config: HostConfig,
    call_counter: AtomicU64,
}

/// The stable rule name a refusal records — the `Denied` disposition's content-free vocabulary.
fn refusal_rule(refusal: &BrokerRefusal) -> &'static str {
    match refusal {
        BrokerRefusal::ActorMismatch { .. } => "actor_mismatch",
        BrokerRefusal::CapabilityMissing { .. } => "capability_missing",
        BrokerRefusal::ProgramDenied => "program_denied",
        BrokerRefusal::ProgramAllowlistInvalid => "program_allowlist_invalid",
        BrokerRefusal::ActorInvalid => "actor_invalid",
        BrokerRefusal::EffectUnsupported(_) => "effect_unsupported",
    }
}

/// The stable internal code a host failure records (`HostError` → `ToolDisposition::HostError`).
fn host_error_code(error: &HostError) -> String {
    match error {
        HostError::Spawn { .. } => "GHTOOL001_SPAWN".to_owned(),
        HostError::ExtraEnvDenied { .. } => "GHTOOL002_ENV".to_owned(),
        HostError::Prepare { .. } => "GHTOOL003_PREPARE".to_owned(),
        HostError::Config { .. } => "GHTOOL004_CONFIG".to_owned(),
        HostError::Escape => "GHTOOL005_ESCAPE".to_owned(),
        HostError::TierViolation => "GHTOOL006_TIER".to_owned(),
    }
}

/// Record-facing names for a call, matching the wire vocabulary's snake_case.
fn call_names(call: &ToolCall) -> (&'static str, &'static str) {
    match call {
        ToolCall::Repository(action) => (
            "repository",
            match action {
                RepositoryAction::ReadFile { .. } => "read_file",
                RepositoryAction::ListFiles { .. } => "list_files",
                RepositoryAction::Diff => "diff",
                RepositoryAction::ApplyPatch { .. } => "apply_patch",
                RepositoryAction::Commit { .. } => "commit",
            },
        ),
        ToolCall::Shell(_) => ("shell", "run"),
        ToolCall::Tests(_) => ("tests", "run"),
    }
}

impl ToolHost {
    #[must_use]
    pub fn new(config: HostConfig) -> Self {
        Self {
            config,
            call_counter: AtomicU64::new(0),
        }
    }

    /// One decided call, end to end. A [`BrokerRefusal`] becomes `Denied` and NO filesystem
    /// action of any kind happens after it; execution failures become `HostError`; a deadline
    /// kill becomes `TimedOut`; everything else is `Completed` with the child's exit code.
    pub fn invoke(
        &self,
        call: &ToolCall,
        lease: &ToolLease,
        actor: &str,
    ) -> (ToolCallRecord, CapturedStreams) {
        let (tool, action) = call_names(call);
        // Recording is its own trust boundary. `authorize` preserves identity/capability
        // precedence, so an earlier refusal may win over a malformed set; either way rejected
        // member bytes must never enter the durable record.
        let program_allowlist = lease
            .validated_program_allowlist()
            .cloned()
            .unwrap_or_default();
        let plan = match authorize(call, lease, actor) {
            Ok(plan) => plan,
            Err(refusal) => {
                // The tier the call WOULD have needed, for the record's shape; a denial
                // touched nothing, so this is classification, not execution truth.
                let tier = required_tier(call.effect()).unwrap_or(IsolationTier::Tier1);
                return (
                    ToolCallRecord {
                        tool: tool.to_owned(),
                        action: action.to_owned(),
                        actor: actor.to_owned(),
                        program_allowlist,
                        tier,
                        disposition: ToolDisposition::Denied {
                            rule: refusal_rule(&refusal).to_owned(),
                        },
                        stdout_sha256: digest_hex(b""),
                        stdout_bytes: 0,
                        stderr_sha256: digest_hex(b""),
                        stderr_bytes: 0,
                        truncated: false,
                        reused: false,
                    },
                    CapturedStreams {
                        stdout: Vec::new(),
                        stderr: Vec::new(),
                    },
                );
            }
        };

        // The read cache (amended pipeline step 6): only a provably-exact shape is eligible —
        // snapshot-closed freshness, Tier 0, and a CLEAN tree (HEAD pins the live bytes only
        // when nothing is dirty; a dirty tree forces fresh, recorded as such by `reused:
        // false`). The key is the declared §7.2 subset; no TTL exists anywhere.
        let cache = ReadCache::new(self.config.workspace.staging());
        let cache_key = if call.freshness() == Some(FreshnessClass::SnapshotClosed)
            && plan.tier == IsolationTier::Tier0
        {
            ReadCache::clean_head(self.config.workspace.project()).map(|head| {
                let canonical = serde_json::to_string(call).unwrap_or_default();
                let mut scope = format!("{}|", lease.actor);
                for capability in &lease.capabilities {
                    scope.push_str(&format!("{capability:?},"));
                }
                ReadCache::key(env!("CARGO_PKG_VERSION"), &canonical, &scope, &head)
            })
        } else {
            None
        };
        if let Some(key) = &cache_key
            && let Some((mut record, stdout, stderr)) = cache.load(key)
        {
            record.reused = true;
            record.actor = actor.to_owned();
            record.program_allowlist = program_allowlist;
            return (record, CapturedStreams { stdout, stderr });
        }

        let executed = self.execute_plan(call, &plan, actor);
        let (disposition, captured) = match executed {
            Ok(captured) => {
                let disposition = if captured.timed_out {
                    ToolDisposition::TimedOut
                } else {
                    match captured.exit_code {
                        Some(code) => ToolDisposition::Completed { exit_code: code },
                        None => ToolDisposition::HostError {
                            code: "GHTOOL007_EXIT_UNKNOWN".to_owned(),
                        },
                    }
                };
                (disposition, captured)
            }
            Err(error) => (
                ToolDisposition::HostError {
                    code: host_error_code(&error),
                },
                CapturedProcess {
                    exit_code: None,
                    stdout: Vec::new(),
                    stderr: Vec::new(),
                    truncated: false,
                    timed_out: false,
                },
            ),
        };

        let reused = false;
        let record = ToolCallRecord {
            tool: tool.to_owned(),
            action: action.to_owned(),
            actor: actor.to_owned(),
            program_allowlist,
            tier: plan.tier,
            disposition,
            stdout_sha256: digest_hex(&captured.stdout),
            stdout_bytes: captured.stdout.len() as u64,
            stderr_sha256: digest_hex(&captured.stderr),
            stderr_bytes: captured.stderr.len() as u64,
            truncated: captured.truncated,
            reused,
        };
        // Store only clean completions of cache-eligible calls; a storage failure is not a
        // call failure (the cache is an economy, not a dependency).
        if let Some(key) = &cache_key
            && matches!(
                record.disposition,
                ToolDisposition::Completed { exit_code: 0 }
            )
        {
            let _ = cache.store(key, &record, &captured.stdout, &captured.stderr);
        }
        (
            record,
            CapturedStreams {
                stdout: captured.stdout,
                stderr: captured.stderr,
            },
        )
    }

    /// The routed execution WITHOUT `authorize` — the forged-plan door Task 7's
    /// defense-in-depth test walks through. Public and `doc(hidden)` rather than
    /// `pub(crate)`, because the integration test lives outside the crate; its only
    /// legitimate callers are `invoke` and that test.
    #[doc(hidden)]
    pub fn execute_plan_for_test(
        &self,
        call: &ToolCall,
        plan: &BrokerPlan,
        actor: &str,
    ) -> Result<CapturedProcess, HostError> {
        let _ = actor;
        self.execute_plan(call, plan, actor)
    }

    fn next_call_id(&self) -> String {
        format!("c{}", self.call_counter.fetch_add(1, Ordering::Relaxed))
    }

    fn execute_plan(
        &self,
        call: &ToolCall,
        plan: &BrokerPlan,
        _actor: &str,
    ) -> Result<CapturedProcess, HostError> {
        // Defense in depth: even a forged plan cannot run write-effect work outside Tier 1.
        // `authorize` can never produce this shape; the host refuses it anyway.
        if plan.effect != ToolEffect::ReadOnly && plan.tier == IsolationTier::Tier0 {
            return Err(HostError::TierViolation);
        }

        let limits = &self.config.limits;
        let prepend = &self.config.path_prepend;
        match plan.tier {
            IsolationTier::Tier0 => match call {
                ToolCall::Repository(RepositoryAction::ReadFile { path }) => {
                    RepositoryTool::read_file(self.config.workspace.project(), path, limits)
                }
                ToolCall::Repository(RepositoryAction::ListFiles { prefix }) => {
                    RepositoryTool::list_files(
                        self.config.workspace.project(),
                        prefix.as_ref(),
                        limits,
                    )
                }
                ToolCall::Repository(RepositoryAction::Diff) => {
                    // The scratch sibling keeps the child's CWD (and its .home/.tmp) out of
                    // the project: a Tier 0 read never writes a byte into the tree it reads.
                    let scratch = self
                        .config
                        .workspace
                        .staging()
                        .join(format!("ghtool-scratch-{}", self.next_call_id()));
                    std::fs::create_dir_all(&scratch)
                        .map_err(|source| HostError::Prepare { source })?;
                    let result = RepositoryTool::diff(
                        &scratch,
                        self.config.workspace.project(),
                        prepend,
                        limits,
                    );
                    let _ = std::fs::remove_dir_all(&scratch);
                    result
                }
                _ => Err(HostError::TierViolation),
            },
            IsolationTier::Tier1 => {
                let workspace =
                    Tier1Workspace::provision(&self.config.workspace, &self.next_call_id())?;
                let result = match call {
                    ToolCall::Repository(RepositoryAction::ApplyPatch { patch }) => {
                        RepositoryTool::apply_patch(workspace.root(), patch, prepend, limits)
                    }
                    ToolCall::Repository(RepositoryAction::Commit { message }) => {
                        RepositoryTool::commit(workspace.root(), message, prepend, limits)
                    }
                    ToolCall::Repository(RepositoryAction::Diff) => {
                        RepositoryTool::diff(workspace.root(), workspace.root(), prepend, limits)
                    }
                    ToolCall::Repository(
                        RepositoryAction::ReadFile { .. } | RepositoryAction::ListFiles { .. },
                    ) => {
                        // Reads route Tier 0; a read plan carrying Tier 1 is not a shape
                        // `authorize` produces. Refuse rather than guess.
                        Err(HostError::TierViolation)
                    }
                    ToolCall::Shell(action) => {
                        // Belt and braces mirroring authorize: the program shape was already
                        // validated, but this host is the last line before a spawn.
                        validate_program_name(&action.program)
                            .map_err(|_| HostError::TierViolation)?;
                        ShellTool::run(
                            workspace.root(),
                            &action.program,
                            &action.arguments,
                            prepend,
                            limits,
                        )
                    }
                    ToolCall::Tests(action) => {
                        validate_program_name(&self.config.tests_runner).map_err(|_| {
                            HostError::Config {
                                rule: "the tests runner must be a bare program name",
                            }
                        })?;
                        TestsTool::run(
                            workspace.root(),
                            &self.config.tests_runner,
                            &self.config.tests_runner_env,
                            &action.arguments,
                            prepend,
                            limits,
                        )
                    }
                };
                if self.config.keep_workspace {
                    return result;
                }
                let removed = workspace.remove();
                match (result, removed) {
                    (Ok(captured), Ok(())) => Ok(captured),
                    // A leaked workspace is a leaked write capability: removal failure wins
                    // over a successful capture, because the record must not say "clean".
                    (Ok(_), Err(error)) | (Err(error), _) => Err(error),
                }
            }
            IsolationTier::Tier2 | IsolationTier::Tier3 => Err(HostError::TierViolation),
        }
    }
}
