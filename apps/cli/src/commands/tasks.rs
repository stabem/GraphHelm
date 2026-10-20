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
    let token_path = path;
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
    let result = client.request_waiting(
        "POST",
        &path,
        Some(&body),
        Some(&key),
        None,
        Duration::from_secs(5),
    );
    match result {
        Ok((status, reply)) if (200..300).contains(&status) && reply["ok"] == true => {
            let released = release_open_claims(&client, &token_path, execution, lane, exit);
            Outcome::success(
                command,
                serde_json::json!({"recorded": true, "signalId": key, "claims": released}),
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

/// #1240: a one-shot run that ended still holding a pre-PR claim will never open its PR, so the
/// lane releases that claim now instead of leaving the issue held until the lease lapses (or, on
/// an orchestrator that counts only repeated failures, forever). Best effort: each claim that
/// cannot be released is reported, never fatal, and the run's own exit code is untouched.
fn release_open_claims(
    client: &ApiClient,
    token_path: &Path,
    execution: &str,
    lane: &str,
    exit: Option<i32>,
) -> Value {
    let mut released = Vec::new();
    let mut refused = Vec::new();
    let Ok((200, view)) = client.request_waiting(
        "GET",
        &format!("/v1/executions/{execution}/tasks"),
        None,
        None,
        None,
        Duration::from_secs(5),
    ) else {
        return serde_json::json!({"released": released, "refused": ["tasks route unavailable"]});
    };
    let data = &view["data"];
    let empty = Vec::new();
    let tasks = data["tasks"].as_array().unwrap_or(&empty);
    // The lane's own credential when one was minted (#1148), else the token the run was given.
    let credential = super::secret_file::read_existing(
        &super::secret_file::lane_credential_beside(token_path, lane),
        "lane credential",
    )
    .or_else(|_| super::secret_file::read_existing(token_path, "agent token"));
    let Ok(credential) = credential else {
        return serde_json::json!({"released": released, "refused": ["no credential"]});
    };
    let writer = ApiClient::new(
        client.base_url.clone(),
        TokenSource::Fixed(Zeroizing::new(credential)),
        lane.to_owned(),
        "agent".to_owned(),
        None,
        None,
        format!("run-ended-release-{lane}"),
    );
    for lease in data["leases"].as_array().unwrap_or(&empty) {
        if lease["lane"] != lane {
            continue;
        }
        let (Some(task_id), Some(sequence)) =
            (lease["taskId"].as_str(), lease["claimSequence"].as_u64())
        else {
            continue;
        };
        let issue = task_id.strip_prefix("issue-").unwrap_or(task_id);
        let repo_url = tasks
            .iter()
            .find(|task| task["key"] == lease["key"])
            .and_then(|task| task["repoUrl"].as_str());
        let Some(repo_url) = repo_url else {
            refused.push(serde_json::json!({"taskId": task_id, "message": "claim names no repo"}));
            continue;
        };
        let reason = format!(
            "run ended (exit {}) without a PR",
            exit.map_or("unknown".to_owned(), |code| code.to_string())
        );
        let document = serde_json::json!({
            "schema": "graphhelm-task-event-v1", "taskId": task_id, "revision": 2,
            "at": Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string(),
            "lane": lane, "claimSequence": sequence, "reason": reason,
        });
        let digest = Sha256::digest(format!("{lane}\n{task_id}\n{sequence}").as_bytes());
        let mut id = format!("{lane}-{task_id}-released-{}", hex::encode(&digest[..8]));
        if id.len() > 64 {
            id = format!("{}-{}", &id[..47], hex::encode(&digest[..8]));
        }
        let body = serde_json::json!({"signal": {
            "id": id, "type": "task.released", "source": {"type": "user", "id": lane},
            "severity": "low", "emittedAt": Utc::now().to_rfc3339(),
            "evidence": [format!("{repo_url}/issues/{issue}")],
            "description": document.to_string(),
        }});
        match writer.request_waiting(
            "POST",
            &format!("/v1/executions/{execution}/signal"),
            Some(&body),
            Some(&id),
            None,
            Duration::from_secs(5),
        ) {
            Ok((status, reply)) if (200..300).contains(&status) && reply["ok"] == true => {
                released.push(task_id.to_owned());
            }
            _ => refused.push(
                serde_json::json!({"taskId": task_id, "message": "Runtime refused task.released"}),
            ),
        }
    }
    serde_json::json!({"released": released, "refused": refused})
}

use graphhelm_protocols::Diagnostic;
use serde_json::Value;

/// `graphhelm tasks durations` (#1516): the `durations` block of the Runtime's tasks route.
pub(super) fn durations(
    execution: &str,
    since: Option<&str>,
    url: Option<&str>,
    token_file: Option<&Path>,
) -> Outcome {
    const COMMAND: &str = "tasks.durations";
    let refuse = |message: &str, pointer: &str| {
        Outcome::application(
            COMMAND,
            Diagnostic::error(
                crate::error_codes::GHCLI001_ARGUMENT_INVALID,
                message,
                pointer,
                "tasks-cli",
            ),
        )
    };
    let url = url
        .map(str::to_owned)
        .or_else(|| std::env::var("GRAPHHELM_RUNTIME_URL").ok())
        .unwrap_or_else(|| "http://127.0.0.1:8793".to_owned());
    if !is_loopback_url(&url) {
        return refuse("--url must be a loopback Runtime address", "/url");
    }
    if execution.is_empty()
        || execution.len() > 128
        || !execution
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"._:-".contains(&c))
    {
        return refuse(
            "--execution must be 1 to 128 characters of [A-Za-z0-9._:-]",
            "/execution",
        );
    }
    let mut path = format!("/v1/executions/{execution}/tasks");
    if let Some(since) = since {
        // The Runtime parses the same RFC 3339 instant; refusing here keeps the query unencoded.
        if graphhelm_protocols::PersistedTimestamp::parse(since).is_err()
            || !since
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"-:.".contains(&c))
        {
            return refuse(
                "--since must be a UTC RFC 3339 instant such as 2026-10-15T00:00:00Z",
                "/since",
            );
        }
        path.push_str(&format!("?since={since}"));
    }
    let token_file = token_file
        .map(Path::to_path_buf)
        .or_else(|| std::env::var_os("GRAPHHELM_TOKEN_FILE").map(Into::into));
    let Some(token_file) = token_file else {
        return refuse(
            "--token-file or GRAPHHELM_TOKEN_FILE is required",
            "/tokenFile",
        );
    };
    let token = match super::secret_file::read_existing(&token_file, "bearer token") {
        Ok(token) => token,
        Err(error) => return refuse(error.message(), "/tokenFile"),
    };
    let client = ApiClient::new(
        url,
        TokenSource::Fixed(Zeroizing::new(token)),
        "graphhelm-cli".to_owned(),
        "tool".to_owned(),
        None,
        None,
        "tasks-durations".to_owned(),
    );
    match client.request_waiting("GET", &path, None, None, None, Duration::from_secs(60)) {
        Ok((200, reply)) if reply["ok"] == true => {
            Outcome::success(COMMAND, reply["data"]["durations"].clone())
        }
        Ok((_, reply)) => Outcome::application(
            COMMAND,
            Diagnostic::error(
                crate::error_codes::GHCLI001_ARGUMENT_INVALID,
                reply["diagnostics"][0]["message"]
                    .as_str()
                    .unwrap_or("the Runtime refused the tasks read"),
                "/",
                "tasks-cli",
            ),
        ),
        Err(_) => refuse("the Runtime is unavailable", "/url"),
    }
}

use crate::args::{TasksArgs, TasksCommand};

const COMMAND: &str = "tasks.reconcile";
const SCRIPT: &str = include_str!("../../../../plugins/graphhelm/hooks/tasks_reconcile.py");

pub(super) fn run(args: &TasksArgs) -> Outcome {
    if let TasksCommand::Admit {
        execution,
        lane,
        issue,
        branch,
        repo,
        claim_sequence,
        url,
        token_file,
    } = &args.command
    {
        let request = super::task_admission::Request {
            execution_id: execution.clone(),
            lane: lane.clone(),
            issue: *issue,
            branch: branch.clone(),
            repo: repo.clone(),
            claim_sequence: *claim_sequence,
        };
        let url = url
            .clone()
            .or_else(|| std::env::var("GRAPHHELM_RUNTIME_URL").ok())
            .unwrap_or_else(|| "http://127.0.0.1:8793".to_owned());
        let refuse = |message: &str, pointer: &str| {
            Outcome::application(
                "tasks.admit",
                Diagnostic::error(
                    crate::error_codes::GHCLI001_ARGUMENT_INVALID,
                    message,
                    pointer,
                    "task-admission",
                ),
            )
        };
        if !is_loopback_url(&url) {
            return refuse("--url must be a loopback Runtime address", "/url");
        }
        let token_file = token_file
            .clone()
            .or_else(|| std::env::var_os("GRAPHHELM_TOKEN_FILE").map(Into::into));
        let Some(token_file) = token_file else {
            return refuse(
                "--token-file or GRAPHHELM_TOKEN_FILE is required for the task read",
                "/tokenFile",
            );
        };
        let token = match super::secret_file::read_existing(&token_file, "read token") {
            Ok(token) => token,
            Err(_) => {
                return refuse(
                    "the read credential is unavailable; restore the Runtime binding before starting the worker",
                    "/tokenFile",
                );
            }
        };
        let api = ApiClient::new(
            url,
            TokenSource::Fixed(Zeroizing::new(token)),
            "graphhelm-cli".to_owned(),
            "agent".to_owned(),
            None,
            None,
            "tasks-admit".to_owned(),
        );
        return super::task_admission::admit(&api, &request);
    }
    if let TasksCommand::Credential {
        lane,
        execution,
        token_file,
        out,
    } = &args.command
    {
        return credential(lane, execution, token_file, out.as_deref());
    }
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

/// `graphhelm tasks credential` (#1148): derives the lane's credential from the OWNER token and
/// writes it owner-only. The lane and execution take the same bounded identifier alphabet the
/// Runtime's actor id and path segment accept, so a value that would never verify is refused here.
fn credential(lane: &str, execution: &str, token_file: &Path, out: Option<&Path>) -> Outcome {
    const COMMAND: &str = "tasks.credential";
    let refuse = |message: &str, pointer: &str| {
        Outcome::application(
            COMMAND,
            Diagnostic::error(
                crate::error_codes::GHCLI001_ARGUMENT_INVALID,
                message,
                pointer,
                "tasks-cli",
            ),
        )
    };
    let identifier = |value: &str| {
        !value.is_empty()
            && value.len() <= 128
            && value
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"._:-".contains(&c))
    };
    if !identifier(lane) {
        return refuse(
            "--lane must be 1 to 128 characters of [A-Za-z0-9._:-]",
            "/lane",
        );
    }
    if !identifier(execution) {
        return refuse(
            "--execution must be 1 to 128 characters of [A-Za-z0-9._:-]",
            "/execution",
        );
    }
    let owner_token = match super::secret_file::read_existing(token_file, "bearer token") {
        Ok(token) => Zeroizing::new(token),
        Err(error) => return refuse(error.message(), "/tokenFile"),
    };
    let value = Zeroizing::new(super::secret_file::lane_credential(
        &owner_token,
        execution,
        lane,
    ));
    let path = out.map_or_else(
        || super::secret_file::lane_credential_beside(token_file, lane),
        Path::to_path_buf,
    );
    match super::secret_file::write_derived(&path, &value, "lane credential") {
        Ok(provision) => Outcome::success(
            COMMAND,
            serde_json::json!({
                "lane": lane,
                "execution": execution,
                "path": path.to_string_lossy(),
                "provision": match provision {
                    super::secret_file::Provision::Created => "created",
                    super::secret_file::Provision::Existing => "existing",
                },
            }),
        ),
        Err(error) => refuse(error.message(), "/out"),
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
