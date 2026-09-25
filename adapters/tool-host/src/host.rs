//! The composed host: authorize (pure) → route by tier → execute → digest → remove → record.
//! Every path through [`ToolHost::invoke`] ends in a [`ToolCallRecord`] — denial, timeout and
//! host error are records too, because 05d must be able to externalize what happened without a
//! side channel. The free-form bytes travel beside the record, never inside it (D-036).

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use graphhelm_tool_broker::call::FreshnessClass;
use graphhelm_tool_broker::call::{RepositoryAction, ToolCall};
use graphhelm_tool_broker::effect::{IsolationTier, ToolEffect, required_tier};
use graphhelm_tool_broker::lease::{BrokerPlan, BrokerRefusal, ToolLease, authorize};
use graphhelm_tool_broker::path::validate_program_name;
use graphhelm_tool_broker::record::{ToolCallRecord, ToolDisposition, digest_hex, execution_ref};

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
    /// What makes this host's scratch names its own (Codex, on #1073): the serve path builds
    /// one `ToolHost` per drive over ONE `--staging`, and every host's `call_counter` starts at
    /// zero, so two hosts' first ref probes both named `ghtool-scratch-c0` and one removed the
    /// other's directory between its `create_dir` and its spawn — the probe answered `false`
    /// and the tree was provisioned from `HEAD` instead of the execution's ref. The token is
    /// this process's id and a process-wide monotonic count, taken at construction, so no two
    /// hosts alive at once — in one process or in two — share a scratch name.
    host_token: String,
    /// Raised by [`ToolHost::cancel_all`], read by the spawn loop at its next 50 ms poll (#180).
    ///
    /// Held by the HOST rather than per call, because a caller cancelling a run does not know
    /// which call is in flight -- it knows the execution is over. Every child this host spawns
    /// shares it, which is exactly the scope `cancel_all` names.
    cancel: crate::process::CancelSignal,
    /// One Tier 1 workspace per execution (#1066), keyed by execution id and created on the
    /// first Tier 1 call that names the execution. Every later call of that execution runs in
    /// the SAME tree, so a patch node A applied is still there when node B tests it and node C
    /// commits it; the tree lives until [`ToolHost::release`] and never past it.
    ///
    /// Each slot carries its own lock, held for the whole of a call: two calls of one execution
    /// serialize (one git index, one tree), two executions run side by side. The outer lock is
    /// held only to find or insert a slot, never across a spawn.
    executions: Mutex<BTreeMap<String, Arc<ExecutionSlot>>>,
}

/// An execution's workspace slot. `None` inside means the slot exists but its workspace has
/// not been provisioned yet (or was released while a caller still held the `Arc`).
struct ExecutionSlot {
    workspace: Mutex<Option<Tier1Workspace>>,
    /// The commit `refs/graphhelm/executions/<id>` pointed at when this slot's tree was
    /// provisioned, `None` when the ref did not exist — and the value a landing then expects to
    /// find there (#1073, compare-and-swap): `update-ref <ref> <new> <old>` refuses if anything
    /// else moved the ref meanwhile, instead of silently overwriting it. Advanced on every
    /// successful landing. Guarded by the same lock as the tree, which is what makes the two
    /// consistent.
    landed_at: Mutex<Option<String>>,
    /// Set by [`ToolHost::release`] when it could not take the tree within its bounded wait
    /// because a call or a context scan still holds it (#1086, Codex P1 on #1092). Whoever holds
    /// the lock next — the holder as it lets go, or the next caller as it acquires — removes the
    /// tree under the lock instead, so the removal never races a reader or a writer and the
    /// release never waits on a scan that was abandoned.
    release_requested: std::sync::atomic::AtomicBool,
}

/// How long [`ToolHost::release`] polls for an execution's tree before deferring its removal to
/// whoever holds the tree. A REAL bound: the loop body is a non-blocking `try_lock` and a short
/// sleep, so the longest thing between two checks is one sleep.
const RELEASE_LOCK_WAIT: std::time::Duration = std::time::Duration::from_secs(2);

/// The pause between two non-blocking lock attempts (release, and a scan waiting for the tree).
const LOCK_POLL: std::time::Duration = std::time::Duration::from_millis(10);

/// What a routed execution produced: the capture, plus — for a `commit` that completed — the
/// object id it made and the ref it moved. The three travel together so the record is written
/// from one value and cannot name a commit that did not happen.
struct Executed {
    captured: CapturedProcess,
    commit: Option<String>,
    landed_ref: Option<String>,
    /// A host failure that happened AFTER the capture — the landing refused (#1073, 4b). The
    /// record takes this failure's disposition and still names `commit`: the commit exists in
    /// the workspace, only the ref did not move, and a record that hid the id would send the
    /// operator looking for a commit that is there.
    failure: Option<HostError>,
    /// Whether a stale tree was reclaimed before this call ran (see `ToolCallRecord`).
    recovered_workspace: bool,
}

impl From<CapturedProcess> for Executed {
    fn from(captured: CapturedProcess) -> Self {
        Self {
            captured,
            commit: None,
            landed_ref: None,
            failure: None,
            recovered_workspace: false,
        }
    }
}

/// Where a Tier 1 call runs: a fresh tree torn down after the call (the per-call contract every
/// existing caller relies on), or the execution's shared tree (#1066).
enum Tier1Home<'a> {
    PerCall(Tier1Workspace),
    Execution {
        id: &'a str,
        guard: std::sync::MutexGuard<'a, Option<Tier1Workspace>>,
        slot: &'a ExecutionSlot,
    },
}

impl Tier1Home<'_> {
    fn workspace(&self) -> &Tier1Workspace {
        match self {
            Self::PerCall(workspace) => workspace,
            Self::Execution { guard, .. } => guard
                .as_ref()
                .expect("an execution home is only built around a provisioned workspace"),
        }
    }

    fn root(&self) -> &std::path::Path {
        self.workspace().root()
    }
}

/// The stable rule name a refusal records — the `Denied` disposition's content-free vocabulary.
fn refusal_rule(refusal: &BrokerRefusal) -> &'static str {
    match refusal {
        BrokerRefusal::ActorMismatch { .. } => "actor_mismatch",
        BrokerRefusal::CapabilityMissing { .. } => "capability_missing",
        BrokerRefusal::ProgramDenied => "program_denied",
        BrokerRefusal::ProgramNameInvalid => "program_name_invalid",
        BrokerRefusal::ProgramAllowlistInvalid => "program_allowlist_invalid",
        BrokerRefusal::ActorInvalid => "actor_invalid",
        BrokerRefusal::EffectUnsupported(_) => "effect_unsupported",
    }
}

/// One name for one condition, because it is reported from TWO places and they must not drift:
/// the disposition below (the funnel was read directly and the call has a verdict to record) and
/// `HostError::CaptureLost` (a seam that returns the capture itself and can only refuse). Same
/// defect, same code, so a record search finds both.
const CAPTURE_LOST_CODE: &str = "GHTOOL013_CAPTURE_LOST";

/// Which disposition a captured run records, in the order the four conditions outrank each other.
///
/// **A lost capture outranks everything, including a cancellation.** That is a reversal (Codex, on
/// #703): the chain used to read `cancelled` first, so a cancelled call whose readers were also
/// abandoned recorded `GHTOOL011_CANCELLED` and threw away the only signal that a process escaped
/// containment. The escape then looked like an ordinary stop, with fabricated empty buffers hashed
/// into the record as evidence.
///
/// The two are not the same KIND of fact, which is what settles the order. A cancellation is a
/// cause, and one the reader already knows because they caused it; losing it costs a name. A lost
/// capture is a statement about whether the bytes in this record mean anything at all, and it is
/// the only place a containment failure is visible. Wrong cause, recorded: a human reads the log
/// and recovers. Fabricated evidence, recorded as success: nobody recovers, because nothing looks
/// wrong.
///
/// Cancellation still outranks the deadline for #609's reason -- a cancel raised inside the last
/// poll interval leaves both true, and blaming the clock invents a fault nobody committed.
///
/// `GHTOOL011_CANCELLED` stays narrower than it could be, deliberately: the honest disposition is a
/// `Cancelled` variant of its own, and adding one is a wire-vocabulary change with its own schema
/// and vocabulary-agreement guards. `HostError` reaches `TerminalFailure`, which is the right
/// outcome for a cancellation, so the narrowing costs the NAME and not the behaviour.
fn disposition_for(captured: &CapturedProcess) -> ToolDisposition {
    // BOTH lost-capture causes answer with the same code, and the distinction lives in the
    // record's own fields rather than in the wire vocabulary (#790). From the caller's side the
    // condition is identical -- bytes this call cannot account for -- and adding a second code
    // would be a wire-vocabulary change of the kind the paragraph above prices for
    // `GHTOOL011_CANCELLED`. `readers_abandoned` says a descendant survived the kill;
    // `reader_lost` says a reader thread died holding output. A consumer that needs to tell them
    // apart reads the record; a consumer that only needs to know the capture is untrustworthy
    // reads this code.
    if captured.readers_abandoned || captured.reader_lost {
        ToolDisposition::HostError {
            code: CAPTURE_LOST_CODE.to_owned(),
        }
    } else if captured.cancelled {
        ToolDisposition::HostError {
            code: "GHTOOL011_CANCELLED".to_owned(),
        }
    } else if captured.timed_out {
        ToolDisposition::TimedOut
    } else {
        match captured.exit_code {
            Some(code) => ToolDisposition::Completed { exit_code: code },
            None => ToolDisposition::HostError {
                code: "GHTOOL007_EXIT_UNKNOWN".to_owned(),
            },
        }
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
        // GHTOOL007 is taken by EXIT_UNKNOWN at the disposition layer.
        HostError::ExecutableNotPinned { .. } => "GHTOOL008_EXECUTABLE_UNPINNED".to_owned(),
        HostError::ExecutableMismatch { .. } => "GHTOOL009_EXECUTABLE_MISMATCH".to_owned(),
        HostError::SnapshotMismatch { .. } => "GHTOOL010_SNAPSHOT_MISMATCH".to_owned(),
        HostError::Cancelled => "GHTOOL011_CANCELLED".to_owned(),
        HostError::ProcessGroup { .. } => "GHTOOL012_PROCESS_GROUP".to_owned(),
        HostError::CaptureLost { .. } => CAPTURE_LOST_CODE.to_owned(),
        HostError::RefProbe { .. } => "GHTOOL014_REF_PROBE".to_owned(),
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
        static HOSTS: AtomicU64 = AtomicU64::new(0);
        Self {
            config,
            call_counter: AtomicU64::new(0),
            host_token: format!(
                "{}-{}",
                std::process::id(),
                HOSTS.fetch_add(1, Ordering::Relaxed)
            ),
            cancel: crate::process::CancelSignal::new(),
            executions: Mutex::new(BTreeMap::new()),
        }
    }

    /// Removes the execution's Tier 1 workspace (#1066), if one was provisioned. The driver
    /// calls this when the execution's drive ends — a terminal state, a pause, a cancellation —
    /// and the ref the execution landed (`refs/graphhelm/executions/<id>`) STAYS: the tree was
    /// the execution's scratch, the ref is its result. A later drive of the same execution
    /// provisions its next tree from that ref, so committed work survives the removal and only
    /// uncommitted edits do not.
    ///
    /// The removal is done UNDER the slot's own lock, so it never races a tool writing into the
    /// tree or a context scan reading it. It never waits on that lock without bound (#1086, Codex
    /// P1 on #1092): a context scan abandoned by a cancelled drive may still hold the lock inside
    /// one blocked syscall, and a cancelled drive must not wait on it. So the lock is polled with a
    /// non-blocking `try_lock` for at most [`RELEASE_LOCK_WAIT`]; when it is still held, the release
    /// is recorded on the slot and returns `Ok(())`, and whoever holds the lock removes the tree as
    /// it lets go (or the next caller, as it acquires). Under `keep_workspace` the tree is left in
    /// place exactly as a per-call tree would be: kept is the operator's to delete.
    ///
    /// # Errors
    /// [`HostError::Config`] when the tree could not be removed here — a leaked workspace is a
    /// leaked write capability and is reported, never swallowed. An execution that never
    /// provisioned a workspace releases as `Ok(())`. **Declared residual:** a removal DEFERRED to
    /// the holder has no caller left to report a failure to; the tree then stays on disk at its
    /// deterministic root, and the next provisioning of the execution reclaims it
    /// (`recovered_workspace`, #1073).
    pub fn release(&self, execution_id: &str) -> Result<(), HostError> {
        let slot = self
            .executions
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(execution_id)
            .cloned();
        let Some(slot) = slot else {
            return Ok(());
        };
        slot.release_requested
            .store(true, std::sync::atomic::Ordering::SeqCst);
        let deadline = std::time::Instant::now() + RELEASE_LOCK_WAIT;
        loop {
            match slot.workspace.try_lock() {
                Ok(mut guard) => {
                    return self.reclaim_under_lock(execution_id, &slot, &mut guard, true);
                }
                Err(std::sync::TryLockError::Poisoned(poisoned)) => {
                    let mut guard = poisoned.into_inner();
                    return self.reclaim_under_lock(execution_id, &slot, &mut guard, true);
                }
                Err(std::sync::TryLockError::WouldBlock) => {
                    if std::time::Instant::now() >= deadline {
                        // Deferred: the holder removes the tree as it lets go.
                        return Ok(());
                    }
                    std::thread::sleep(LOCK_POLL);
                }
            }
        }
    }

    /// With the slot's lock held: when a release is pending, take the tree and remove it, and —
    /// unless the caller is about to provision into the slot (`forget_slot: false`) — forget the
    /// slot. A no-op when no release is pending (someone else already did it).
    fn reclaim_under_lock(
        &self,
        execution_id: &str,
        slot: &Arc<ExecutionSlot>,
        guard: &mut Option<Tier1Workspace>,
        forget_slot: bool,
    ) -> Result<(), HostError> {
        if !slot
            .release_requested
            .swap(false, std::sync::atomic::Ordering::SeqCst)
        {
            return Ok(());
        }
        if forget_slot {
            let mut executions = self
                .executions
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if executions
                .get(execution_id)
                .is_some_and(|current| Arc::ptr_eq(current, slot))
            {
                executions.remove(execution_id);
            }
        }
        match guard.take() {
            Some(workspace) if !self.config.keep_workspace => workspace.remove(),
            _ => Ok(()),
        }
    }

    /// Called by a lock holder right after it lets go: performs a release that was deferred to
    /// it. Non-blocking; a failure has no caller to report to (see [`ToolHost::release`]).
    fn reclaim_if_released(&self, execution_id: &str, slot: &Arc<ExecutionSlot>) {
        if !slot
            .release_requested
            .load(std::sync::atomic::Ordering::SeqCst)
        {
            return;
        }
        let guard = match slot.workspace.try_lock() {
            Ok(guard) => guard,
            Err(std::sync::TryLockError::Poisoned(poisoned)) => poisoned.into_inner(),
            Err(std::sync::TryLockError::WouldBlock) => return,
        };
        let mut guard = guard;
        let _ = self.reclaim_under_lock(execution_id, slot, &mut guard, true);
    }

    /// The execution ids that currently hold a provisioned workspace — what
    /// [`ToolHost::release`] would tear down. For the driver's own bookkeeping and for tests;
    /// never a path, never content.
    #[must_use]
    pub fn live_executions(&self) -> Vec<String> {
        self.executions
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .keys()
            .cloned()
            .collect()
    }

    /// Run `read` over `execution_id`'s own Tier 1 tree, holding the tree still (#1086 item 5).
    ///
    /// The slot's lock — the one every tool call of the execution holds for its whole duration —
    /// is held for as long as `read` runs, so no tool of this execution writes while it reads and
    /// a tool call that arrives meanwhile waits for it. `Ok(None)` when the execution has no tree:
    /// none provisioned in this host AND no `refs/graphhelm/executions/<id>` landed in the
    /// project. When the ref exists and no tree does (a resumed execution before its first tool
    /// call), the tree is provisioned from the ref exactly as that first call would provision it,
    /// so a read sees the execution's committed work; a read never provisions from `HEAD`, whose
    /// committed bytes the project checkout already serves. [`ToolHost::release`] removes a tree
    /// provisioned here like any other.
    ///
    /// **Cancellation (#1086, Codex P1 on #1092).** `cancel` is checked before the tree is waited
    /// for, while it is waited for (the lock is polled with `try_lock`, never blocked on), before
    /// provisioning and before `read` runs; `read` is expected to check it too (the channel and
    /// the reader do). A cancelled call answers [`HostError::Cancelled`] and holds nothing. When
    /// `read` returns, the lock is dropped at once and a release that was deferred meanwhile is
    /// performed.
    ///
    /// # Errors
    /// [`HostError::Cancelled`] when `cancel` was set; [`HostError`] when the tree had to be
    /// provisioned and could not be.
    pub fn with_execution_tree<R>(
        &self,
        execution_id: &str,
        cancel: &graphhelm_runtime::ports::ScanCancel,
        read: impl FnOnce(&std::path::Path) -> R,
    ) -> Result<Option<R>, HostError> {
        if cancel.is_cancelled() {
            return Err(HostError::Cancelled);
        }
        let existing = self
            .executions
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(execution_id)
            .cloned();
        let reference = execution_ref(execution_id);
        let (slot, ref_landed) = match existing {
            Some(slot) => (slot, None),
            None => {
                if !self.probe_ref(&reference)? {
                    return Ok(None);
                }
                (self.execution_slot(execution_id), Some(true))
            }
        };
        let outcome = {
            let mut guard = loop {
                if cancel.is_cancelled() {
                    return Err(HostError::Cancelled);
                }
                match slot.workspace.try_lock() {
                    Ok(guard) => break guard,
                    Err(std::sync::TryLockError::Poisoned(poisoned)) => {
                        break poisoned.into_inner();
                    }
                    Err(std::sync::TryLockError::WouldBlock) => std::thread::sleep(LOCK_POLL),
                }
            };
            // A release that was deferred to whoever holds the lock next: the execution is over,
            // so its tree goes, and nothing is read from it.
            if slot
                .release_requested
                .load(std::sync::atomic::Ordering::SeqCst)
            {
                self.reclaim_under_lock(execution_id, &slot, &mut guard, true)?;
                return Ok(None);
            }
            if cancel.is_cancelled() {
                return Err(HostError::Cancelled);
            }
            if guard.is_none() {
                let landed = match ref_landed {
                    Some(landed) => landed,
                    None => self.probe_ref(&reference)?,
                };
                if !landed {
                    return Ok(None);
                }
                if cancel.is_cancelled() {
                    return Err(HostError::Cancelled);
                }
                self.provision_execution_tree(execution_id, &slot, &mut guard, &reference)?;
                // A read spawns nothing, so the fresh tree's cancellation span is parked at once,
                // as a tool call parks it when it returns.
                if let Some(workspace) = guard.as_mut() {
                    workspace.park();
                }
            }
            guard.as_ref().map(|workspace| read(workspace.root()))
        };
        // The lock is already dropped (the block above ended): perform a release deferred to us.
        self.reclaim_if_released(execution_id, &slot);
        Ok(outcome)
    }

    /// Provision `id`'s tree into its slot from `start_point`, and remember what the ref held,
    /// for the landing's compare-and-swap. Shared by the first Tier 1 call and by
    /// [`ToolHost::with_execution_tree`], so the two provision one way.
    fn provision_execution_tree(
        &self,
        id: &str,
        slot: &ExecutionSlot,
        guard: &mut Option<Tier1Workspace>,
        start_point: &str,
    ) -> Result<(), HostError> {
        let workspace = Tier1Workspace::provision_from(
            &self.config.workspace,
            &Self::execution_tree_id(id),
            Some(&self.cancel),
            start_point,
        )?;
        // Read from the TREE (`rev-parse HEAD` there is the ref's commit when provisioned from
        // it), not from the ref again, so a ref that moves between the two reads is caught, not
        // absorbed.
        let landed_at = if start_point == "HEAD" {
            None
        } else {
            self.head_object_id(workspace.root())
        };
        *slot
            .landed_at
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = landed_at;
        *guard = Some(workspace);
        Ok(())
    }

    /// Kill and reap every child this host has in flight (#180).
    ///
    /// The mechanism is the deadline's rather than a new one: the poll loop that already owns
    /// each child gains a second reason to kill it, and reaps by the same two lines. What this
    /// does NOT do is unwind the call -- the invocation still returns a record, now for a child
    /// that stopped, which is what keeps a cancelled run from becoming an absence in the journal.
    ///
    /// One-way, and that matches the caller: a run is cancelled once, and the host that carried
    /// it is not reused afterwards.
    pub fn cancel_all(&self) {
        self.cancel.cancel();
    }

    /// One decided call, end to end. A [`BrokerRefusal`] becomes `Denied` and NO filesystem
    /// action of any kind happens after it; execution failures become `HostError`; a deadline
    /// kill becomes `TimedOut`; a cancellation becomes `HostError { GHTOOL011_CANCELLED }`;
    /// everything else is `Completed` with the child's exit code.
    ///
    /// The cancellation arm is read BEFORE the exit code because a killed child HAS one — this
    /// sentence said "everything else" while a cancelled call fell through to
    /// `Completed { exit_code: 1 }` on Windows, and the description was accurate about the code
    /// while being wrong about the outcome.
    pub fn invoke(
        &self,
        call: &ToolCall,
        lease: &ToolLease,
        actor: &str,
    ) -> (ToolCallRecord, CapturedStreams) {
        self.invoke_inner(call, lease, actor, None)
    }

    /// [`ToolHost::invoke`] inside an execution's own Tier 1 workspace (#1066): the first Tier 1
    /// call of `execution_id` provisions the tree (from `refs/graphhelm/executions/<id>` when an
    /// earlier drive landed it, from `HEAD` otherwise), every later call of the same execution
    /// reuses it, and a `commit` that completes moves that ref to the new object — recorded as
    /// `commit` and `landed_ref` on the [`ToolCallRecord`]. The tree lives until
    /// [`ToolHost::release`]. Authorization is unchanged: the execution id is a workspace key,
    /// not a capability, and the lease decides exactly what it decided before.
    ///
    /// Tier 0 reads are unaffected: they aim at the project as they always have.
    pub fn invoke_for_execution(
        &self,
        execution_id: &str,
        call: &ToolCall,
        lease: &ToolLease,
        actor: &str,
    ) -> (ToolCallRecord, CapturedStreams) {
        self.invoke_inner(call, lease, actor, Some(execution_id))
    }

    fn invoke_inner(
        &self,
        call: &ToolCall,
        lease: &ToolLease,
        actor: &str,
        execution: Option<&str>,
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
                        // A DENIAL touched nothing: no stream was read, so neither reached a
                        // cap. False is the measurement, not a default.
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
                        verified_executable: None,
                        contained_session: None,
                        commit: None,
                        landed_ref: None,
                        recovered_workspace: false,
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

        let executed = self.execute_plan(call, &plan, execution);
        let (disposition, captured, commit, landed_ref, recovered_workspace) = match executed {
            Ok(Executed {
                captured,
                commit,
                landed_ref,
                failure,
                recovered_workspace,
            }) => {
                let disposition = match failure {
                    Some(error) => ToolDisposition::HostError {
                        code: host_error_code(&error),
                    },
                    None => disposition_for(&captured),
                };
                (
                    disposition,
                    captured,
                    commit,
                    landed_ref,
                    recovered_workspace,
                )
            }
            Err(error) => (
                ToolDisposition::HostError {
                    code: host_error_code(&error),
                },
                CapturedProcess {
                    exit_code: None,
                    stdout: Vec::new(),
                    stderr: Vec::new(),
                    // Nothing ran, so neither stream reached a cap. False here is a measurement
                    // about a child that never existed, not a default.
                    stdout_truncated: false,
                    stderr_truncated: false,
                    truncated: false,
                    timed_out: false,
                    // Nothing was read because nothing ran.
                    readers_abandoned: false,
                    reader_lost: false,
                    tree_kill: None,
                    // A refusal before the spawn already carries its own code through
                    // `host_error_code`; this arm is about a child that never existed.
                    cancelled: false,
                },
                None,
                None,
                false,
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
            // Builtin tools run in-process or through the unverified funnel today; the
            // verified doorway's consumer is #543's provider session. Absent, explicitly
            // unknown -- never invented (D-042).
            verified_executable: None,
            contained_session: None,
            commit,
            landed_ref,
            recovered_workspace,
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
        self.execute_plan(call, plan, None)
            .map(|executed| executed.captured)
    }

    fn next_call_id(&self) -> String {
        format!("c{}", self.call_counter.fetch_add(1, Ordering::Relaxed))
    }

    /// A fresh scratch directory under staging for a Tier 0 spawn's CWD, named by this host's
    /// token and the call id (see `host_token`), created with `create_dir` so a name that
    /// somehow already exists is an error rather than a directory shared with whoever made it.
    fn scratch_dir(&self) -> std::io::Result<PathBuf> {
        let scratch = self.config.workspace.staging().join(format!(
            "ghtool-scratch-{}-{}",
            self.host_token,
            self.next_call_id()
        ));
        std::fs::create_dir(&scratch)?;
        Ok(scratch)
    }

    /// The staging-relative name of an execution's tree. `Tier1Workspace::provision` pins its
    /// id to `[a-z0-9-]{1,64}` and an execution id is wider than that (`is_opaque_id` admits most
    /// of printable ASCII), so the tree is named by a digest of the id rather than by the id:
    /// deterministic, always legal, never a path a caller chose. The REF is where the id is
    /// readable (`execution_ref`), and the ref is the audit surface; the tree is scratch.
    fn execution_tree_id(execution_id: &str) -> String {
        format!("exec-{}", &digest_hex(execution_id.as_bytes())[..24])
    }

    /// Finds or creates `execution_id`'s slot. The outer map lock is held only for this lookup.
    fn execution_slot(&self, execution_id: &str) -> Arc<ExecutionSlot> {
        let mut executions = self
            .executions
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        executions
            .entry(execution_id.to_owned())
            .or_insert_with(|| {
                Arc::new(ExecutionSlot {
                    workspace: Mutex::new(None),
                    landed_at: Mutex::new(None),
                    release_requested: std::sync::atomic::AtomicBool::new(false),
                })
            })
            .clone()
    }

    /// Whether `reference` resolves to an object in the PROJECT (#1066): the read-only probe
    /// that decides whether an execution's next tree starts from its own last landing. Through
    /// `run_in_workspace` like every other spawn (Codex, on #1073: it used to run bare, with no
    /// cancel signal and no deadline, so a stalled `rev-parse` could not be paused) — the child's
    /// CWD is an ephemeral scratch sibling under staging, the Tier 0 `diff` shape, so the probe
    /// never writes a byte into the project; `-C` aims git at it. Output is never read, only the
    /// exit status: a failed spawn, a timeout or a cancellation answers `false`, and the caller
    /// then provisions from `HEAD` exactly as before this probe existed.
    fn ref_exists(&self, reference: &str) -> bool {
        self.probe_ref(reference).unwrap_or(false)
    }

    /// The same probe with its failures kept apart from a verified absence (#1086, Codex P1 on
    /// #1092): `Ok(true)` when `rev-parse --verify` exited 0, `Ok(false)` when it exited 1 (the ref
    /// is not there), `Err` for everything else -- no scratch directory, a failed spawn, a timeout,
    /// a cancellation, an exit that says "not a repository". [`ToolHost::with_execution_tree`]
    /// answers `Err` for the last group so the compile reports `Unavailable`, never `Absent`, and
    /// the caller does not read the project checkout in place of a tree it could not see.
    fn probe_ref(&self, reference: &str) -> Result<bool, HostError> {
        let scratch = self
            .scratch_dir()
            .map_err(|_| HostError::RefProbe { rule: "scratch" })?;
        let project = crate::workspace::git_safe(self.config.workspace.project());
        let captured = crate::process::run_in_workspace(
            &scratch,
            "git",
            &[
                "-C".to_owned(),
                project,
                "rev-parse".to_owned(),
                "--verify".to_owned(),
                "--quiet".to_owned(),
                "--end-of-options".to_owned(),
                reference.to_owned(),
            ],
            &BTreeMap::new(),
            &self.config.path_prepend,
            None,
            &self.config.limits,
            Some(&self.cancel),
        );
        let _ = std::fs::remove_dir_all(&scratch);
        let captured = captured?;
        if captured.cancelled {
            return Err(HostError::Cancelled);
        }
        if captured.timed_out {
            return Err(HostError::RefProbe { rule: "timeout" });
        }
        match captured.exit_code {
            Some(0) => Ok(true),
            Some(1) => Ok(false),
            _ => Err(HostError::RefProbe { rule: "exit" }),
        }
    }

    /// The full object id of the workspace's `HEAD` after a commit: `git rev-parse HEAD` under
    /// the same funnel every tool spawn uses, parsed by [`object_id_from_rev_parse`] — 40 hex
    /// (SHA-1) or 64 hex (SHA-256 object format). Anything else — a failed spawn, a non-zero
    /// exit, output that is not an object id — is `None`, and the record then carries no commit
    /// rather than a guess.
    fn head_object_id(&self, workspace_root: &std::path::Path) -> Option<String> {
        let captured = crate::process::run_in_workspace(
            workspace_root,
            "git",
            &["rev-parse".to_owned(), "HEAD".to_owned()],
            &BTreeMap::new(),
            &self.config.path_prepend,
            None,
            &self.config.limits,
            Some(&self.cancel),
        )
        .ok()?;
        if captured.exit_code != Some(0) || captured.readers_abandoned || captured.reader_lost {
            return None;
        }
        object_id_from_rev_parse(&captured.stdout)
    }

    /// `git -C <project> update-ref <ref> <commit>` (#1066): a PLAIN ref under
    /// `refs/graphhelm/executions/`, never `refs/heads`, so no branch the operator has checked
    /// out can move — the operator's `HEAD` is untouched by construction, and the record names
    /// the ref so a reader can `git merge` it deliberately. Runs from the workspace (the child's
    /// CWD) aimed at the project with `-C`, the same shape `RepositoryTool::diff` uses for a
    /// Tier 0 read, so the spawn never has the project as its working directory.
    ///
    /// Compare-and-swap (#1073): `expected` is the commit the ref held when this execution's
    /// tree was provisioned (`None` = the ref must not exist yet), passed as `update-ref`'s
    /// `<oldvalue>` (the empty string for "must not exist"), so a ref moved by anyone else
    /// since — another server, an operator — refuses instead of being overwritten.
    ///
    /// `--no-deref` (Codex, on #1073, reproduced): repository state is untrusted, and a
    /// repository can carry `refs/graphhelm/executions/<id>` as a SYMBOLIC ref aimed at
    /// `refs/heads/main`. Without the flag `update-ref` follows the symref and moves the
    /// branch — the operator's checked-out branch, the thing the plain-ref guarantee exists
    /// to protect. With it the ref itself is rewritten into a direct ref at the commit; the
    /// compare-and-swap still reads through the symref for `<oldvalue>`, so it is the same
    /// check either way. The branch does not move by construction.
    fn land_ref(
        &self,
        workspace_root: &std::path::Path,
        reference: &str,
        commit: &str,
        expected: Option<&str>,
    ) -> Result<(), HostError> {
        let project = crate::workspace::git_safe(self.config.workspace.project());
        let captured = crate::process::run_in_workspace(
            workspace_root,
            "git",
            &[
                "-C".to_owned(),
                project,
                "update-ref".to_owned(),
                "--no-deref".to_owned(),
                reference.to_owned(),
                commit.to_owned(),
                expected.unwrap_or_default().to_owned(),
            ],
            &BTreeMap::new(),
            &self.config.path_prepend,
            None,
            &self.config.limits,
            Some(&self.cancel),
        )?;
        if captured.exit_code == Some(0) && !captured.readers_abandoned && !captured.reader_lost {
            Ok(())
        } else {
            Err(HostError::Config {
                rule: "the execution ref could not be updated",
            })
        }
    }

    fn execute_plan(
        &self,
        call: &ToolCall,
        plan: &BrokerPlan,
        execution: Option<&str>,
    ) -> Result<Executed, HostError> {
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
                        .map(Executed::from)
                }
                ToolCall::Repository(RepositoryAction::ListFiles { prefix }) => {
                    RepositoryTool::list_files(
                        self.config.workspace.project(),
                        prefix.as_ref(),
                        limits,
                    )
                    .map(Executed::from)
                }
                ToolCall::Repository(RepositoryAction::Diff) => {
                    // The scratch sibling keeps the child's CWD (and its .home/.tmp) out of
                    // the project: a Tier 0 read never writes a byte into the tree it reads.
                    let scratch = self
                        .scratch_dir()
                        .map_err(|source| HostError::Prepare { source })?;
                    let result = RepositoryTool::diff(
                        &scratch,
                        self.config.workspace.project(),
                        prepend,
                        limits,
                        Some(&self.cancel),
                    );
                    let _ = std::fs::remove_dir_all(&scratch);
                    result.map(Executed::from)
                }
                _ => Err(HostError::TierViolation),
            },
            IsolationTier::Tier1 => {
                // WHERE the call runs (#1066). Without an execution: a fresh tree per call, torn
                // down below — the contract every existing caller (the CLI's `tool invoke`, the
                // broker suite) was written against, unchanged. With one: the execution's own
                // tree, found or provisioned here and released only by `release`, so this call's
                // writes are the next call's starting state.
                //
                // The slot's lock is taken BEFORE provisioning and held until this arm returns:
                // the second call of an execution that arrives while the first is still running
                // waits here rather than racing it for one git index.
                let slot = execution.map(|id| self.execution_slot(id));
                let home = match (execution, slot.as_ref()) {
                    (Some(id), Some(slot)) => {
                        let mut guard = slot
                            .workspace
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner);
                        // A release deferred to the next lock holder (#1086): the released tree
                        // goes first, and this call provisions a fresh one below, exactly as it
                        // would after a release that had not been deferred.
                        // The slot is kept (`forget_slot: false`): this call provisions into it.
                        self.reclaim_under_lock(id, slot, &mut guard, false)?;
                        if guard.is_none() {
                            // Continue from the execution's own last landing when there is one:
                            // a resumed execution picks up the tree it committed, not the
                            // operator's checkout as it happens to stand now.
                            let reference = execution_ref(id);
                            let start_point = if self.ref_exists(&reference) {
                                reference
                            } else {
                                "HEAD".to_owned()
                            };
                            self.provision_execution_tree(id, slot, &mut guard, &start_point)?;
                        }
                        // Retake the cancellation span for THIS call (a no-op on a tree fresh
                        // from `provision`); it is parked again below when the call returns,
                        // so a pause between calls never waits on a tree with nothing running.
                        guard
                            .as_mut()
                            .expect("provisioned or reused just above")
                            .unpark(Some(&self.cancel))?;
                        Tier1Home::Execution { id, guard, slot }
                    }
                    _ => Tier1Home::PerCall(Tier1Workspace::provision(
                        &self.config.workspace,
                        &self.next_call_id(),
                        Some(&self.cancel),
                    )?),
                };
                let root = home.root().to_path_buf();
                let recovered_workspace = home.workspace().recovered();
                let result = match call {
                    ToolCall::Repository(RepositoryAction::ApplyPatch { patch }) => {
                        RepositoryTool::apply_patch(
                            &root,
                            patch,
                            prepend,
                            limits,
                            Some(&self.cancel),
                        )
                        .map(Executed::from)
                    }
                    ToolCall::Repository(RepositoryAction::Commit { message }) => {
                        RepositoryTool::commit(&root, message, prepend, limits, Some(&self.cancel))
                            .and_then(|captured| self.landed_commit(&home, &root, captured))
                    }
                    ToolCall::Repository(RepositoryAction::Diff) => {
                        RepositoryTool::diff(&root, &root, prepend, limits, Some(&self.cancel))
                            .map(Executed::from)
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
                            &root,
                            &action.program,
                            &action.arguments,
                            prepend,
                            limits,
                            Some(&self.cancel),
                        )
                        .map(Executed::from)
                    }
                    ToolCall::Tests(action) => {
                        validate_program_name(&self.config.tests_runner).map_err(|_| {
                            HostError::Config {
                                rule: "the tests runner must be a bare program name",
                            }
                        })?;
                        TestsTool::run(
                            &root,
                            &self.config.tests_runner,
                            &self.config.tests_runner_env,
                            &action.arguments,
                            prepend,
                            limits,
                            Some(&self.cancel),
                        )
                        .map(Executed::from)
                    }
                };
                let result = result.map(|mut executed| {
                    executed.recovered_workspace = recovered_workspace;
                    executed
                });
                match home {
                    // An execution's tree outlives the call; `release` is its only exit. The
                    // cancellation span does NOT outlive the call (#1073).
                    Tier1Home::Execution { id, mut guard, .. } => {
                        if let Some(workspace) = guard.as_mut() {
                            workspace.park();
                        }
                        drop(guard);
                        // A release that timed out waiting for this call is performed now.
                        if let Some(owned) = slot.as_ref() {
                            self.reclaim_if_released(id, owned);
                        }
                        result
                    }
                    Tier1Home::PerCall(workspace) => {
                        if self.config.keep_workspace {
                            return result;
                        }
                        let removed = workspace.remove();
                        match (result, removed) {
                            (Ok(executed), Ok(())) => Ok(executed),
                            // A leaked workspace is a leaked write capability: removal failure
                            // wins over a successful capture, because the record must not say
                            // "clean".
                            (Ok(_), Err(error)) | (Err(error), _) => Err(error),
                        }
                    }
                }
            }
            IsolationTier::Tier2 | IsolationTier::Tier3 => Err(HostError::TierViolation),
        }
    }

    /// What a completed `commit` records beyond its capture (#1066): the object id it made, and
    /// — inside an execution — the ref it moved. A commit that did not complete (non-zero exit,
    /// a lost capture, a first stage that was final) records neither: the capture already says
    /// what happened, and naming a commit that may not exist would be an invention.
    fn landed_commit(
        &self,
        home: &Tier1Home<'_>,
        root: &std::path::Path,
        captured: CapturedProcess,
    ) -> Result<Executed, HostError> {
        let completed =
            captured.exit_code == Some(0) && !captured.readers_abandoned && !captured.reader_lost;
        if !completed {
            return Ok(Executed::from(captured));
        }
        let commit = self.head_object_id(root);
        // A commit whose object id cannot be read back — a truncated, timed-out, cancelled or
        // failed `rev-parse` — is a LANDING FAILURE, not an optional field (Codex, on #1073):
        // nothing can be landed for it, and a record that said "completed" with no commit and
        // no ref would report a successful commit node that published nothing.
        if commit.is_none() {
            return Ok(Executed {
                captured,
                commit: None,
                landed_ref: None,
                failure: Some(HostError::Config {
                    rule: "the commit's object id could not be read back",
                }),
                recovered_workspace: false,
            });
        }
        let (landed_ref, failure) = match (home, commit.as_deref()) {
            (Tier1Home::Execution { id, slot, .. }, Some(commit)) => {
                let reference = execution_ref(id);
                let mut landed_at = slot
                    .landed_at
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                match self.land_ref(root, &reference, commit, landed_at.as_deref()) {
                    Ok(()) => {
                        *landed_at = Some(commit.to_owned());
                        (Some(reference), None)
                    }
                    // The commit exists; the ref did not move. Both facts go on the record.
                    Err(error) => (None, Some(error)),
                }
            }
            _ => (None, None),
        };
        Ok(Executed {
            captured,
            commit,
            landed_ref,
            failure,
            recovered_workspace: false,
        })
    }
}

/// `git rev-parse HEAD`'s stdout as an object id, or `None`. Both object formats git supports
/// are accepted — 40 hex for SHA-1, 64 hex for a repository initialised with
/// `--object-format=sha256` (Codex, on #1073: a 40-only check turned a SHA-256 repository's
/// commit into `commit: None`, and `landed_commit` then skipped `update-ref` while the call
/// still recorded as completed). Any other width, a non-hex byte, or extra lines is `None`: the
/// record must not name a commit this function could not read back.
fn object_id_from_rev_parse(stdout: &[u8]) -> Option<String> {
    let text = std::str::from_utf8(stdout).ok()?;
    let id = text.trim();
    (matches!(id.len(), 40 | 64) && id.bytes().all(|byte| byte.is_ascii_hexdigit()))
        .then(|| id.to_ascii_lowercase())
}

#[cfg(test)]
mod object_id_tests {
    use super::object_id_from_rev_parse;

    #[test]
    fn a_sha1_object_id_is_accepted() {
        let id = "e83e553422f7c7d375e6aa44236b4ca05a8f7606";
        assert_eq!(
            object_id_from_rev_parse(
                format!(
                    "{id}
"
                )
                .as_bytes()
            )
            .as_deref(),
            Some(id)
        );
    }

    /// THE cell for the review finding: a SHA-256 repository's 64-hex id is an id.
    #[test]
    fn a_sha256_object_id_is_accepted() {
        let id = "a".repeat(64);
        assert_eq!(
            object_id_from_rev_parse(
                format!(
                    "{id}
"
                )
                .as_bytes()
            )
            .as_deref(),
            Some(id.as_str())
        );
    }

    #[test]
    fn uppercase_hex_is_lowercased() {
        assert_eq!(
            object_id_from_rev_parse(b"ABCDEF0123456789ABCDEF0123456789ABCDEF01").as_deref(),
            Some("abcdef0123456789abcdef0123456789abcdef01")
        );
    }

    /// Any other width, a non-hex byte, or a second line is not an object id — the record names
    /// nothing rather than guessing.
    #[test]
    fn anything_else_is_none() {
        for junk in [
            "",
            "HEAD",
            "abc",
            "g".repeat(40).as_str(),
            &"a".repeat(41),
            &"a".repeat(63),
            &"a".repeat(65),
            &format!(
                "{}
{}",
                "a".repeat(40),
                "b".repeat(40)
            ),
        ] {
            assert_eq!(object_id_from_rev_parse(junk.as_bytes()), None, "{junk:?}");
        }
    }
}

#[cfg(test)]
mod disposition_tests {
    use super::{CAPTURE_LOST_CODE, disposition_for};
    use crate::process::CapturedProcess;
    use graphhelm_tool_broker::record::ToolDisposition;

    /// A lost reader answers with the same code an abandoned one does (#790).
    ///
    /// The boundary, not the field: `disposition_for` is where the record's third state becomes
    /// something a caller can act on, and deleting its `|| captured.reader_lost` reddens nothing
    /// without this.
    ///
    /// One code for two causes is deliberate. From the caller's side the condition is identical
    /// -- bytes this call cannot account for -- and the distinction lives in the record's own
    /// fields, so a second wire code would buy nothing and cost a vocabulary change.
    #[test]
    fn a_lost_reader_reports_the_capture_as_lost() {
        let mut lost = captured(false, false, false);
        lost.reader_lost = true;
        assert_eq!(
            disposition_for(&lost),
            ToolDisposition::HostError {
                code: CAPTURE_LOST_CODE.to_owned()
            },
            "a reader that died holding output must not report as a completed run"
        );
    }

    /// CONTROL: with neither cause set, the exit code still decides.
    ///
    /// Without this, a `disposition_for` that answered CAPTURE_LOST unconditionally would satisfy
    /// the cell above.
    #[test]
    fn an_ordinary_capture_still_reports_its_exit_code() {
        assert_eq!(
            disposition_for(&captured(false, false, false)),
            ToolDisposition::Completed { exit_code: 0 },
            "an ordinary capture is not a lost one"
        );
    }

    fn captured(cancelled: bool, readers_abandoned: bool, timed_out: bool) -> CapturedProcess {
        CapturedProcess {
            exit_code: Some(0),
            stdout: Vec::new(),
            stderr: Vec::new(),
            stdout_truncated: false,
            stderr_truncated: false,
            truncated: false,
            timed_out,
            readers_abandoned,
            reader_lost: false,
            tree_kill: None,
            cancelled,
        }
    }

    /// THE cell: both true at once, which is the case the old order got wrong. A cancellation that
    /// also lost its capture means a process escaped containment, and recording it as an ordinary
    /// stop threw that away along with any warning that the empty buffers are fabricated.
    #[test]
    fn a_lost_capture_outranks_a_cancellation() {
        match disposition_for(&captured(true, true, false)) {
            ToolDisposition::HostError { code } => assert_eq!(code, CAPTURE_LOST_CODE),
            other => panic!("a cancelled call that lost its capture recorded {other:?}"),
        }
    }

    /// The control that keeps the cell above from passing for the wrong reason: a cancellation
    /// with an intact capture is still a cancellation, so the guard is an ORDER and not a blanket.
    #[test]
    fn a_cancellation_that_kept_its_capture_is_still_a_cancellation() {
        match disposition_for(&captured(true, false, false)) {
            ToolDisposition::HostError { code } => assert_eq!(code, "GHTOOL011_CANCELLED"),
            other => panic!("an ordinary cancellation recorded {other:?}"),
        }
    }

    /// The deadline loses to a lost capture for the same reason the cancellation does: a
    /// `TimedOut` carrying an empty digest reads as "the tool printed nothing before it hung".
    #[test]
    fn a_lost_capture_outranks_a_deadline() {
        match disposition_for(&captured(false, true, true)) {
            ToolDisposition::HostError { code } => assert_eq!(code, CAPTURE_LOST_CODE),
            other => panic!("a timed-out call that lost its capture recorded {other:?}"),
        }
    }

    /// And the cancellation still outranks the deadline (#609), which the reversal must not undo.
    #[test]
    fn a_cancellation_still_outranks_a_deadline() {
        match disposition_for(&captured(true, false, true)) {
            ToolDisposition::HostError { code } => assert_eq!(code, "GHTOOL011_CANCELLED"),
            other => panic!("a cancelled call that also expired recorded {other:?}"),
        }
    }

    /// Nothing set: the exit code decides, so the chain above is an exception list and not the
    /// normal path.
    #[test]
    fn an_ordinary_run_records_its_exit_code() {
        assert!(matches!(
            disposition_for(&captured(false, false, false)),
            ToolDisposition::Completed { exit_code: 0 }
        ));
    }
}
