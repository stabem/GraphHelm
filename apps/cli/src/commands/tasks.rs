//! Best-effort process-boundary record. The caller's exit status belongs to the agent run.
use std::path::Path;
use std::time::Duration;

use chrono::Utc;
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

use super::mcp::client::{ApiClient, TokenSource, is_loopback_url};
use crate::output::Outcome;

pub(crate) fn run_ended(
    execution: &str,
    lane: &str,
    exit: Option<i32>,
    result_file: Option<&Path>,
    run_id: Option<&str>,
    url: Option<&str>,
    token_file: Option<&Path>,
) -> Outcome {
    let command = "tasks.run-ended";
    let url = url
        .map(str::to_owned)
        .or_else(|| std::env::var("GRAPHHELM_RUNTIME_URL").ok())
        .unwrap_or_else(|| "http://127.0.0.1:8793".to_owned());
    let token_file = token_file
        .map(Path::to_path_buf)
        .or_else(|| std::env::var_os("GRAPHHELM_TOKEN_FILE").map(Into::into));
    let identity = run_id
        .map(str::to_owned)
        .or_else(|| std::env::var("GRAPHHELM_RUN_ID").ok())
        .or_else(|| result_file.map(|path| path.to_string_lossy().into_owned()))
        .unwrap_or_else(|| "default".to_owned());
    let digest = Sha256::digest(format!("{execution}\0{lane}\0{identity}\0{exit:?}").as_bytes());
    let key = format!("run-ended-{}", hex::encode(&digest[..20]));
    let unavailable = |reason: &str| {
        Outcome::success(
            command,
            serde_json::json!({
                "recorded": false, "reason": reason, "signalId": key,
            }),
        )
    };
    if lane.is_empty()
        || lane.len() > 128
        || !lane
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"._:-".contains(&c))
        || execution.is_empty()
        || execution.len() > 128
        || !execution
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"._:-".contains(&c))
        || !is_loopback_url(&url)
    {
        return unavailable("invalid lane, execution or Runtime URL");
    }
    let Some(path) = token_file else {
        return unavailable("agent token file is missing");
    };
    let Ok(token) = super::secret_file::read_existing(&path, "agent token") else {
        return unavailable("agent token file is unavailable");
    };
    let client = ApiClient::new(
        url,
        TokenSource::Fixed(Zeroizing::new(token)),
        lane.to_owned(),
        "agent".to_owned(),
        None,
        None,
        key.clone(),
    );
    let body = serde_json::json!({"signal": {
        "id": key, "source": {"type": "tool", "id": lane}, "type": "agent_session_ended",
        "severity": "low", "description": format!("{lane} run ended (exit {}).", exit.map_or("unknown".to_owned(), |code| code.to_string())),
        "evidence": [execution], "emittedAt": Utc::now().to_rfc3339(),
    }});
    let path = format!("/v1/executions/{execution}/signal");
    match client.request_waiting(
        "POST",
        &path,
        Some(&body),
        Some(&key),
        None,
        Duration::from_secs(5),
    ) {
        Ok((status, reply)) if (200..300).contains(&status) && reply["ok"] == true => {
            Outcome::success(
                command,
                serde_json::json!({"recorded": true, "signalId": key}),
            )
        }
        Ok((_, reply))
            if reply["diagnostics"].as_array().is_some_and(|diagnostics| {
                diagnostics
                    .iter()
                    .any(|diagnostic| diagnostic["code"] == "GHE003_IDEMPOTENCY_CONFLICT")
            }) =>
        {
            Outcome::success(
                command,
                serde_json::json!({"recorded": true, "alreadyRecorded": true, "signalId": key}),
            )
        }
        Ok(_) => unavailable("Runtime refused session end"),
        Err(_) => unavailable("Runtime unavailable"),
    }
}

use graphhelm_protocols::Diagnostic;
use serde_json::Value;

use crate::args::{TasksArgs, TasksCommand};

const COMMAND: &str = "tasks.reconcile";
const SCRIPT: &str = include_str!("../../../../plugins/graphhelm/hooks/tasks_reconcile.py");

pub(super) fn run(args: &TasksArgs) -> Outcome {
    let TasksCommand::Reconcile {
        execution,
        url,
        token_file,
        lane,
        repo,
    } = &args.command
    else {
        return failure("unsupported tasks command");
    };
    let python = if cfg!(windows) { "python" } else { "python3" };
    let mut child = graphhelm_process::command(python);
    child
        .arg("-c")
        .arg(SCRIPT)
        .arg("--execution")
        .arg(execution)
        .arg("--url")
        .arg(url)
        .arg("--token-file")
        .arg(token_file)
        .arg("--repo")
        .arg(repo);
    if let Some(lane) = lane {
        child.arg("--lane").arg(lane);
    }
    match child.output() {
        Ok(output) if output.status.success() => {
            match serde_json::from_slice::<Value>(&output.stdout) {
                Ok(data) => Outcome::success(COMMAND, data),
                Err(_) => failure("reconciler returned invalid JSON"),
            }
        }
        Ok(_) => failure("reconciler failed; check the Runtime binding and agent token"),
        Err(_) => failure("could not start Python reconciler"),
    }
}

fn failure(message: impl AsRef<str>) -> Outcome {
    Outcome::application(
        COMMAND,
        Diagnostic::error(
            crate::error_codes::GHCLI001_ARGUMENT_INVALID,
            message.as_ref(),
            "/tasks/reconcile",
            "tasks-cli",
        ),
    )
}
