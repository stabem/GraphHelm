//! The producer, end to end (#219): a REAL contained session (verified executable, pinned
//! snapshot, provisioned Tier 1 workspace) speaking one-shot MCP framing to a provider binary,
//! decoded by the bounded decoder, validated by `retrieve_coverage` against the REAL
//! `WorkspaceSourceReader` — the whole month's gates, composed and observed working together.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use graphhelm_codebase_memory_mcp::DecodeLimits;
use graphhelm_codebase_memory_mcp::provider::ContainedIndexProvider;
use graphhelm_runtime::ports::{SourceReader, StructuralCodeIndex};
use graphhelm_runtime::retrieval::{StructuralIndexRequest, retrieve_coverage};
use graphhelm_tool_broker::record::digest_hex;
use graphhelm_tool_host::process::ProcessLimits;
use graphhelm_tool_host::session::ContainedProviderSession;
use graphhelm_tool_host::snapshot::pin_snapshot;
use graphhelm_tool_host::source_reader::{SourceReaderLimits, WorkspaceSourceReader};
use graphhelm_tool_host::verified::verify_executable;
use graphhelm_tool_host::workspace::{Tier1Workspace, WorkspaceConfig};

fn fake_server() -> String {
    env!("CARGO_BIN_EXE_fake_mcp_server").to_owned()
}

fn process_limits() -> ProcessLimits {
    ProcessLimits {
        timeout: Duration::from_secs(20),
        max_output_bytes: 1024 * 1024,
    }
}

fn scratch_repo() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let project = dir.path().join("project");
    std::fs::create_dir_all(project.join("src")).unwrap();
    std::fs::write(project.join("src/lib.rs"), "// scratch\n").unwrap();
    let git = |args: &[&str]| {
        let status = Command::new("git")
            .args(args)
            .current_dir(&project)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_AUTHOR_NAME", "scratch")
            .env("GIT_AUTHOR_EMAIL", "scratch@test.invalid")
            .env("GIT_COMMITTER_NAME", "scratch")
            .env("GIT_COMMITTER_EMAIL", "scratch@test.invalid")
            .status()
            .unwrap();
        assert!(status.success(), "git {args:?} failed");
    };
    git(&["init", "--quiet"]);
    git(&["add", "-A"]);
    git(&["commit", "--quiet", "-m", "scratch"]);
    (dir, project)
}

/// The full contained composition: provisioned workspace, verified fake server, pinned snapshot
/// of a tiny index tree, session over all three. The provider itself is built per cell, because
/// `indexed_over` (the repo snapshot the index claims to be built from) is the axis the cells
/// vary.
fn composed(
    call_id: &str,
) -> (
    tempfile::TempDir,
    tempfile::TempDir,
    ContainedProviderSession,
    Tier1Workspace,
    PathBuf,
) {
    let (project_dir, project) = scratch_repo();
    let staging = tempfile::tempdir().unwrap();
    let config = WorkspaceConfig::validated(&project, staging.path(), &[]).unwrap();
    let workspace = Tier1Workspace::provision(&config, call_id, None).unwrap();

    let exe = fake_server();
    let verified =
        verify_executable(Path::new(&exe), &digest_hex(&std::fs::read(&exe).unwrap())).unwrap();
    let index_source = workspace.root().join("index-source");
    std::fs::create_dir_all(&index_source).unwrap();
    std::fs::write(index_source.join("graph.bin"), b"index bytes v1").unwrap();
    let pinned = pin_snapshot(&index_source, workspace.root()).unwrap();
    let session = ContainedProviderSession::open(&workspace, verified, pinned);
    (project_dir, staging, session, workspace, project)
}

fn provider_over(
    session: ContainedProviderSession,
    indexed_over: &str,
    mode: &str,
) -> ContainedIndexProvider {
    ContainedIndexProvider::new(
        session,
        graphhelm_protocols::OpaqueId::parse(indexed_over).unwrap(),
        vec![mode.to_owned()],
        "graphhelm-main".to_owned(),
        "retrieval-runtime".to_owned(),
        DecodeLimits::default(),
        process_limits(),
    )
}

/// A request whose binding is built FROM the composition's own identities, the way the plan
/// compiler will build it: repo snapshot from the reader, index generation from the session.
/// Typed construction, mirroring the runtime's own receipt_request fixture (the request struct
/// is deliberately not Deserialize).
fn request_for(reader_snapshot: &str, index_generation: &str) -> StructuralIndexRequest {
    use graphhelm_protocols::{
        ArtifactBinding, ArtifactId, DeclaredLimits, DevelopmentEnvelope, DevelopmentKind,
        DevelopmentMetadata, DevelopmentScope, OpaqueId, ProjectId, RetrievalProviderBinding,
        RetrievalStepBinding, SemanticVersion, SnapshotBinding, WireHash, WorkspaceId,
    };
    let scope = DevelopmentScope {
        workspace_id: WorkspaceId::parse("workspace-producer").unwrap(),
        project_id: ProjectId::parse("project-producer").unwrap(),
        subproject_id: None,
        execution_id: None,
    };
    let hash = |letter: char| {
        WireHash::parse(format!("sha256:{}", letter.to_string().repeat(64))).unwrap()
    };
    let mut plan_binding = ArtifactBinding {
        artifact_id: ArtifactId::parse("retrieval-plan-0001").unwrap(),
        schema_id: "https://p50.dev/schemas/development-envelope.schema.json".to_owned(),
        document_version: SemanticVersion::parse("1.0.0").unwrap(),
        schema_version: SemanticVersion::parse("1.0.0").unwrap(),
        digest: hash('a'),
        scope: scope.clone(),
        producer: OpaqueId::parse("runtime-planner").unwrap(),
        snapshots: SnapshotBinding {
            repo_snapshot: OpaqueId::parse(reader_snapshot).unwrap(),
            index_generation: OpaqueId::parse(index_generation).unwrap(),
        },
    };
    let mut plan = DevelopmentEnvelope {
        api_version: "p50.dev/development/v1".to_owned(),
        kind: DevelopmentKind::RetrievalPlan,
        metadata: DevelopmentMetadata {
            id: plan_binding.artifact_id.clone(),
            artifact_version: plan_binding.document_version.clone(),
            scope: scope.clone(),
        },
        producer: plan_binding.producer.clone(),
        producer_version: SemanticVersion::parse("1.0.0").unwrap(),
        bindings: Vec::new(),
        spec: serde_json::json!({}),
        digest: plan_binding.digest.clone(),
        additional: serde_json::Map::new(),
    };
    plan.digest = WireHash::parse(format!(
        "sha256:{}",
        hex_digest(plan.digest_input().as_bytes())
    ))
    .unwrap();
    plan_binding.digest = plan.digest.clone();
    StructuralIndexRequest {
        plan,
        plan_binding,
        step: RetrievalStepBinding {
            step_id: OpaqueId::parse("step-structural-search").unwrap(),
            step_digest: hash('b'),
            query_digest: hash('c'),
        },
        provider: RetrievalProviderBinding {
            provider_id: OpaqueId::parse("codebase-memory").unwrap(),
            capability_id: OpaqueId::parse("search-graph").unwrap(),
            capability_version: SemanticVersion::parse("1.0.0").unwrap(),
            tool: "structural-code-index".to_owned(),
            action: "search_graph".to_owned(),
        },
        requested_paths: vec!["core/events/src/local.rs".to_owned()],
        negative_scopes: Vec::new(),
        limits: DeclaredLimits {
            max_results: 50,
            max_pages: 8,
            max_bytes: 1_000_000,
            max_tokens: 250_000,
        },
    }
}

fn hex_digest(bytes: &[u8]) -> String {
    graphhelm_tool_broker::record::digest_hex(bytes)
}

/// THE COMPOSITION CELL: everything this month built, working at once. The receipt validates,
/// and its broker record names the program AND the session -- D-042's closing demand, produced.
#[test]
fn the_contained_producer_yields_a_validated_receipt_naming_program_and_session() {
    // The reader reads the PROJECT tree, the way production does — never the provider's
    // workspace, whose sandbox dirs the session writes into on every call (the serving copy of
    // the pin, at minimum), which would move the reader's snapshot mid-validation.
    let (_project, _staging, session, _workspace, project) = composed("producer-a");
    let session_generation = session.identity().snapshot_generation.clone();
    let reader = WorkspaceSourceReader::open(
        &project,
        SourceReaderLimits {
            max_files: 100_000,
            max_bytes: 1024 * 1024 * 1024,
        },
    )
    .unwrap();
    // The plan compiler's contract: the index was built over the CURRENT repo snapshot, and the
    // plan binds that same identity on both halves.
    let repo = reader.current_snapshot();
    let provider = provider_over(session, repo.as_str(), "fixture");
    let request = request_for(repo.as_str(), repo.as_str());

    let receipt = retrieve_coverage(&provider, &reader, &request)
        .expect("the composed producer satisfies the consumer's whole validation");

    let record = receipt.broker_record();
    let executable = record
        .verified_executable
        .as_ref()
        .expect("a named program");
    assert!(
        executable
            .path
            .ends_with(&format!("fake_mcp_server{}", std::env::consts::EXE_SUFFIX))
    );
    // #153 item 3 asks for "which binary (path AND hash)", and the path alone is half of it.
    //
    // The digest is taken OF THE FILE THE RECORD NAMES -- not of a path this test knows by another
    // route -- so the two fields are proved against EACH OTHER. That is what makes them one
    // identity instead of two loose facts: a `sha256` populated from the wrong source would still
    // be a well-formed digest, and nothing here would notice.
    //
    // Measured before this line existed: with `provider.rs` populating `sha256` from unrelated
    // bytes, this whole suite stayed GREEN (3 passed). The blindness was real, not hypothetical.
    let named = std::path::Path::new(&executable.path);
    let bytes = std::fs::read(named).unwrap_or_else(|error| {
        panic!(
            "the record names {}, which cannot be read: {error}",
            executable.path
        )
    });
    assert_eq!(
        executable.sha256,
        graphhelm_tool_broker::record::digest_hex(&bytes),
        "a path carrying another binary's hash names one thing and identifies another"
    );
    let session_identity = record.contained_session.as_ref().expect("a named session");
    assert_eq!(
        session_identity.snapshot_generation, session_generation,
        "the record's session names the PINNED COPY it served -- provenance, distinct from the \
         semantic generation the receipt binds"
    );
}

/// The bytes the provider READS are the bytes the broker PINNED. The real provider (measured:
/// codebase-memory-mcp 0.10.8) reads its store from `CBM_CACHE_DIR` — the directory #538's
/// funnel confines — and ignores every other name; a pin that lives anywhere else is a pin no
/// provider ever consults. The `echo-cache` mode reports what actually sits at that address, and
/// this cell demands it be the pinned snapshot's content: fail here means the containment chain
/// pins one tree and serves another (an EMPTY one), which is exactly the silent hole #219's
/// composition exists to close.
#[test]
fn the_cache_dir_the_provider_reads_serves_the_pinned_bytes() {
    let (_project, _staging, session, _workspace, _source) = composed("producer-d");

    let stdin = concat!(
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#,
        "\n",
        r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"search_graph","arguments":{}}}"#,
        "\n",
    );
    let captured = session
        .call(
            &["echo-cache".to_owned()],
            Some(stdin.as_bytes()),
            &process_limits(),
            // #180: the same declared limit as the shipped provider call site. This session is
            // opened here, not by a ToolHost, so there is no host-scoped signal to share -- its
            // bound stays the deadline.
            None,
        )
        .expect("the echo-cache call completes");

    let reply = String::from_utf8_lossy(&captured.stdout);
    assert!(
        reply.contains("index bytes v1"),
        "the provider's CBM_CACHE_DIR does not serve the pinned snapshot's bytes; it answered: {reply}"
    );
}

/// A provider that answers garbage is UNAVAILABLE -- a typed refusal, never a partial answer.
#[test]
fn a_garbage_provider_is_unavailable_not_partially_believed() {
    let (_project, _staging, session, _workspace, _source) = composed("producer-b");
    let provider = provider_over(session, "sha256-x", "garbage");

    let refused = provider
        .retrieve(&request_for("sha256-x", "sha256-x"))
        .expect_err("bytes that parse as nothing were believed");

    assert!(matches!(
        refused,
        graphhelm_runtime::ports::StructuralCodeIndexError::Unavailable
    ));
}

/// The staleness composition: a plan bound to a DIFFERENT index generation refuses index_stale
/// at the consumer -- the session's pinned generation is what the producer reports, so the
/// comparison has real teeth on both sides.
#[test]
fn a_plan_over_another_generation_refuses_index_stale() {
    use graphhelm_runtime::retrieval::RetrievalReceiptError;

    let (_project, _staging, session, _workspace, project) = composed("producer-c");
    let reader = WorkspaceSourceReader::open(
        &project,
        SourceReaderLimits {
            max_files: 100_000,
            max_bytes: 1024 * 1024 * 1024,
        },
    )
    .unwrap();
    let repo = reader.current_snapshot();
    // The index claims to be built over ANOTHER repo snapshot than the plan (and the reader).
    let provider = provider_over(session, "sha256-some-other-generation", "fixture");
    let request = request_for(repo.as_str(), repo.as_str());

    let refused = retrieve_coverage(&provider, &reader, &request)
        .expect_err("a plan over another generation was served anyway");

    assert!(matches!(refused, RetrievalReceiptError::IndexStale));
}

/// Codex P1 on #579: the provider has TWO valid encodings and the producer read only one.
///
/// `search_graph` answers either a flat `rows` list or a GROUPED page, where a shared
/// `(qn_prefix, file)` header is printed once and the top-level `rows` is left EMPTY. The decoder
/// supports both — `search_graph_grouped.json` is checked in and exercises it — while the
/// producer iterated `page.rows()` alone. Every grouped answer therefore produced ZERO hits, and
/// zero hits is a legal answer that means something completely different: it reaches
/// `compile_plan` as a negative claim about the repository rather than as a decoding failure.
///
/// The subject is the SHIPPED fixture, so this cell moves the moment the provider's real encoding
/// does.
#[test]
fn a_grouped_page_yields_its_files_rather_than_nothing() {
    let fixture = include_str!("fixtures/search_graph_grouped.json");
    let value: serde_json::Value = serde_json::from_str(fixture).expect("the fixture parses");
    let page = graphhelm_codebase_memory_mcp::decode_search_graph(
        &serde_json::to_vec(&value).unwrap(),
        DecodeLimits::default(),
    )
    .expect("the decoder accepts the grouped encoding it ships a fixture for");

    assert!(
        page.rows().is_empty(),
        "arrangement: a grouped page leaves the flat rows empty, which is why reading only rows \
         lost everything"
    );
    let files: Vec<&str> = page.groups().iter().map(|group| group.file()).collect();
    assert_eq!(
        files,
        vec!["core/tool-broker/src/record.rs"],
        "the group header carries the path; that is what it exists for"
    );
}
