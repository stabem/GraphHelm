//! #1792 (Studio v2 R1): owner requests and what was done for them.
//!
//! A request is a `task.request_recorded` record, the ask in the owner's words, and the issues
//! linked to it by `task.request_linked`. This fold joins them with the task fold's slices by
//! issue, so a reader sees each request's issues, their PRs and steps, and one status, without
//! folding raw events itself. It is pure: it reads only the records and slices the tasks read
//! already produced.

use std::collections::BTreeMap;

use graphhelm_protocols::PersistedTimestamp;
use serde::Serialize;

use crate::tasks::{TaskEventKind, TaskEventRecord, TaskState, TaskStep};

/// Where a request stands, judged from its linked issues.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RequestStatus {
    /// No issue is linked yet.
    Open,
    /// At least one linked issue has no merged PR.
    InProgress,
    /// Every linked issue has a merged PR.
    Delivered,
}

/// One PR slice of a linked issue.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestSlice {
    pub pr: Option<u64>,
    pub step: &'static str,
    pub merged: bool,
}

/// One issue opened for a request, with the slices the task fold holds for it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestIssue {
    pub issue: u64,
    pub slices: Vec<RequestSlice>,
}

/// One owner request as the requests route serves it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestView {
    pub request_id: String,
    pub ask: String,
    pub title: Option<String>,
    pub recorded_by: String,
    /// The revision of the record whose `ask` and `title` are shown.
    pub revision: u64,
    /// The sequence of the first record of this request, so readers can order requests.
    pub sequence: u64,
    /// #1901: when the Runtime appended the first record of this request (its own clock, which a
    /// lane cannot write), so the Studio can date the request. Absent on a record read without it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recorded_at: Option<PersistedTimestamp>,
    pub status: RequestStatus,
    pub issues: Vec<RequestIssue>,
}

/// Every recorded request, oldest first. A later `task.request_recorded` for the same request
/// replaces its ask and title; a link is counted once per issue, and a link naming a request that
/// was never recorded is ignored (the Runtime refuses it at admission; this keeps an old log honest).
#[must_use]
pub fn requests(records: &[TaskEventRecord], tasks: &[TaskState]) -> Vec<RequestView> {
    let mut ordered: Vec<&TaskEventRecord> = records
        .iter()
        .filter(|record| {
            matches!(
                record.kind,
                TaskEventKind::RequestRecorded | TaskEventKind::RequestLinked
            )
        })
        .collect();
    ordered.sort_by_key(|record| record.sequence);
    let mut views: BTreeMap<&str, RequestView> = BTreeMap::new();
    let mut links: BTreeMap<&str, Vec<u64>> = BTreeMap::new();
    for record in ordered {
        let Some(request_id) = record.request_id.as_deref() else {
            continue;
        };
        match record.kind {
            TaskEventKind::RequestRecorded => {
                let ask = record.ask.clone().unwrap_or_default();
                let revision = record.revision.unwrap_or(1);
                views
                    .entry(request_id)
                    .and_modify(|view| {
                        // Only the first recorder supersedes, and only with a higher revision; any
                        // other record for this request leaves the ask as recorded.
                        if view.recorded_by == record.actor_id && revision > view.revision {
                            view.ask.clone_from(&ask);
                            view.title.clone_from(&record.title);
                            view.revision = revision;
                        }
                    })
                    .or_insert_with(|| RequestView {
                        request_id: request_id.to_owned(),
                        ask: ask.clone(),
                        title: record.title.clone(),
                        recorded_by: record.actor_id.clone(),
                        revision,
                        sequence: record.sequence,
                        recorded_at: record.occurred_at.clone(),
                        status: RequestStatus::Open,
                        issues: Vec::new(),
                    });
            }
            TaskEventKind::RequestLinked => {
                if let Some(issue) = record.issue {
                    let issues = links.entry(request_id).or_default();
                    if !issues.contains(&issue) {
                        issues.push(issue);
                    }
                }
            }
            _ => {}
        }
    }
    let mut out: Vec<RequestView> = views
        .into_iter()
        .map(|(request_id, mut view)| {
            view.issues = links
                .get(request_id)
                .into_iter()
                .flatten()
                .map(|&issue| RequestIssue {
                    issue,
                    slices: tasks
                        .iter()
                        .filter(|task| task.issue == Some(issue) && task.pr.is_some())
                        .map(|task| RequestSlice {
                            pr: task.pr,
                            step: task.step.as_str(),
                            merged: task.step == TaskStep::Merged,
                        })
                        .collect(),
                })
                .collect();
            view.status = status(&view.issues);
            view
        })
        .collect();
    out.sort_by_key(|view| view.sequence);
    out
}

fn status(issues: &[RequestIssue]) -> RequestStatus {
    if issues.is_empty() {
        RequestStatus::Open
    } else if issues
        .iter()
        .all(|issue| issue.slices.iter().any(|slice| slice.merged))
    {
        RequestStatus::Delivered
    } else {
        RequestStatus::InProgress
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tasks::{fold_task_events, parse_task_event};

    fn task(
        sequence: u64,
        actor: &str,
        kind: &str,
        task_id: &str,
        mut fields: serde_json::Value,
    ) -> TaskEventRecord {
        fields["schema"] = serde_json::json!("graphhelm-task-event-v1");
        fields["taskId"] = serde_json::json!(task_id);
        fields["revision"] = serde_json::json!(1);
        fields["at"] = serde_json::json!("2026-10-22T00:00:00Z");
        record(kind, actor, sequence, &fields)
    }

    fn record(
        kind: &str,
        actor: &str,
        sequence: u64,
        document: &serde_json::Value,
    ) -> TaskEventRecord {
        parse_task_event(kind, actor, &document.to_string(), sequence, None)
            .unwrap_or_else(|| panic!("{kind} must parse: {document}"))
    }

    fn recorded(sequence: u64, id: &str, ask: &str) -> TaskEventRecord {
        recorded_by(sequence, "owner", 1, id, ask)
    }

    fn recorded_by(
        sequence: u64,
        actor: &str,
        revision: u64,
        id: &str,
        ask: &str,
    ) -> TaskEventRecord {
        record(
            "task.request_recorded",
            actor,
            sequence,
            &serde_json::json!({"schema": "graphhelm-task-event-v1", "taskId": format!("request-{id}"),
                "revision": revision, "at": "2026-10-22T00:00:00Z", "requestId": id, "ask": ask, "recordedBy": actor}),
        )
    }

    fn linked(sequence: u64, id: &str, issue: u64) -> TaskEventRecord {
        record(
            "task.request_linked",
            "orchestrator",
            sequence,
            &serde_json::json!({"schema": "graphhelm-task-event-v1", "taskId": format!("request-{id}"),
                "revision": 1, "at": "2026-10-22T00:00:00Z", "requestId": id, "issue": issue, "linkedBy": "orchestrator"}),
        )
    }

    /// The status rule is the contract the Graph reads: no issue is open, one unmerged issue is in
    /// progress, every issue merged is delivered; a duplicate link counts once, a later record
    /// replaces the ask, and a link to an unrecorded request shows nothing.
    #[test]
    fn a_request_moves_from_open_to_delivered_as_its_issues_merge() {
        let only_recorded = [recorded(1, "r1", "Make the Studio one header")];
        let views = requests(&only_recorded, &[]);
        assert_eq!(views.len(), 1);
        assert_eq!(views[0].status, RequestStatus::Open);

        let records = [
            recorded(1, "r1", "Make the Studio one header"),
            linked(2, "r1", 1770),
            linked(3, "r1", 1770),
            linked(4, "r1", 1771),
            recorded_by(
                5,
                "owner",
                2,
                "r1",
                "One header, and Start a team run in it",
            ),
            linked(6, "ghost", 9),
        ];
        // Real slices from the task fold: issue 1770 merged, issue 1771 open in review.
        let work = |merge: bool| {
            let mut out = vec![
                task(
                    10,
                    "lane-a",
                    "task.claimed",
                    "issue-1770",
                    serde_json::json!({"issue": 1770, "lane": "lane-a", "branch": "b1"}),
                ),
                task(
                    11,
                    "lane-a",
                    "task.pr_opened",
                    "issue-1770",
                    serde_json::json!({"pr": 1771, "headSha": "0123456789abcdef", "journeys": [], "lane": "lane-a"}),
                ),
                task(
                    12,
                    "lane-r",
                    "task.merged",
                    "issue-1770",
                    serde_json::json!({"pr": 1771, "mergeSha": "fedcba9876543210", "closes": [1770], "merger": "lane-r"}),
                ),
                task(
                    13,
                    "lane-b",
                    "task.claimed",
                    "issue-1771",
                    serde_json::json!({"issue": 1771, "lane": "lane-b", "branch": "b2"}),
                ),
                task(
                    14,
                    "lane-b",
                    "task.pr_opened",
                    "issue-1771",
                    serde_json::json!({"pr": 1772, "headSha": "0123456789abcdee", "journeys": [], "lane": "lane-b"}),
                ),
            ];
            if merge {
                out.push(task(15, "lane-r", "task.merged", "issue-1771", serde_json::json!({"pr": 1772, "mergeSha": "fedcba9876543211", "closes": [1771], "merger": "lane-r"})));
            }
            fold_task_events(&out)
        };
        let views = requests(&records, &work(false));
        assert_eq!(
            views.len(),
            1,
            "a link to an unrecorded request adds no request"
        );
        let view = &views[0];
        assert_eq!(view.ask, "One header, and Start a team run in it");
        assert_eq!(view.recorded_by, "owner");
        assert_eq!(
            view.issues.iter().map(|i| i.issue).collect::<Vec<_>>(),
            [1770, 1771]
        );
        assert_eq!(
            view.issues[0].slices,
            [RequestSlice {
                pr: Some(1771),
                step: "merged",
                merged: true
            }]
        );
        assert_eq!(view.status, RequestStatus::InProgress);
        assert_eq!(
            requests(&records, &work(true))[0].status,
            RequestStatus::Delivered
        );
    }

    /// #1792 review: only the first recorder supersedes an ask, and only with a higher revision.
    /// Defect named: any lane's later record rewrote the owner's ask while `recordedBy` still
    /// named the owner.
    #[test]
    fn only_the_first_recorder_with_a_higher_revision_supersedes_the_ask() {
        let records = [
            recorded_by(1, "owner", 1, "r1", "The owner's words"),
            recorded_by(2, "lane-a", 9, "r1", "rewritten by a lane"),
            recorded_by(3, "owner", 1, "r1", "same revision again"),
        ];
        let view = &requests(&records, &[])[0];
        assert_eq!(
            (view.ask.as_str(), view.recorded_by.as_str(), view.revision),
            ("The owner's words", "owner", 1)
        );
        let records = [
            recorded_by(1, "owner", 1, "r1", "The owner's words"),
            recorded_by(4, "owner", 2, "r1", "The owner's corrected words"),
        ];
        assert_eq!(
            requests(&records, &[])[0].ask,
            "The owner's corrected words"
        );
    }

    /// The records' own admission: the recorder must name itself, and an empty ask is no request.
    #[test]
    fn a_request_record_names_its_own_recorder_and_carries_an_ask() {
        let base = serde_json::json!({"schema": "graphhelm-task-event-v1", "taskId": "request-r1",
            "revision": 1, "at": "2026-10-22T00:00:00Z", "requestId": "r1", "ask": "x", "recordedBy": "owner"});
        assert!(
            parse_task_event(
                "task.request_recorded",
                "lane-a",
                &base.to_string(),
                1,
                None
            )
            .is_none()
        );
        let mut empty = base.clone();
        empty["ask"] = serde_json::json!("  ");
        assert!(
            parse_task_event(
                "task.request_recorded",
                "owner",
                &empty.to_string(),
                1,
                None
            )
            .is_none()
        );
        let mut extra = base;
        extra["issue"] = serde_json::json!(1);
        assert!(
            parse_task_event(
                "task.request_recorded",
                "owner",
                &extra.to_string(),
                1,
                None
            )
            .is_none()
        );
    }

    /// #1901: a request is dated by the Runtime's append time of its first record; a later
    /// revision keeps that date, and a record read without one serves no `recordedAt`.
    #[test]
    fn a_request_is_dated_by_its_first_record() {
        let at = |value: &str| Some(PersistedTimestamp::parse(value).expect("timestamp"));
        let document = |revision: u64| {
            serde_json::json!({"schema": "graphhelm-task-event-v1", "taskId": "request-r1",
            "revision": revision, "at": "2026-10-22T00:00:00Z", "requestId": "r1", "ask": "x", "recordedBy": "owner"})
        };
        let records = [
            parse_task_event(
                "task.request_recorded",
                "owner",
                &document(1).to_string(),
                1,
                at("2026-10-21T18:30:00Z"),
            )
            .expect("first"),
            parse_task_event(
                "task.request_recorded",
                "owner",
                &document(2).to_string(),
                2,
                at("2026-10-22T09:00:00Z"),
            )
            .expect("second"),
        ];
        let view = serde_json::to_value(&requests(&records, &[])[0]).expect("serialize");
        assert_eq!(view["recordedAt"], "2026-10-21T18:30:00Z");
        let view =
            serde_json::to_value(&requests(&[recorded(1, "r1", "x")], &[])[0]).expect("serialize");
        assert!(view.get("recordedAt").is_none(), "{view}");
    }
}
