//! Explicit, one-shot Codebase Memory operations.
//!
//! This module deliberately owns no daemon and no process pool.  An index build and a query each
//! provision one Tier 1 workspace, run one verified provider process, and remove that workspace
//! before returning.  The bundle is the only reusable state.

use fs2::FileExt as _;
use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

use graphhelm_codebase_memory_mcp::{DecodeLimits, provider::ContainedIndexProvider};
use graphhelm_runtime::ports::SourceReader;
use graphhelm_runtime::retrieval::{
    StructuralIndexRequest, retrieve_coverage, validate_retrieval_receipt,
};
use graphhelm_tool_broker::record::{ToolCallRecord, digest_hex};
use graphhelm_tool_host::process::{CancelSignal, ProcessLimits, run_in_workspace};
use graphhelm_tool_host::session::ContainedProviderSession;
use graphhelm_tool_host::snapshot::{file_manifest, pin_snapshot};
use graphhelm_tool_host::verified::verify_executable;
use graphhelm_tool_host::workspace::{Tier1Workspace, WorkspaceConfig};
use serde::{Deserialize, Serialize};

use crate::args::{CodeIndexBuildArgs, CodeIndexQueryArgs};
use crate::output::Outcome;
const MAX_GIT_LIST_BYTES: usize = 2 * 1024 * 1024;
const MAX_OUTPUT_BYTES: usize = 128 * 1024;
const MAX_EVIDENCE_BYTES: usize = 128 * 1024;
const MAX_EVIDENCE_ENTRIES: usize = 32;

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BundleManifest {
    version: u32,
    project_name: String,
    project_identity: String,
    source_snapshot: String,
    index_generation: String,
    executable_sha256: String,
    actor: String,
    builder_exit_code: Option<i32>,
    builder_timed_out: bool,
    builder_cancelled: bool,
    builder_truncated: bool,
    builder_stdout_sha256: String,
    builder_stderr_sha256: String,
    files: BTreeMap<String, String>,
}

struct ManifestSourceReader {
    root: PathBuf,
    paths: Vec<String>,
    expected: BTreeMap<String, String>,
    failures: std::sync::atomic::AtomicU64,
}

impl ManifestSourceReader {
    fn new(root: &Path, paths: Vec<String>, expected: BTreeMap<String, String>) -> Self {
        Self {
            root: root.to_owned(),
            paths,
            expected,
            failures: std::sync::atomic::AtomicU64::new(0),
        }
    }

    fn current_snapshot(&self) -> graphhelm_protocols::OpaqueId {
        let Ok(current) = file_manifest(&self.root, &self.paths) else {
            return self.unreadable();
        };
        let digest = manifest_digest(&current);
        if current != self.expected {
            return self.unreadable();
        }
        graphhelm_protocols::OpaqueId::parse(digest).expect("manifest digest is opaque-safe")
    }

    fn unreadable(&self) -> graphhelm_protocols::OpaqueId {
        let n = self.failures.fetch_add(1, Ordering::SeqCst);
        graphhelm_protocols::OpaqueId::parse(format!("unreadable-{n}-{}", std::process::id()))
            .expect("failure identity is opaque-safe")
    }
}

impl SourceReader for ManifestSourceReader {
    fn current_snapshot(&self) -> graphhelm_protocols::OpaqueId {
        self.current_snapshot()
    }
}

pub fn build(args: &CodeIndexBuildArgs) -> Outcome {
    let result = build_inner(args);
    match result {
        Ok(data) => Outcome::success("tool.code-index.build", data),
        Err(message) => Outcome::application(
            "tool.code-index.build",
            graphhelm_protocols::Diagnostic::error("GHTOOL_CBM_BUILD", message, "/", "graphhelm"),
        ),
    }
}

pub fn query(args: &CodeIndexQueryArgs) -> Outcome {
    let result = query_inner(args);
    match result {
        Ok(data) => Outcome::success("tool.code-index.query", data),
        Err(message) => Outcome::application(
            "tool.code-index.query",
            graphhelm_protocols::Diagnostic::error("GHTOOL_CBM_QUERY", message, "/", "graphhelm"),
        ),
    }
}

fn build_inner(args: &CodeIndexBuildArgs) -> Result<serde_json::Value, String> {
    refuse_existing_bundle(&args.bundle)?;
    refuse_overlap(&args.project, &args.staging, &args.bundle)?;
    if args.cancel_file.as_ref().is_some_and(|path| path.exists()) {
        return Err("index build cancelled before workspace provisioning".to_owned());
    }
    let executable = verify_executable(&args.executable, &args.sha256).map_err(host_error)?;
    let project_name = deterministic_project_name(&args.project)?;
    let actor = actor_for(args.actor.as_deref())?;
    let config =
        WorkspaceConfig::validated(&args.project, &args.staging, &[]).map_err(host_error)?;
    let signal = args.cancel_file.as_ref().map(|_| CancelSignal::new());
    let workspace = Tier1Workspace::provision(
        &config,
        &format!("cbm-index-build-{}", uuid::Uuid::new_v4().simple()),
        None,
    )
    .map_err(host_error)?;
    let result = build_in_workspace(
        args,
        &workspace,
        &executable,
        &project_name,
        signal.clone(),
        actor,
    );
    let cleanup = workspace.remove().map_err(host_error);
    match (result, cleanup) {
        (Ok(value), Ok(())) => Ok(value),
        (Err(error), _) => Err(error),
        (Ok(_), Err(error)) => Err(error),
    }
}

fn build_in_workspace(
    args: &CodeIndexBuildArgs,
    workspace: &Tier1Workspace,
    executable: &graphhelm_tool_host::verified::VerifiedExecutable,
    project_name: &str,
    signal: Option<CancelSignal>,
    actor: String,
) -> Result<serde_json::Value, String> {
    let paths = tracked_files(workspace.root())?;
    let original_population = tracked_files_at(workspace.root(), Some(&args.project))?;
    if original_population != paths {
        return Err("the source tracked-file population differs from the indexed HEAD".to_owned());
    }
    let workspace_before = file_manifest(workspace.root(), &paths).map_err(host_error)?;
    let source_before = file_manifest(&args.project, &paths).map_err(host_error)?;
    let source_snapshot = manifest_digest(&source_before);
    if source_before != workspace_before {
        return Err("the source changed before indexing started".to_owned());
    }
    if args.cancel_file.as_ref().is_some_and(|path| path.exists()) {
        return Err("index build cancelled before provider invocation".to_owned());
    }
    let watcher = cancellation_watcher(args.cancel_file.clone(), signal.clone());
    let input = serde_json::json!({"repo_path": ".", "name": project_name}).to_string();
    let limits = ProcessLimits {
        timeout: Duration::from_secs(args.timeout_seconds),
        max_output_bytes: MAX_OUTPUT_BYTES,
    };
    let captured_result = run_verified_provider(
        workspace,
        executable,
        &["cli".to_owned(), "index_repository".to_owned(), input],
        None,
        &limits,
        signal.as_ref(),
    )
    .map_err(host_error);
    finish_watcher(watcher);
    let captured = captured_result?;
    if captured.exit_code != Some(0)
        || captured.timed_out
        || captured.cancelled
        || captured.truncated
        || captured.reader_lost
        || captured.readers_abandoned
    {
        return Err("Codebase Memory index build did not complete successfully".to_owned());
    }
    let index_result: serde_json::Value = serde_json::from_slice(&captured.stdout)
        .map_err(|_| "Codebase Memory index build returned invalid output".to_owned())?;
    if index_result
        .get("status")
        .and_then(serde_json::Value::as_str)
        != Some("indexed")
        || index_result
            .get("project")
            .and_then(serde_json::Value::as_str)
            .is_none_or(|project| project != project_name)
    {
        return Err("Codebase Memory did not report a completed index".to_owned());
    }
    let current_files = file_manifest(&args.project, &paths).map_err(host_error)?;
    let workspace_files = file_manifest(workspace.root(), &paths).map_err(host_error)?;
    if current_files != source_before
        || workspace_files != workspace_before
        || tracked_files_at(workspace.root(), Some(&args.project))? != paths
    {
        return Err("the source changed while the index was being built".to_owned());
    }
    publish_bundle(&args.bundle, |temporary| {
        let pinned =
            pin_snapshot(&workspace.root().join(".cbm-cache"), temporary).map_err(host_error)?;
        let manifest = BundleManifest {
            version: 1,
            project_name: project_name.to_owned(),
            project_identity: project_identity(&args.project)?,
            source_snapshot,
            index_generation: pinned.generation().to_owned(),
            executable_sha256: executable.sha256().to_owned(),
            actor,
            builder_exit_code: captured.exit_code,
            builder_timed_out: captured.timed_out,
            builder_cancelled: captured.cancelled,
            builder_truncated: captured.truncated,
            builder_stdout_sha256: digest_hex(&captured.stdout),
            builder_stderr_sha256: digest_hex(&captured.stderr),
            files: source_before,
        };
        write_manifest(temporary, &manifest)?;
        Ok(serde_json::json!({
            "bundle": args.bundle,
            "sourceSnapshot": manifest.source_snapshot,
            "indexGeneration": manifest.index_generation,
            "executableSha256": manifest.executable_sha256,
            "providerExitCode": captured.exit_code,
        }))
    })
}

fn publish_bundle(
    bundle: &Path,
    populate: impl FnOnce(&Path) -> Result<serde_json::Value, String>,
) -> Result<serde_json::Value, String> {
    // Chosen ancestor chains are owner-trusted and stable. Never expose a partially written
    // final bundle; this per-destination lock also prevents two publishers overwriting each other.
    let parent = bundle
        .parent()
        .ok_or("the bundle parent is invalid")?
        .canonicalize()
        .map_err(|_| "the bundle parent is invalid")?;
    let _parent_guard = guard_directory(&parent)?;
    let lock_name = format!(
        ".cbm-publish-{}.lock",
        digest_hex(
            bundle
                .file_name()
                .ok_or("the bundle name is invalid")?
                .to_string_lossy()
                .as_bytes()
        )
    );
    let publication_lock = open_local_file(&parent, &lock_name, false, true)?;
    publication_lock
        .try_lock_exclusive()
        .map_err(|_| "another build is publishing this bundle")?;
    refuse_existing_bundle(bundle)?;
    let temporary = parent.join(format!(".cbm-building-{}", uuid::Uuid::new_v4().simple()));
    fs::create_dir(&temporary).map_err(|_| "the temporary bundle could not be created")?;
    let publish = populate(&temporary).and_then(|value| {
        fs::rename(&temporary, bundle).map_err(|_| "the complete bundle could not be published")?;
        Ok(value)
    });
    // This exact UUID path was created above under the held, trusted parent. No shared path is
    // enumerated or removed, and successful rename leaves nothing at this temporary name.
    if temporary.exists() {
        if temporary.parent() != Some(parent.as_path()) || !temporary.starts_with(&parent) {
            return Err("owned temporary bundle escaped its parent".to_owned());
        }
        fs::remove_dir_all(&temporary).map_err(|_| "owned temporary bundle cleanup failed")?;
    }
    publish
}

fn query_inner(args: &CodeIndexQueryArgs) -> Result<serde_json::Value, String> {
    let manifest = read_manifest(&args.bundle)?;
    refuse_overlap(&args.project, &args.staging, &args.bundle)?;
    let executable = verify_executable(&args.executable, &args.sha256).map_err(host_error)?;
    let actor = actor_for(args.actor.as_deref())?;
    if args.cancel_file.as_ref().is_some_and(|path| path.exists()) {
        return Err("retrieval cancelled before workspace provisioning".to_owned());
    }
    if executable.sha256() != manifest.executable_sha256
        || project_identity(&args.project)? != manifest.project_identity
        || args.paths.is_empty()
        || args.paths.len() > 64
        || args.paths.iter().map(String::len).sum::<usize>() > 8192
    {
        return Err("the executable or query does not match the bundle".to_owned());
    }
    let expected_paths = manifest.files.keys().cloned().collect::<Vec<_>>();
    let current = file_manifest(
        &args.project,
        &manifest.files.keys().cloned().collect::<Vec<_>>(),
    )
    .map_err(host_error)?;
    if current != manifest.files {
        return Err("the project source changed since the index was built".to_owned());
    }
    let reader = ManifestSourceReader::new(&args.project, expected_paths.clone(), current.clone());
    let source_snapshot = reader.current_snapshot().to_string();
    if source_snapshot != manifest.source_snapshot {
        return Err("the project source manifest does not match its bundle identity".to_owned());
    }
    let config =
        WorkspaceConfig::validated(&args.project, &args.staging, &[]).map_err(host_error)?;
    let signal = args.cancel_file.as_ref().map(|_| CancelSignal::new());
    let workspace = Tier1Workspace::provision(
        &config,
        &format!("cbm-index-query-{}", uuid::Uuid::new_v4().simple()),
        None,
    )
    .map_err(host_error)?;
    let result = query_in_workspace(
        args,
        &workspace,
        &executable,
        &manifest,
        signal.clone(),
        actor,
        &reader,
    );
    let cleanup = workspace.remove().map_err(host_error);
    match (result, cleanup) {
        (Ok(value), Ok(())) => Ok(value),
        (Err(error), _) => Err(error),
        (Ok(_), Err(error)) => Err(error),
    }
}

fn query_in_workspace(
    args: &CodeIndexQueryArgs,
    workspace: &Tier1Workspace,
    executable: &graphhelm_tool_host::verified::VerifiedExecutable,
    manifest: &BundleManifest,
    signal: Option<CancelSignal>,
    actor: String,
    reader: &ManifestSourceReader,
) -> Result<serde_json::Value, String> {
    let source_snapshot = manifest.source_snapshot.clone();
    let current_paths = tracked_files(workspace.root())?;
    let expected_paths = manifest.files.keys().cloned().collect::<Vec<_>>();
    if current_paths != expected_paths
        || tracked_files_at(workspace.root(), Some(&args.project))? != expected_paths
    {
        return Err("the tracked source set changed since the index was built".to_owned());
    }
    let pinned =
        pin_snapshot(&args.bundle.join(".index-snapshot"), workspace.root()).map_err(host_error)?;
    if pinned.generation() != manifest.index_generation {
        return Err("the index bundle snapshot does not match its manifest".to_owned());
    }
    let request = request_for(
        &source_snapshot,
        &args.paths,
        args.limit,
        &manifest.project_identity,
        &actor,
    )?;
    if args.cancel_file.as_ref().is_some_and(|path| path.exists()) {
        return Err("retrieval cancelled before provider invocation".to_owned());
    }
    let _bundle_guard = guard_directory(&args.bundle)?;
    let lock = open_local_file(&args.bundle, ".query.lock", false, true)?;
    lock.try_lock_exclusive()
        .map_err(|_| "the bundle is busy in another explicit query".to_owned())?;
    let evidence_dir = args.bundle.join("evidence");
    match fs::create_dir(&evidence_dir) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(_) => return Err("the evidence directory could not be created".to_owned()),
    }
    let _evidence_guard = guard_directory(&evidence_dir)?;
    let key = digest_hex(&serde_json::to_vec(&serde_json::json!({
        "source":source_snapshot,"store":manifest.index_generation,"executable":executable.sha256(),
        "project":manifest.project_identity,"actor":actor,"paths":args.paths,"limit":args.limit
    })).map_err(|_| "the query identity could not be encoded")?);
    let name = format!("query-{key}.json");
    if evidence_dir
        .join(&name)
        .try_exists()
        .map_err(|_| "cached evidence is unreadable")?
    {
        let cached: CachedReceipt =
            serde_json::from_slice(&read_local(&evidence_dir, &name, MAX_EVIDENCE_BYTES)?)
                .map_err(|_| "cached evidence is invalid")?;
        let record = &cached.record;
        if record.actor != actor
            || record
                .verified_executable
                .as_ref()
                .is_none_or(|identity| identity.sha256 != executable.sha256())
            || record
                .contained_session
                .as_ref()
                .is_none_or(|identity| identity.snapshot_generation != manifest.index_generation)
        {
            return Err("cached invocation evidence does not match this query".to_owned());
        }
        let bytes = serde_json::to_vec(&cached.receipt).map_err(|_| "cached receipt is invalid")?;
        validate_retrieval_receipt(&bytes, reader, &request, record)
            .map_err(|_| "cached receipt no longer validates against current source")?;
        if tracked_files_at(workspace.root(), Some(&args.project))? != expected_paths
            || reader.current_snapshot().as_str() != source_snapshot
        {
            return Err("the source changed during cached retrieval validation".to_owned());
        }
        return compact_result(
            &cached.receipt,
            &source_snapshot,
            manifest,
            &evidence_dir.join(name),
            true,
        );
    }
    let entries = fs::read_dir(&evidence_dir)
        .map_err(|_| "the evidence directory is unreadable")?
        .take(MAX_EVIDENCE_ENTRIES + 1)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| "the evidence directory is unreadable")?;
    if entries.len() >= MAX_EVIDENCE_ENTRIES {
        return Err("the bundle reached its 32-entry evidence limit".to_owned());
    }
    let session = ContainedProviderSession::open(workspace, executable.clone(), pinned);
    let provider = ContainedIndexProvider::new(
        session,
        graphhelm_protocols::OpaqueId::parse(source_snapshot.clone())
            .map_err(|_| "invalid source identity")?,
        Vec::new(),
        manifest.project_name.clone(),
        actor,
        DecodeLimits {
            max_result_bytes: MAX_OUTPUT_BYTES,
            max_total_bytes: MAX_OUTPUT_BYTES,
            ..DecodeLimits::default()
        },
        ProcessLimits {
            timeout: Duration::from_secs(args.timeout_seconds),
            max_output_bytes: MAX_OUTPUT_BYTES,
        },
    );
    let provider = match signal.clone() {
        Some(cancel) => provider.with_cancel_signal(cancel),
        None => provider,
    };
    let watcher = cancellation_watcher(args.cancel_file.clone(), signal);
    let receipt_result = retrieve_coverage(&provider, reader, &request);
    finish_watcher(watcher);
    let receipt = match receipt_result {
        Ok(receipt) => receipt,
        Err(_) => {
            if let Some(record) = provider.last_invocation() {
                let bytes = serde_json::to_vec(&record)
                    .map_err(|_| "invocation evidence could not be encoded")?;
                let failure_name = format!("failure-{}.json", digest_hex(&bytes));
                write_local_new(&evidence_dir, &failure_name, &bytes)?;
                let reason = match record.disposition {
                    graphhelm_tool_broker::record::ToolDisposition::TimedOut => {
                        "provider timed out"
                    }
                    graphhelm_tool_broker::record::ToolDisposition::HostError { ref code }
                        if code == "GHTOOL011_CANCELLED" =>
                    {
                        "provider was cancelled"
                    }
                    _ => "provider returned unavailable or invalid evidence",
                };
                return Err(format!(
                    "{reason}; invocation evidence: evidence/{failure_name}"
                ));
            }
            return Err(
                "provider preparation was refused before invocation evidence existed".to_owned(),
            );
        }
    };
    let receipt_json = serde_json::from_slice(
        &receipt
            .stable_bytes()
            .map_err(|_| "receipt encoding failed")?,
    )
    .map_err(|_| "receipt encoding failed")?;
    let cached = CachedReceipt {
        receipt: receipt_json,
        record: receipt.broker_record().clone(),
    };
    let bytes = serde_json::to_vec(&cached).map_err(|_| "retrieval evidence encoding failed")?;
    if bytes.len() > MAX_EVIDENCE_BYTES {
        return Err("retrieval evidence exceeds its declared bound".to_owned());
    }
    if tracked_files_at(workspace.root(), Some(&args.project))? != expected_paths
        || reader.current_snapshot().as_str() != source_snapshot
    {
        return Err("the source changed during retrieval".to_owned());
    }
    write_local_new(&evidence_dir, &name, &bytes)?;
    compact_result(
        &cached.receipt,
        &source_snapshot,
        manifest,
        &evidence_dir.join(name),
        false,
    )
}

#[derive(Serialize, Deserialize)]
struct CachedReceipt {
    receipt: serde_json::Value,
    record: ToolCallRecord,
}

fn compact_result(
    receipt: &serde_json::Value,
    source: &str,
    manifest: &BundleManifest,
    evidence: &Path,
    reused: bool,
) -> Result<serde_json::Value, String> {
    let body = receipt.get("body").ok_or("receipt body missing")?;
    Ok(
        serde_json::json!({"hits":body.get("hits").ok_or("receipt hits missing")?,
        "coverage":body.get("coverage").ok_or("receipt coverage missing")?,
        "sourceSnapshot":source,"indexGeneration":manifest.index_generation,"evidencePath":evidence,
        "reused":reused,"providerInvocations":if reused {0} else {1}}),
    )
}

fn request_for(
    source_snapshot: &str,
    paths: &[String],
    limit: u32,
    project: &str,
    actor: &str,
) -> Result<StructuralIndexRequest, String> {
    use graphhelm_protocols::{
        ArtifactBinding, ArtifactId, DeclaredLimits, DevelopmentEnvelope, DevelopmentKind,
        DevelopmentMetadata, DevelopmentScope, OpaqueId, ProjectId, RetrievalProviderBinding,
        RetrievalStepBinding, SemanticVersion, SnapshotBinding, WireHash, WorkspaceId,
    };
    for path in paths {
        graphhelm_tool_broker::path::RelativePath::parse(path)
            .map_err(|_| "query paths must be canonical repository-relative paths")?;
    }
    let query_input = serde_json::json!({"source":source_snapshot,"project":project,"actor":actor,"paths":paths,"limit":limit});
    let query_digest = digest_hex(query_input.to_string().as_bytes());
    let scope = DevelopmentScope {
        workspace_id: WorkspaceId::parse(format!("workspace-{}", &project[..32]))
            .map_err(|_| "invalid workspace identity")?,
        project_id: ProjectId::parse(format!("project-{}", &project[..32]))
            .map_err(|_| "invalid project identity")?,
        subproject_id: None,
        execution_id: None,
    };
    let hash = |value: &str| {
        WireHash::parse(format!("sha256:{}", &digest_hex(value.as_bytes())[..64]))
            .map_err(|_| "invalid request digest".to_owned())
    };
    let version =
        SemanticVersion::parse("1.0.0").map_err(|_| "invalid semantic version".to_owned())?;
    let id = ArtifactId::parse(format!("retrieval-{}", &query_digest[..32]))
        .map_err(|_| "invalid artifact identity".to_owned())?;
    let producer =
        OpaqueId::parse("runtime-cli").map_err(|_| "invalid producer identity".to_owned())?;
    let snapshot = OpaqueId::parse(source_snapshot.to_owned())
        .map_err(|_| "invalid source identity".to_owned())?;
    let binding_snapshot = SnapshotBinding {
        repo_snapshot: snapshot.clone(),
        index_generation: snapshot,
    };
    let mut binding = ArtifactBinding {
        artifact_id: id.clone(),
        schema_id: "https://p50.dev/schemas/development-envelope.schema.json".to_owned(),
        document_version: version.clone(),
        schema_version: version.clone(),
        digest: hash("plan").map_err(|_| "invalid plan digest")?,
        scope: scope.clone(),
        producer: producer.clone(),
        snapshots: binding_snapshot,
    };
    let mut plan = DevelopmentEnvelope {
        api_version: "p50.dev/development/v1".to_owned(),
        kind: DevelopmentKind::RetrievalPlan,
        metadata: DevelopmentMetadata {
            id,
            artifact_version: version.clone(),
            scope: scope.clone(),
        },
        producer: producer.clone(),
        producer_version: version.clone(),
        bindings: Vec::new(),
        spec: query_input,
        digest: binding.digest.clone(),
        additional: serde_json::Map::new(),
    };
    let digest = digest_hex(plan.digest_input().as_bytes());
    plan.digest = WireHash::parse(format!("sha256:{digest}")).map_err(|_| "invalid plan digest")?;
    binding.digest = plan.digest.clone();
    Ok(StructuralIndexRequest {
        plan,
        plan_binding: binding,
        step: RetrievalStepBinding {
            step_id: OpaqueId::parse("step-cli").map_err(|_| "invalid step identity")?,
            step_digest: hash(&format!("structural-search:{query_digest}"))
                .map_err(|_| "invalid step digest")?,
            query_digest: WireHash::parse(format!("sha256:{query_digest}"))
                .map_err(|_| "invalid query digest")?,
        },
        provider: RetrievalProviderBinding {
            provider_id: OpaqueId::parse("codebase-memory")
                .map_err(|_| "invalid provider identity")?,
            capability_id: OpaqueId::parse("search-graph")
                .map_err(|_| "invalid capability identity")?,
            capability_version: version,
            tool: "structural-code-index".to_owned(),
            action: "search_graph".to_owned(),
        },
        requested_paths: paths.to_vec(),
        negative_scopes: Vec::new(),
        limits: DeclaredLimits {
            max_results: limit,
            max_pages: 1,
            max_bytes: MAX_OUTPUT_BYTES as u64,
            max_tokens: 32_768,
        },
    })
}

fn tracked_files(project: &Path) -> Result<Vec<String>, String> {
    tracked_files_at(project, None)
}

fn tracked_files_at(workspace: &Path, original: Option<&Path>) -> Result<Vec<String>, String> {
    let mut arguments = vec!["-c".to_owned(), "core.fsmonitor=false".to_owned()];
    if let Some(project) = original {
        arguments.push("-C".to_owned());
        arguments.push(project.to_string_lossy().into_owned());
    }
    arguments.extend(["ls-files".to_owned(), "-z".to_owned()]);
    let limits = ProcessLimits {
        timeout: Duration::from_secs(30),
        max_output_bytes: MAX_GIT_LIST_BYTES,
    };
    let captured = run_in_workspace(
        workspace,
        "git",
        &arguments,
        &BTreeMap::new(),
        &[],
        None,
        &limits,
        None,
    )
    .map_err(host_error)?;
    if captured.exit_code != Some(0)
        || captured.truncated
        || captured.timed_out
        || captured.cancelled
        || captured.reader_lost
        || captured.readers_abandoned
    {
        return Err("the tracked-file manifest could not be read".to_owned());
    }
    let mut files = Vec::new();
    for value in captured
        .stdout
        .split(|byte| *byte == 0)
        .filter(|value| !value.is_empty())
    {
        files.push(
            String::from_utf8(value.to_vec())
                .map_err(|_| "the tracked-file manifest is not UTF-8".to_owned())?,
        );
    }
    files.sort();
    if files.is_empty()
        || files.len() > 50_000
        || files.iter().any(|path| {
            matches!(
                path.split('/').next().unwrap_or(""),
                ".home" | ".tmp" | ".cbm-cache" | ".index-snapshot"
            )
        })
    {
        return Err("tracked source uses reserved scratch or exceeds its file bound".to_owned());
    }
    Ok(files)
}

fn project_identity(project: &Path) -> Result<String, String> {
    let canonical = project
        .canonicalize()
        .map_err(|_| "the project root is unreadable")?;
    Ok(digest_hex(canonical.to_string_lossy().as_bytes()))
}

fn deterministic_project_name(project: &Path) -> Result<String, String> {
    Ok(format!("gh-{}", &project_identity(project)?[..24]))
}

fn manifest_digest(files: &BTreeMap<String, String>) -> String {
    format!(
        "sha256-{}",
        digest_hex(&serde_json::to_vec(files).unwrap_or_default())
    )
}

fn refuse_existing_bundle(bundle: &Path) -> Result<(), String> {
    if bundle.exists() {
        Err("the index bundle already exists".to_owned())
    } else {
        Ok(())
    }
}

fn refuse_overlap(project: &Path, staging: &Path, bundle: &Path) -> Result<(), String> {
    let project = project
        .canonicalize()
        .map_err(|_| "the project path is invalid".to_owned())?;
    let staging = staging
        .canonicalize()
        .or_else(|_| {
            staging
                .parent()
                .unwrap_or(staging)
                .canonicalize()
                .map(|parent| parent.join(staging.file_name().unwrap_or_default()))
        })
        .map_err(|_| "the staging path is invalid".to_owned())?;
    let bundle_parent = bundle
        .parent()
        .unwrap_or(bundle)
        .canonicalize()
        .map_err(|_| "the bundle parent is invalid".to_owned())?;
    let name = bundle
        .file_name()
        .ok_or_else(|| "the bundle path is invalid".to_owned())?;
    let bundle_probe = bundle_parent.join(name);
    if bundle_probe.starts_with(&project)
        || project.starts_with(&bundle_probe)
        || bundle_probe.starts_with(&staging)
        || staging.starts_with(&bundle_probe)
    {
        return Err("the bundle must be outside the project and staging roots".to_owned());
    }
    Ok(())
}

fn write_manifest(bundle: &Path, manifest: &BundleManifest) -> Result<(), String> {
    let bytes = serde_json::to_vec_pretty(manifest)
        .map_err(|_| "the index manifest could not be encoded".to_owned())?;
    write_local_new(bundle, "manifest.json", &bytes)
}

fn read_manifest(bundle: &Path) -> Result<BundleManifest, String> {
    let manifest: BundleManifest =
        serde_json::from_slice(&read_local(bundle, "manifest.json", 8 * 1024 * 1024)?)
            .map_err(|_| "the index bundle manifest is invalid")?;
    let hex = |value: &str| {
        value.len() == 64
            && value
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    };
    if manifest.version != 1
        || manifest.files.is_empty()
        || manifest.files.len() > 50_000
        || !hex(&manifest.project_identity)
        || !hex(&manifest.executable_sha256)
        || manifest.source_snapshot != manifest_digest(&manifest.files)
        || manifest
            .index_generation
            .strip_prefix("sha256-")
            .is_none_or(|value| !hex(value))
        || manifest.builder_exit_code != Some(0)
        || manifest.builder_timed_out
        || manifest.builder_cancelled
        || manifest.builder_truncated
        || !hex(&manifest.builder_stdout_sha256)
        || !hex(&manifest.builder_stderr_sha256)
        || manifest.files.iter().any(|(path, digest)| {
            graphhelm_tool_broker::path::RelativePath::parse(path).is_err() || !hex(digest)
        })
    {
        return Err("the index bundle manifest is outside its declared contract".to_owned());
    }
    Ok(manifest)
}

// The explicitly chosen local bundle owner is trusted. Sidecar hashes detect corruption and
// staleness; they are not signatures against an owner who can rewrite both bytes and metadata.
fn guard_directory(path: &Path) -> Result<File, String> {
    let mut options = OpenOptions::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options
            .read(true)
            .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt as _;
        use windows_sys::Win32::Storage::FileSystem::*;
        options
            .access_mode(FILE_READ_ATTRIBUTES)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT);
    }
    let file = options
        .open(path)
        .map_err(|_| "the bundle directory cannot be opened safely")?;
    let metadata = file
        .metadata()
        .map_err(|_| "the bundle directory is unreadable")?;
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt as _;
        if metadata.file_attributes()
            & windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT
            != 0
        {
            return Err("bundle directory links are refused".to_owned());
        }
    }
    if !metadata.is_dir() {
        return Err("the bundle root must be a directory".to_owned());
    }
    Ok(file)
}

fn open_local_file(root: &Path, name: &str, read: bool, lock: bool) -> Result<File, String> {
    if name.is_empty() || name.contains(['/', '\\']) {
        return Err("invalid local evidence name".to_owned());
    }
    let directory = guard_directory(root)?;
    #[cfg(unix)]
    {
        use std::os::fd::{AsRawFd as _, FromRawFd as _};
        let name = std::ffi::CString::new(name).map_err(|_| "invalid local evidence name")?;
        let flags = if read {
            libc::O_RDONLY
        } else {
            libc::O_RDWR | libc::O_CREAT | if lock { 0 } else { libc::O_EXCL }
        };
        // SAFETY: directory and NUL-terminated name live through openat; a successful fd is
        // owned exactly once by File, and NOFOLLOW refuses a planted final-component link.
        let fd = unsafe {
            libc::openat(
                directory.as_raw_fd(),
                name.as_ptr(),
                flags | libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK,
                0o600,
            )
        };
        if fd < 0 {
            return Err("local evidence could not be opened safely".to_owned());
        }
        let file = unsafe { File::from_raw_fd(fd) };
        if !file
            .metadata()
            .map_err(|_| "local evidence is unreadable")?
            .is_file()
        {
            return Err("local evidence must be a regular file".to_owned());
        }
        return Ok(file);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::{MetadataExt as _, OpenOptionsExt as _};
        use windows_sys::Win32::Storage::FileSystem::*;
        let _directory = directory;
        let mut options = OpenOptions::new();
        options
            .read(read || lock)
            .write(!read)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
        if !read {
            if lock {
                options.create(true);
            } else {
                options.create_new(true);
            }
        }
        let file = options
            .open(root.join(name))
            .map_err(|_| "local evidence could not be opened safely")?;
        let metadata = file
            .metadata()
            .map_err(|_| "local evidence is unreadable")?;
        if !metadata.is_file() || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return Err("local evidence must be a regular file".to_owned());
        }
        Ok(file)
    }
}

fn read_local(root: &Path, name: &str, max: usize) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    open_local_file(root, name, true, false)?
        .take(max as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "local evidence is unreadable")?;
    if bytes.len() > max {
        return Err("local evidence exceeds its byte limit".to_owned());
    }
    Ok(bytes)
}

fn write_local_new(root: &Path, name: &str, bytes: &[u8]) -> Result<(), String> {
    let mut file = open_local_file(root, name, false, false)?;
    file.write_all(bytes)
        .and_then(|()| file.sync_all())
        .map_err(|_| "local evidence could not be durably written".to_owned())
}

fn actor_for(explicit: Option<&str>) -> Result<String, String> {
    let actor = explicit
        .map(str::to_owned)
        .or_else(|| std::env::var("GRAPHHELM_ACTOR").ok())
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| "an explicit actor or GRAPHHELM_ACTOR is required".to_owned())?;
    if actor.len() > 256
        || !actor
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
    {
        return Err("the actor identity is invalid".to_owned());
    }
    Ok(actor)
}

fn run_verified_provider(
    root: &Tier1Workspace,
    executable: &graphhelm_tool_host::verified::VerifiedExecutable,
    args: &[String],
    stdin: Option<&[u8]>,
    limits: &ProcessLimits,
    signal: Option<&CancelSignal>,
) -> Result<graphhelm_tool_host::process::CapturedProcess, graphhelm_tool_host::process::HostError>
{
    graphhelm_tool_host::process::run_verified_in_workspace(
        root.root(),
        executable,
        args,
        &BTreeMap::new(),
        &[],
        stdin,
        limits,
        signal,
    )
}

struct CancellationWatcher {
    done: Arc<AtomicBool>,
    handle: thread::JoinHandle<()>,
}

fn cancellation_watcher(
    path: Option<PathBuf>,
    signal: Option<CancelSignal>,
) -> Option<CancellationWatcher> {
    let (Some(path), Some(signal)) = (path, signal) else {
        return None;
    };
    let done = Arc::new(AtomicBool::new(false));
    let stop = Arc::clone(&done);
    let handle = thread::spawn(move || {
        loop {
            if stop.load(Ordering::SeqCst) {
                break;
            }
            if path.exists() {
                signal.cancel();
                break;
            }
            thread::sleep(Duration::from_millis(50));
        }
    });
    Some(CancellationWatcher { done, handle })
}

fn finish_watcher(watcher: Option<CancellationWatcher>) {
    if let Some(watcher) = watcher {
        watcher.done.store(true, Ordering::SeqCst);
        let _ = watcher.handle.join();
    }
}

fn host_error(_error: impl std::fmt::Display) -> String {
    // HostError deliberately stays out of the CLI envelope: its source field can contain an OS
    // path. Operators get the stable command code; detailed process evidence is in the bundle.
    "the broker operation was refused".to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    // Contract: failed snapshot preparation never publishes a final bundle and leaves the same
    // destination retryable. Defect: create-final-before-copy strands an empty/partial bundle.
    // Gap: host snapshot tests observe copy refusal, not CLI publication/retry. No test-only
    // production seam: publish_bundle is the real builder's publication boundary. Cost: local
    // tiny files and directories, no provider, network or Git.
    #[test]
    fn failed_snapshot_publication_leaves_the_destination_retryable() {
        let fixture = tempfile::tempdir().unwrap();
        let source = fixture.path().join("source");
        fs::create_dir(&source).unwrap();
        let bundle = fixture.path().join("bundle");
        let failed = publish_bundle(&bundle, |temporary| {
            pin_snapshot(&source, temporary).map_err(host_error)?;
            Ok(serde_json::json!({}))
        });
        assert!(failed.is_err(), "an empty snapshot must refuse");
        assert!(!bundle.exists(), "failure must not publish a final bundle");
        assert!(
            fs::read_dir(fixture.path()).unwrap().all(|entry| {
                !entry
                    .unwrap()
                    .file_name()
                    .to_string_lossy()
                    .starts_with(".cbm-building-")
            }),
            "failure must remove its own temporary directory"
        );
        fs::write(source.join("index.bin"), b"actual index bytes").unwrap();
        publish_bundle(&bundle, |temporary| {
            pin_snapshot(&source, temporary).map_err(host_error)?;
            write_local_new(temporary, "manifest.json", b"complete manifest")?;
            Ok(serde_json::json!({}))
        })
        .expect("the same destination must remain retryable");
        assert_eq!(
            fs::read(bundle.join(".index-snapshot/index.bin")).unwrap(),
            b"actual index bytes"
        );
        assert_eq!(
            fs::read(bundle.join("manifest.json")).unwrap(),
            b"complete manifest"
        );
    }
}
