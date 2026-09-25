//! Generate the frozen retrieval artifacts through the REAL contained provider (#225 wiring).
//!
//! This replaces the hand-run BM25 queries the first freeze was built from: the selection is now
//! produced by the same composition the runtime uses — a `ContainedProviderSession` (verified
//! executable, pinned snapshot copy, provisioned Tier 1 workspace) speaking one-shot
//! newline-delimited JSON-RPC — so the frozen artifacts are what the shipped door actually
//! answers, not what an operator typed at a store.
//!
//! ONE invocation carries all cases: initialize, initialized, then one `tools/call` per case
//! (`format:"json"`, query = the case objective VERBATIM — the mechanical rule that squeezes the
//! harness's discretion out of the selection). One invocation because the provider writes into
//! its served cache copy (logs, at minimum), and the pin is re-verified per call: N invocations
//! would be N copies for no isolation gain.
//!
//! Output: `retrieval/<case>.json` per case, and the manifest rewritten with the fourth freeze
//! (`retrievalDigest`) over exactly those bytes. The tool prints the digests it froze; it never
//! prints provider output.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Duration;

use graphhelm_development_benchmark::{
    RetrievalArtifact, artifact_from_search_rows, frozen_files_digest, has_more_of, load_manifest,
    page_of,
};
use graphhelm_tool_host::process::ProcessLimits;
use graphhelm_tool_host::session::ContainedProviderSession;
use graphhelm_tool_host::snapshot::pin_snapshot;
use graphhelm_tool_host::verified::verify_executable;
use graphhelm_tool_host::workspace::{Tier1Workspace, WorkspaceConfig};

struct Arguments {
    manifest: PathBuf,
    store: PathBuf,
    exe: PathBuf,
    /// The expected digest, supplied INDEPENDENTLY by the operator.
    ///
    /// Hashing the file and handing that same hash to `verify_executable` made the pin vacuous:
    /// it can never reject anything, so a substituted provider returns plausible rows that freeze
    /// as benchmark evidence while appearing to have passed the executable-pin trust boundary
    /// (Codex P1). A pin whose expected value comes from the thing being pinned is not a pin.
    exe_sha256: String,
    staging: PathBuf,
    project: String,
    repo_snapshot: String,
    /// The revision the STORE is expected to have indexed, declared by the operator and checked
    /// against what the provider reports.
    ///
    /// `--repo-snapshot` was copied into both binding halves without ever being verified against
    /// the store, so an index built from another revision passed freshness validation and its
    /// outdated hits and line ranges were read against the current repository (Codex P1). Measured
    /// while fixing it: the shipped store reported `head_sha` 91618019 while the declared snapshot
    /// was 136440ae — not merely different values, different KINDS of identifier, and nothing
    /// compared them.
    ///
    /// Same shape as the executable pin's repair: the expected value comes from the operator, the
    /// observed value from the thing being checked, and they must agree.
    store_head_sha: String,
    /// The repository whose object database DERIVES the head-to-tree binding (#637).
    ///
    /// `--store-head-sha` binds the store to a COMMIT and `--repo-snapshot` names the TREE the
    /// artifacts will declare, and these are different kinds of identifier: nothing tied them
    /// together, so the shipped corpus froze coordinates produced against tree `15456c4d`
    /// while every artifact declared `136440ae`. Line ranges that were correct in one tree
    /// address different code in the other, silently, because both are valid line numbers.
    /// The link is DERIVED here (`git rev-parse <head>^{tree}` in this repository) and compared
    /// to the declared snapshot -- never declared on both sides, the same repair shape as the
    /// executable and store pins. An explicit path, never the process CWD: the working
    /// directory answers for whatever checkout the operator happened to stand in.
    repo: PathBuf,
    limit: u32,
}

fn parse_arguments() -> Result<Arguments, String> {
    let mut values: BTreeMap<String, String> = BTreeMap::new();
    let mut arguments = std::env::args().skip(1);
    while let Some(name) = arguments.next() {
        let value = arguments
            .next()
            .ok_or_else(|| format!("{name} needs a value"))?;
        values.insert(name, value);
    }
    let take = |name: &str| -> Result<String, String> {
        values
            .get(name)
            .cloned()
            .ok_or_else(|| format!("missing {name}"))
    };
    Ok(Arguments {
        manifest: PathBuf::from(take("--manifest")?),
        store: PathBuf::from(take("--store")?),
        exe: PathBuf::from(take("--exe")?),
        exe_sha256: take("--exe-sha256")?,
        staging: PathBuf::from(take("--staging")?),
        project: take("--project")?,
        repo_snapshot: take("--repo-snapshot")?,
        store_head_sha: take("--store-head-sha")?,
        repo: PathBuf::from(take("--repo")?),
        limit: values
            .get("--limit")
            .map_or(Ok(5), |value| value.parse::<u32>())
            .map_err(|error| format!("--limit: {error}"))?,
    })
}

fn run() -> Result<(), String> {
    let arguments = parse_arguments()?;
    let bench_root = arguments
        .manifest
        .parent()
        .ok_or("the manifest has no parent directory")?
        .to_path_buf();

    // Bounded BEFORE the read: `read_to_string` allocates the whole input before any validation
    // runs, so an oversized manifest exhausts memory before a single check applies (Codex P2).
    // The shipped corpus manifest is ~2 KiB; a megabyte is four hundred times that and still
    // refuses long before allocation matters.
    const MANIFEST_MAX_BYTES: u64 = 1024 * 1024;
    let manifest_size = std::fs::metadata(&arguments.manifest)
        .map_err(|error| error.to_string())?
        .len();
    if manifest_size > MANIFEST_MAX_BYTES {
        return Err(format!(
            "the manifest is {manifest_size} bytes, beyond the {MANIFEST_MAX_BYTES}-byte bound"
        ));
    }
    let manifest_text =
        std::fs::read_to_string(&arguments.manifest).map_err(|error| error.to_string())?;
    let manifest = load_manifest(&manifest_text).map_err(|refusal| format!("{refusal:?}"))?;

    // The containment chain, exactly the runtime's: verified binary, provisioned workspace,
    // pinned store copy, session over all three. The scratch project exists only to satisfy the
    // workspace's provisioning contract — the provider reads the pinned store, never the repo.
    // The expected digest is the OPERATOR's, never re-derived from the file: see `exe_sha256`.
    let verified = verify_executable(&arguments.exe, &arguments.exe_sha256)
        .map_err(|error| format!("verify_executable: {error}"))?;
    let exe_sha256 = arguments.exe_sha256.clone();
    let scratch_project = arguments.staging.join("scratch-project");
    std::fs::create_dir_all(&scratch_project).map_err(|error| error.to_string())?;
    std::fs::write(scratch_project.join("README.md"), b"generator scratch\n")
        .map_err(|error| error.to_string())?;
    // Provisioning goes through `git worktree add`, so the scratch project must be a committed
    // repository — same arrangement the composition cells use.
    for git_arguments in [
        &["init", "--quiet"][..],
        &["add", "-A"][..],
        &["commit", "--quiet", "-m", "scratch", "--allow-empty"][..],
    ] {
        let status = std::process::Command::new("git")
            .args(git_arguments)
            .current_dir(&scratch_project)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_AUTHOR_NAME", "generator")
            .env("GIT_AUTHOR_EMAIL", "generator@graphhelm.invalid")
            .env("GIT_COMMITTER_NAME", "generator")
            .env("GIT_COMMITTER_EMAIL", "generator@graphhelm.invalid")
            .status()
            .map_err(|error| error.to_string())?;
        if !status.success() {
            return Err(format!(
                "git {git_arguments:?} failed in the scratch project"
            ));
        }
    }
    let workspace_staging = arguments.staging.join("workspaces");
    let config = WorkspaceConfig::validated(&scratch_project, &workspace_staging, &[])
        .map_err(|error| format!("workspace config: {error}"))?;
    let workspace = Tier1Workspace::provision(&config, "generate-retrieval", None)
        .map_err(|error| format!("workspace provision: {error}"))?;
    let pinned = pin_snapshot(&arguments.store, workspace.root())
        .map_err(|error| format!("pin_snapshot: {error}"))?;
    let store_generation = pinned.generation().to_owned();
    let session = ContainedProviderSession::open(&workspace, verified, pinned);
    // The workspace has a FIXED call id and no cleanup on drop, so every run left it behind and
    // the next one refused with "the workspace directory already exists" (Codex P2). Removed
    // explicitly at the end of a successful run; a run that fails earlier still leaves it, which
    // is the deliberate half -- a failed generation's workspace is evidence to look at.

    // One invocation, all cases: ids 2..2+N map back to manifest order.
    let mut stdin = String::new();
    stdin.push_str(concat!(
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","#,
        r#""capabilities":{},"clientInfo":{"name":"graphhelm-generate-retrieval","version":"1"}}}"#,
        "\n",
        r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
        "\n",
    ));
    // The store's own account of what it indexed, asked BEFORE any case: a store built from
    // another revision must not silently supply hits that are then read against this repository.
    stdin.push_str(
        &serde_json::json!({
            "jsonrpc": "2.0", "id": 2, "method": "tools/call",
            "params": {"name": "index_status",
                        "arguments": {"project": arguments.project, "verbose": true}}
        })
        .to_string(),
    );
    stdin.push('\n');

    let mut objectives: Vec<(String, String)> = Vec::new();
    for (index, case) in manifest.cases.iter().enumerate() {
        let objective_path = bench_root
            .join("objectives")
            .join(format!("{}.json", case.id));
        // Bounded before the read, same reason as the manifest: `read_to_string` allocates the
        // whole file before any check runs, and this one is then cloned into both the
        // conversation and the objectives list (Codex P2).
        const OBJECTIVE_MAX_BYTES: u64 = 64 * 1024;
        let objective_size = std::fs::metadata(&objective_path)
            .map_err(|error| error.to_string())?
            .len();
        if objective_size > OBJECTIVE_MAX_BYTES {
            return Err(format!(
                "objective for {} is {objective_size} bytes, beyond the {OBJECTIVE_MAX_BYTES}-byte bound",
                case.id
            ));
        }
        let objective_text =
            std::fs::read_to_string(&objective_path).map_err(|error| error.to_string())?;
        let objective_value: serde_json::Value =
            serde_json::from_str(&objective_text).map_err(|error| error.to_string())?;
        let objective = objective_value["objective"]
            .as_str()
            .ok_or_else(|| format!("{}: objective is not a string", case.id))?
            .to_owned();
        let call = serde_json::json!({
            "jsonrpc": "2.0", "id": index + 3, "method": "tools/call",
            "params": {"name": "search_graph",
                        "arguments": {"project": arguments.project, "query": objective,
                                      "limit": arguments.limit, "format": "json"}}
        });
        stdin.push_str(&call.to_string());
        stdin.push('\n');
        objectives.push((case.id.clone(), objective));
    }

    let captured = session
        .call(
            &[],
            Some(stdin.as_bytes()),
            &ProcessLimits {
                timeout: Duration::from_secs(300),
                max_output_bytes: 32 * 1024 * 1024,
            },
            // #180: the same declared limit as the shipped provider call site. This generator
            // opens the session itself, so there is no host-scoped signal to share -- the
            // 300 s timeout above stays the bound.
            None,
        )
        .map_err(|error| format!("session call: {error}"))?;
    // A truncated stdout is NOT a short answer: `CapturedProcess` keeps the retained prefix and
    // says so, and if that prefix happens to carry every expected id the generator would freeze
    // partial replies as the corpus -- discarding later bytes that may correct or duplicate them
    // (Codex P2). Checked BEFORE the replies are parsed, because a parse that succeeds on a
    // prefix is the failure mode.
    if captured.truncated {
        return Err(
            "the provider's output was truncated at the capture bound; a prefix that parses \
         is not a complete answer and must not be frozen"
                .to_owned(),
        );
    }
    if captured.exit_code != Some(0) {
        return Err(format!(
            "the provider exited {:?}; stderr: {}",
            captured.exit_code,
            String::from_utf8_lossy(&captured.stderr)
        ));
    }

    // Index replies by id; provider prose on other lines is not consulted.
    let stdout = String::from_utf8_lossy(&captured.stdout);
    let mut replies: BTreeMap<u64, serde_json::Value> = BTreeMap::new();
    for line in stdout.lines() {
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(line)
            && let Some(id) = value.get("id").and_then(serde_json::Value::as_u64)
        {
            // `insert` REPLACES, so a provider answering twice for one id would have the last
            // reply silently win — a duplicate could overwrite a valid page with different rows,
            // or overwrite an error envelope with plausible data, and the survivor would freeze.
            //
            // THIRD RESTORATION of a guard of mine in this PR (after the duplicate-case-id one
            // and the frozen-file byte bound), all three lost the same way: a patch that did not
            // match, unverified, then reported as landed. The pattern is mine; leaving the note
            // is cheaper than pretending it was one accident.
            if replies.insert(id, value).is_some() {
                return Err(format!(
                    "the provider sent more than one reply for id {id}; a duplicated response \
                     cannot be frozen because nothing says which one is the answer"
                ));
            }
        }
    }

    // The store binding, CHECKED rather than copied: the operator declares which revision the
    // store should have indexed and the provider says which one it did. Disagreement refuses,
    // because the alternative is outdated hits and line ranges read against today's repository
    // while the artifact claims a fresh binding (Codex P1).
    // An error envelope can carry status-shaped JSON, and accepting its head_sha would let
    // artifacts freeze against a store whose freshness check actually FAILED. The search replies
    // below already require an explicit `isError: false`; this one did not (Codex P2) — the same
    // asymmetry, one reply earlier.
    match replies
        .get(&2)
        .and_then(|reply| reply.pointer("/result/isError"))
        .and_then(serde_json::Value::as_bool)
    {
        Some(false) => {}
        Some(true) => return Err("the provider returned a tool error for index_status".to_owned()),
        None => {
            return Err(
                "the index_status reply does not state isError, and unstated is not false"
                    .to_owned(),
            );
        }
    }
    let status = replies
        .get(&2)
        .and_then(|reply| reply.pointer("/result/content/0/text"))
        .and_then(serde_json::Value::as_str)
        .and_then(|text| serde_json::from_str::<serde_json::Value>(text).ok())
        .ok_or("the provider did not report an index status, so the store cannot be bound")?;
    let reported = status
        .pointer("/git/head_sha")
        .or_else(|| status.get("head_sha"))
        .and_then(serde_json::Value::as_str)
        .ok_or("the index status carries no head_sha, so the store cannot be bound")?;
    if reported != arguments.store_head_sha {
        return Err(format!(
            "the store reports head_sha {reported}, not the declared {}: an index built from \
             another revision would supply hits read against a repository it never saw",
            arguments.store_head_sha
        ));
    }

    // THE HEAD MUST RESOLVE TO THE DECLARED TREE (#637), derived and compared, never declared
    // on both sides. The head check above says the store indexed the commit the operator named;
    // nothing yet says that commit's TREE is the one every artifact is about to declare -- and
    // when they disagree, the coordinates are produced against one tree and sliced out of
    // another. Both checks refuse BEFORE any search reply is trusted.
    // `GIT_NO_REPLACE_OBJECTS=1`: a `refs/replace/<reported>` ref would redirect `rev-parse` to
    // ANOTHER commit's tree by default (git applies replacements silently), so a replacement
    // added after the store was indexed could make the derivation resolve to a tree that equals
    // `--repo-snapshot` while the real commit names different bytes -- recreating the exact
    // wrong-coordinate corpus this check exists to prevent, through a git mechanism rather than a
    // wrong checkout (K on #675). The derivation must read the object database as the store did:
    // unreplaced. Same family as the store/exe pins -- the instrument must answer for THIS
    // subject, not a redirected one.
    let derived = std::process::Command::new("git")
        .env("GIT_NO_REPLACE_OBJECTS", "1")
        .arg("--no-replace-objects")
        .arg("-C")
        .arg(&arguments.repo)
        .arg("rev-parse")
        .arg(format!("{reported}^{{tree}}"))
        .output()
        .map_err(|error| format!("git rev-parse could not run: {error}"))?;
    if !derived.status.success() {
        return Err(format!(
            "the store's head {reported} does not resolve in {}: without the commit, the head-to-tree binding cannot be derived, and an underivable binding refuses rather than assumes",
            arguments.repo.display()
        ));
    }
    let derived_tree = String::from_utf8_lossy(&derived.stdout).trim().to_owned();
    if derived_tree != arguments.repo_snapshot {
        return Err(format!(
            "the store's head {reported} resolves to tree {derived_tree}, not the declared {}: the coordinates about to be frozen were produced against a tree the artifacts would not name (#637)",
            arguments.repo_snapshot
        ));
    }

    let limits = graphhelm_protocols::DeclaredLimits {
        max_results: arguments.limit,
        max_pages: 8,
        max_bytes: 1_000_000,
        max_tokens: 250_000,
    };
    let retrieval_dir = bench_root.join("retrieval");
    let mut pending: Vec<(String, Vec<u8>)> = Vec::new();
    std::fs::create_dir_all(&retrieval_dir).map_err(|error| error.to_string())?;
    for (index, (case_id, objective)) in objectives.iter().enumerate() {
        let id = (index + 3) as u64;
        let reply = replies
            .get(&id)
            .ok_or_else(|| format!("{case_id}: the provider answered nothing for id {id}"))?;
        // An MCP tool-error envelope can carry page-shaped `structuredContent`, and freezing its
        // rows would make an error into benchmark evidence. This repository's own decoder already
        // refuses those envelopes (`adapters/codebase-memory-mcp/src/lib.rs:645-648`); the
        // generator was more permissive than the decoder it feeds (Codex P2). An OMITTED
        // `isError` is refused too: unstated is not false.
        match reply
            .pointer("/result/isError")
            .and_then(serde_json::Value::as_bool)
        {
            Some(false) => {}
            Some(true) => return Err(format!("{case_id}: the provider returned a tool error")),
            None => {
                return Err(format!(
                    "{case_id}: the reply does not state isError, and unstated is not false"
                ));
            }
        }
        let structured = reply
            .pointer("/result/structuredContent")
            .ok_or_else(|| format!("{case_id}: no structuredContent in the reply"))?;
        // Decoded BY SHAPE. The `filter_map` pair this replaces repaired a malformed page into a
        // plausible one: a dropped column name shifted the `file` index onto another cell, and a
        // dropped row silently shrank the hit set recall is measured from (Codex P2 pair).
        let (cols, rows) =
            page_of(structured, case_id).map_err(|refusal| format!("{refusal:?}"))?;
        // REQUIRED, and not because the artifact carries it — coverage is unconditionally
        // `partial` now, so this value decides nothing downstream. It is demanded for its own
        // reason: a provider that will not describe its own page is a provider whose answer
        // should not be frozen into the corpus at all (Codex P1-2 / P2 on #579).
        let _stated = has_more_of(structured, case_id).map_err(|refusal| format!("{refusal:?}"))?;
        let artifact: RetrievalArtifact = artifact_from_search_rows(
            case_id,
            objective,
            &arguments.repo_snapshot,
            &cols,
            &rows,
            &limits,
        )
        .map_err(|refusal| format!("{refusal:?}"))?;
        let mut bytes = serde_json::to_vec_pretty(&artifact).map_err(|error| error.to_string())?;
        bytes.push(b'\n');
        // COLLECTED, not written yet: a malformed reply for a LATE case used to surface after the
        // early artifacts had already been overwritten, leaving the previous corpus destroyed and
        // the manifest still carrying its old digest — a failed generation that corrupts the very
        // thing it failed to replace (Codex P2). Nothing lands until every case has validated.
        pending.push((case_id.clone(), bytes));
    }

    // Every case survived validation, so the corpus may now move as ONE — and it moves through a
    // staging directory rather than over the live files. Validation-before-write closed the
    // malformed-reply case; a write that fails PART WAY (a full disk) still left a mixture of old
    // and new artifacts under the old manifest digest (Codex P2). Written aside first, then moved
    // into place, so a failure during staging destroys nothing.
    let staging_dir = bench_root.join(".retrieval-staging");
    if staging_dir.exists() {
        std::fs::remove_dir_all(&staging_dir).map_err(|error| error.to_string())?;
    }
    std::fs::create_dir_all(&staging_dir).map_err(|error| error.to_string())?;
    for (case_id, bytes) in &pending {
        std::fs::write(staging_dir.join(format!("{case_id}.json")), bytes)
            .map_err(|error| error.to_string())?;
    }
    for (case_id, _) in &pending {
        let name = format!("{case_id}.json");
        let destination = retrieval_dir.join(&name);
        // `rename` FAILS on Windows when the destination exists, and REGENERATION is the case
        // where every destination exists — so the atomic-publish fix I added would have broken
        // the generator on the platform it actually runs on (Codex P1). Removed first: the
        // window this opens is between two local file operations, and the alternative (leaving
        // the old corpus in place) is the corruption the staging exists to prevent.
        if destination.exists() {
            std::fs::remove_file(&destination).map_err(|error| error.to_string())?;
        }
        std::fs::rename(staging_dir.join(&name), &destination)
            .map_err(|error| error.to_string())?;
    }
    std::fs::remove_dir_all(&staging_dir).map_err(|error| error.to_string())?;

    // The fourth freeze, over exactly the bytes just written.
    let retrieval_digest = frozen_files_digest(&bench_root, "retrieval", &manifest.cases)
        .map_err(|refusal| format!("{refusal:?}"))?;
    let mut manifest_value: serde_json::Value =
        serde_json::from_str(&manifest_text).map_err(|error| error.to_string())?;
    manifest_value["retrievalDigest"] = serde_json::json!(retrieval_digest);
    let mut manifest_bytes =
        serde_json::to_vec_pretty(&manifest_value).map_err(|error| error.to_string())?;
    manifest_bytes.push(b'\n');
    std::fs::write(&arguments.manifest, manifest_bytes).map_err(|error| error.to_string())?;

    // The workspace has a FIXED call id and no cleanup on drop, so every run left it behind and
    // the next refused with "the workspace directory already exists" (Codex P2). Removed here, at
    // the end of a SUCCESSFUL run only: a run that failed earlier keeps its workspace, because a
    // failed generation's sandbox is evidence to look at rather than litter to sweep.
    workspace
        .remove()
        .map_err(|error| format!("the generator workspace could not be removed: {error}"))?;

    println!(
        "{}",
        serde_json::json!({
            "cases": objectives.len(),
            "retrievalDigest": retrieval_digest,
            "storeGeneration": store_generation,
            "executableSha256": exe_sha256,
            "project": arguments.project,
            "repoSnapshot": arguments.repo_snapshot,
            "limit": arguments.limit,
        })
    );
    Ok(())
}

fn main() {
    if let Err(message) = run() {
        eprintln!("generate-retrieval: {message}");
        std::process::exit(2);
    }
}
