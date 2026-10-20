//! Read a current task claim and its lease before a host starts the bound worker.
use serde::Deserialize;
use serde_json::{Value, json};

use super::mcp::{client::ApiClient, url};
use crate::output::Outcome;
use graphhelm_protocols::Diagnostic;

const COMMAND: &str = "tasks.admit";

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Request {
    pub execution_id: String,
    pub lane: String,
    pub issue: u64,
    pub branch: String,
    pub repo: String,
    pub claim_sequence: u64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Snapshot {
    execution_id: String,
    head_sequence: u64,
    tasks: Vec<Task>,
    leases: Vec<Lease>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Task {
    key: String,
    task_id: String,
    issue: Option<u64>,
    lane: Option<String>,
    branch: Option<String>,
    repo_url: Option<String>,
    claim_sequence: Option<u64>,
    released_by: Option<String>,
    #[serde(default)]
    orphaned: bool,
    step: String,
    pr: Option<u64>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Lease {
    key: String,
    task_id: String,
    lane: String,
    claim_sequence: u64,
    overdue: bool,
}

fn refused(code: &'static str, pointer: &str, message: &str) -> Outcome {
    Outcome::application(
        COMMAND,
        Diagnostic::error(code, message, pointer, "task-admission"),
    )
}

fn identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"._:-".contains(&byte))
}

impl Request {
    fn validate(&self) -> Result<(), &'static str> {
        if !identifier(&self.execution_id) {
            return Err("/executionId");
        }
        if !identifier(&self.lane) {
            return Err("/lane");
        }
        if self.issue == 0 {
            return Err("/issue");
        }
        if self.claim_sequence == 0 {
            return Err("/claimSequence");
        }
        if self.branch.is_empty()
            || self.branch.len() > 1024
            || self.branch.chars().any(char::is_control)
        {
            return Err("/branch");
        }
        let parts: Vec<_> = self.repo.split('/').collect();
        if parts.len() != 2
            || parts.iter().any(|part| {
                part.is_empty()
                    || part.len() > 100
                    || matches!(*part, "." | "..")
                    || !part
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte))
            })
        {
            return Err("/repo");
        }
        Ok(())
    }
}

/// One filtered read, using the process's existing read credential. Never signs or writes.
pub(crate) fn admit(api: &ApiClient, request: &Request) -> Outcome {
    if let Err(pointer) = request.validate() {
        return refused(
            crate::error_codes::GHCLI001_ARGUMENT_INVALID,
            pointer,
            "supply an explicit execution, lane, positive issue and claim sequence, branch, and owner/repository",
        );
    }
    let path = format!(
        "{}?lane={}&issue={}",
        url::segment_path(&["v1", "executions", &request.execution_id, "tasks"]),
        url::encode_component(&request.lane),
        request.issue
    );
    match api.request("GET", &path, None, None, None) {
        Ok((200, reply)) if reply["ok"] == true => evaluate(request, &reply["data"]),
        Ok((401 | 403, _)) => refused(
            crate::error_codes::GHCLI007_SERVE_UNAUTHORIZED,
            "/claimSequence",
            "the Runtime refused the task read; bind an existing read credential and confirm the lane's own claim before starting the worker",
        ),
        Ok(_) => refused(
            crate::error_codes::GHCLI016_DRIVER_FAILURE,
            "/executionId",
            "the Runtime did not return a successful tasks snapshot; restore the Runtime task read before starting the worker",
        ),
        Err(_) => refused(
            crate::error_codes::GHCLI016_DRIVER_FAILURE,
            "/executionId",
            "the Runtime task read is unavailable; restore the Runtime binding before starting the worker",
        ),
    }
}

fn evaluate(request: &Request, data: &Value) -> Outcome {
    let fail = || {
        refused(
            crate::error_codes::GHCLI041_TASK_ADMISSION_REFUSED,
            "/claimSequence",
            "no exact current own claim with a live lease matches this worker binding; record and confirm that lane's claim before starting the worker",
        )
    };
    let Ok(snapshot) = Snapshot::deserialize(data) else {
        return fail();
    };
    if snapshot.execution_id != request.execution_id
        || snapshot.head_sequence < request.claim_sequence
    {
        return fail();
    }
    let task_id = format!("issue-{}", request.issue);
    let repo_url = format!("https://github.com/{}", request.repo);
    let mut candidates = snapshot.tasks.iter().filter(|task| {
        task.task_id == task_id && task.claim_sequence == Some(request.claim_sequence)
    });
    let Some(task) = candidates.next() else {
        return fail();
    };
    if candidates.next().is_some()
        || task.key.is_empty()
        || task.issue != Some(request.issue)
        || task.lane.as_deref() != Some(&request.lane)
        || task.branch.as_deref() != Some(&request.branch)
        || task.repo_url.as_deref() != Some(&repo_url)
        || task.claim_sequence != Some(request.claim_sequence)
        || task.released_by.is_some()
        || task.orphaned
        || task.pr.is_some()
        || !matches!(task.step.as_str(), "plan" | "critic" | "implement")
    {
        return fail();
    }
    let mut leases = snapshot.leases.iter().filter(|lease| {
        lease.task_id == task_id
            && (lease.claim_sequence == request.claim_sequence || !lease.overdue)
    });
    let Some(lease) = leases.next() else {
        return fail();
    };
    if leases.next().is_some()
        || lease.key != task.key
        || lease.lane != request.lane
        || lease.claim_sequence != request.claim_sequence
        || lease.overdue
    {
        return fail();
    }
    Outcome::success(
        COMMAND,
        json!({
            "admitted": true, "executionId": request.execution_id, "lane": request.lane,
            "issue": request.issue, "branch": request.branch, "repo": request.repo,
            "claimSequence": request.claim_sequence, "headSequence": snapshot.head_sequence,
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request() -> Request {
        Request {
            execution_id: "team".into(),
            lane: "worker".into(),
            issue: 1739,
            branch: "issue-1739".into(),
            repo: "owner/repo".into(),
            claim_sequence: 42,
        }
    }

    fn snapshot() -> Value {
        json!({"executionId":"team", "headSequence":43,
            "tasks":[{"key":"issue-1739#claim-0", "taskId":"issue-1739", "issue":1739,
                "lane":"worker", "branch":"issue-1739", "repoUrl":"https://github.com/owner/repo",
                "claimSequence":42, "step":"implement", "pr":null}],
            "leases":[{"key":"issue-1739#claim-0", "taskId":"issue-1739", "lane":"worker",
                "claimSequence":42, "overdue":false}]})
    }

    #[test]
    fn exact_current_claim_is_a_read_receipt_not_worker_activity() {
        let outcome = evaluate(&request(), &snapshot());
        assert_eq!(outcome.exit_code, 0);
        let data = outcome.output.data.unwrap();
        assert_eq!(data["admitted"], true);
        assert_eq!(data["claimSequence"], 42);
        assert!(data.get("working").is_none());
    }

    #[test]
    fn changed_or_missing_bindings_and_leases_refuse_admission() {
        for (pointer, replacement) in [
            ("/executionId", json!("other")),
            ("/headSequence", json!(41)),
            ("/tasks/0/lane", json!("sibling")),
            ("/tasks/0/issue", json!(1)),
            ("/tasks/0/branch", json!("other")),
            ("/tasks/0/repoUrl", json!("https://github.com/other/repo")),
            ("/tasks/0/claimSequence", json!(41)),
            ("/tasks/0/releasedBy", json!("worker")),
            ("/tasks/0/orphaned", json!(true)),
            ("/tasks/0/pr", json!(5)),
            ("/tasks/0/step", json!("merged")),
            ("/leases/0/overdue", json!(true)),
            ("/leases/0/key", json!("other")),
            ("/leases/0/lane", json!("sibling")),
            ("/leases/0/claimSequence", json!(41)),
            ("/tasks", json!([])),
            ("/leases", json!([])),
        ] {
            let mut data = snapshot();
            if pointer == "/tasks/0/releasedBy" || pointer == "/tasks/0/orphaned" {
                data["tasks"][0][pointer.rsplit('/').next().unwrap()] = replacement;
            } else {
                *data.pointer_mut(pointer).unwrap() = replacement;
            }
            let outcome = evaluate(&request(), &data);
            assert_ne!(outcome.exit_code, 0, "{pointer}");
            assert!(outcome.output.data.is_none(), "{pointer}");
            assert_eq!(
                outcome.output.diagnostics[0].code,
                "GHCLI041_TASK_ADMISSION_REFUSED"
            );
        }
    }

    #[test]
    fn ambiguous_or_malformed_snapshot_cannot_admit() {
        for field in ["tasks", "leases"] {
            let mut data = snapshot();
            let duplicate = data[field][0].clone();
            data[field].as_array_mut().unwrap().push(duplicate);
            assert_ne!(evaluate(&request(), &data).exit_code, 0);
        }
        let mut data = snapshot();
        data["leases"][0].as_object_mut().unwrap().remove("overdue");
        assert_ne!(evaluate(&request(), &data).exit_code, 0);
    }

    #[test]
    fn released_history_does_not_hide_a_fresh_claim_at_n_and_ten_n() {
        for count in [1_000, 10_000] {
            let mut data = snapshot();
            let mut historical = data["tasks"][0].clone();
            historical["claimSequence"] = json!(1);
            historical["key"] = json!("old-claim");
            historical["releasedBy"] = json!("worker");
            for _ in 0..count {
                data["tasks"]
                    .as_array_mut()
                    .unwrap()
                    .push(historical.clone());
            }
            let started = std::time::Instant::now();
            let outcome = evaluate(&request(), &data);
            eprintln!(
                "admission historical rows={count} elapsed={:?}",
                started.elapsed()
            );
            assert_eq!(outcome.exit_code, 0, "released history count {count}");
        }
    }

    #[test]
    fn another_live_claim_lease_refuses_but_expired_history_does_not() {
        let mut data = snapshot();
        let mut lease = data["leases"][0].clone();
        lease["claimSequence"] = json!(41);
        lease["key"] = json!("older-claim");
        data["leases"].as_array_mut().unwrap().push(lease);
        assert_ne!(evaluate(&request(), &data).exit_code, 0);
        data["leases"][1]["overdue"] = json!(true);
        assert_eq!(evaluate(&request(), &data).exit_code, 0);
    }

    #[test]
    fn identifiers_and_positive_claim_sequence_are_required() {
        let mut request = request();
        request.claim_sequence = 0;
        assert_eq!(request.validate(), Err("/claimSequence"));
        request.claim_sequence = 42;
        request.repo = "owner/repo/extra".into();
        assert_eq!(request.validate(), Err("/repo"));
        request.repo = "owner/repo".into();
        request.lane = "../other".into();
        assert_eq!(request.validate(), Err("/lane"));
    }
}
