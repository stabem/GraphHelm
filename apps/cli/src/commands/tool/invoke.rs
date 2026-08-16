//! `graphhelm tool invoke` — one brokered tool call, end to end, with the record in the reply
//! and the stream bytes on the operator's disk.

use std::collections::BTreeSet;
use std::io::Read as _;
use std::path::{Path, PathBuf};

use graphhelm_protocols::Diagnostic;
use graphhelm_tool_broker::call::ToolCall;
use graphhelm_tool_broker::lease::{Capability, ToolLease};
use graphhelm_tool_broker::record::ToolDisposition;
use graphhelm_tool_host::host::{HostConfig, ToolHost};
use graphhelm_tool_host::process::ProcessLimits;
use graphhelm_tool_host::workspace::WorkspaceConfig;

use super::{DENIED_CODE, HOST_CODE, INVALID_CODE};
use crate::output::Outcome;

const COMMAND: &str = "tool invoke";
const SOURCE: &str = "tool-cli";
/// A tool request is small by construction; anything larger is refused unread (the
/// `read_bounded_manifest` metadata-first pattern).
const MAX_REQUEST_BYTES: usize = 1024 * 1024;
/// Fixed operator-side execution limits for this milestone: the CLI is the only caller shaping
/// them, and a flag per knob would be configuration surface without a consumer yet (05d's node
/// contract is where per-call limits arrive).
const TIMEOUT_SECONDS: u64 = 300;
const MAX_OUTPUT_BYTES: usize = 8 * 1024 * 1024;

pub(in crate::commands) struct InvokeArguments {
    pub project: PathBuf,
    pub staging: PathBuf,
    pub protected: Vec<PathBuf>,
    pub request: PathBuf,
    pub actor: String,
    pub capabilities: Vec<String>,
    pub allow_programs: Vec<String>,
    pub tests_runner: String,
    pub capture_out: Option<PathBuf>,
    pub keep_workspace: bool,
}

fn failure(code: &'static str, message: String, pointer: &str) -> Outcome {
    Outcome::domain(
        COMMAND,
        vec![Diagnostic::error(code, message, pointer, SOURCE)],
    )
}

/// Maps `--capability` strings onto the closed lease vocabulary; unknown names are GHCLI012
/// with the accepted set named (never the input echoed beyond the flag's own vocabulary —
/// these are operator flag values, not tool content, and naming them aids the operator).
fn parse_capability(text: &str) -> Option<Capability> {
    match text {
        "repository.read" => Some(Capability::RepositoryRead),
        "repository.write" => Some(Capability::RepositoryWrite),
        "shell.execute" => Some(Capability::ShellExecute),
        "tests.execute" => Some(Capability::TestsExecute),
        _ => None,
    }
}

fn read_bounded_request(path: &Path) -> Result<String, Outcome> {
    let metadata = std::fs::metadata(path).map_err(|_| {
        failure(
            INVALID_CODE,
            "the request file cannot be read".to_owned(),
            "/tool/request",
        )
    })?;
    if metadata.len() > MAX_REQUEST_BYTES as u64 {
        return Err(failure(
            INVALID_CODE,
            format!("the request file exceeds {MAX_REQUEST_BYTES} bytes"),
            "/tool/request",
        ));
    }
    let file = std::fs::File::open(path).map_err(|_| {
        failure(
            INVALID_CODE,
            "the request file cannot be read".to_owned(),
            "/tool/request",
        )
    })?;
    let mut text = String::new();
    // Defense in depth behind the metadata check (the gateway's read_bounded pattern).
    file.take(MAX_REQUEST_BYTES as u64 + 1)
        .read_to_string(&mut text)
        .map_err(|_| {
            failure(
                INVALID_CODE,
                "the request file is not valid UTF-8".to_owned(),
                "/tool/request",
            )
        })?;
    Ok(text)
}

pub(in crate::commands) fn run(arguments: &InvokeArguments) -> Outcome {
    // Review finding 7 closed the ambiguity: EVERY invoke captures (a Tier 0 read's file
    // content is its stdout), so --capture-out is mandatory, refused before anything runs.
    let Some(capture_out) = arguments.capture_out.as_deref() else {
        return failure(
            INVALID_CODE,
            "--capture-out is required on every invoke: captured stream bytes are operator \
             material and must have a destination"
                .to_owned(),
            "/tool/captureOut",
        );
    };

    let mut capabilities = BTreeSet::new();
    for text in &arguments.capabilities {
        match parse_capability(text) {
            Some(capability) => {
                capabilities.insert(capability);
            }
            None => {
                return failure(
                    INVALID_CODE,
                    "an unknown capability was named; accepted: repository.read, \
                     repository.write, shell.execute, tests.execute"
                        .to_owned(),
                    "/tool/capability",
                );
            }
        }
    }

    let request_text = match read_bounded_request(&arguments.request) {
        Ok(text) => text,
        Err(outcome) => return outcome,
    };
    let call = match ToolCall::from_json(&request_text) {
        Ok(call) => call,
        // CallParseError's Display names the violated rule and never request bytes.
        Err(error) => return failure(INVALID_CODE, error.to_string(), "/tool/request"),
    };

    let workspace = match WorkspaceConfig::validated(
        &arguments.project,
        &arguments.staging,
        &arguments.protected,
    ) {
        Ok(workspace) => workspace,
        // HostError's Display names the violated rule, never a path.
        Err(error) => return failure(HOST_CODE, error.to_string(), "/tool/workspace"),
    };

    let lease = ToolLease {
        actor: arguments.actor.clone(),
        capabilities,
        programs: arguments.allow_programs.iter().cloned().collect(),
    };
    let host = ToolHost::new(HostConfig {
        workspace,
        limits: ProcessLimits {
            timeout: std::time::Duration::from_secs(TIMEOUT_SECONDS),
            max_output_bytes: MAX_OUTPUT_BYTES,
        },
        tests_runner: arguments.tests_runner.clone(),
        tests_runner_env: std::collections::BTreeMap::new(),
        path_prepend: Vec::new(),
        keep_workspace: arguments.keep_workspace,
    });

    let staging_before = staging_names(&arguments.staging);
    let (record, streams) = host.invoke(&call, &lease, &arguments.actor);

    match &record.disposition {
        ToolDisposition::Denied { rule } => {
            return failure(
                DENIED_CODE,
                format!("authorize refused: {rule}"),
                "/tool/lease",
            );
        }
        ToolDisposition::HostError { code } => {
            return failure(HOST_CODE, format!("the host failed: {code}"), "/tool/host");
        }
        ToolDisposition::Completed { .. } | ToolDisposition::TimedOut => {}
    }

    let stdout_path = capture_out.join("stdout");
    let stderr_path = capture_out.join("stderr");
    if std::fs::write(&stdout_path, &streams.stdout).is_err()
        || std::fs::write(&stderr_path, &streams.stderr).is_err()
    {
        return failure(
            INVALID_CODE,
            "the captured streams could not be written to --capture-out".to_owned(),
            "/tool/captureOut",
        );
    }

    let mut data = serde_json::json!({
        "record": serde_json::to_value(&record).expect("a record serializes"),
        "capturedTo": {
            "stdout": stdout_path.display().to_string(),
            "stderr": stderr_path.display().to_string(),
        },
    });
    if arguments.keep_workspace {
        // The host does not surface the kept root; the staging delta is the honest,
        // API-free way to report it (kept trees are the operator's to delete).
        let kept: Vec<String> = staging_names(&arguments.staging)
            .into_iter()
            .filter(|name| !staging_before.contains(name))
            .collect();
        data["keptWorkspaces"] = serde_json::json!(kept);
    }
    Outcome::success(COMMAND, data)
}

fn staging_names(staging: &Path) -> Vec<String> {
    std::fs::read_dir(staging)
        .map(|entries| {
            entries
                .filter_map(Result::ok)
                .map(|entry| entry.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default()
}
