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
    /// #1981: the lane the Runtime handed this request to (its dispatch note's `to`), absent until
    /// it is handed. The lane links its issue with `task.request_linked`, which moves the status.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dispatched_to: Option<String>,
    /// #1981: lanes a hand-off of this request lapsed on (no link within the lease, or the lane
    /// went silent); none of them is handed it again.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub dispatch_lapsed: Vec<String>,
    /// #1981: how many hand-off notes the journal holds for this request.
    #[serde(skip)]
    pub dispatch_attempts: u32,
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
                        dispatched_to: None,
                        dispatch_lapsed: Vec::new(),
                        dispatch_attempts: 0,
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

/// #1981: how many times one request is handed before only the owner's Hand off is left.
pub const MAX_DISPATCH_ATTEMPTS: u32 = 3;

/// #1981: how long a hand-off stays live without the lane linking an issue to the request.
pub const DISPATCH_LEASE_SECONDS: i64 = 600;

/// #1981: the id of the operator note that hands `request_id` to a lane on its `attempt`th try
/// (from 0). Derived from the request and the attempt alone, so every dispatcher names the same
/// signal and the store admits it once.
#[must_use]
pub fn dispatch_signal_id(request_id: &str, attempt: u32) -> String {
    let seed = if attempt == 0 {
        format!("request-dispatch\n{request_id}")
    } else {
        format!("request-dispatch\n{request_id}\n{attempt}")
    };
    let digest = graphhelm_protocols::hash::sha256_hex(seed.as_bytes());
    format!("dispatch-{}", &digest[..32])
}

/// #1981: one hand-off note on the journal: the lane it names and when the Runtime appended it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DispatchNote {
    pub lane: String,
    pub at: PersistedTimestamp,
}

/// #1981: fill each request's hand-offs from the notes on the journal (by signal id). The newest
/// hand-off is live (`dispatched_to`) while the request has no linked issue, the note is younger
/// than `lease_seconds` at `now`, and `gone` does not say its lane went silent or was retired.
/// Every other handed lane of an unlinked request is in `dispatch_lapsed`.
pub fn mark_dispatched(
    views: &mut [RequestView],
    notes: &BTreeMap<String, DispatchNote>,
    now: &PersistedTimestamp,
    lease_seconds: i64,
    gone: impl Fn(&str) -> bool,
) {
    for view in views {
        view.dispatched_to = None;
        view.dispatch_lapsed.clear();
        view.dispatch_attempts = 0;
        let mut newest: Option<&DispatchNote> = None;
        for attempt in 0..MAX_DISPATCH_ATTEMPTS {
            let Some(note) = notes.get(&dispatch_signal_id(&view.request_id, attempt)) else {
                break;
            };
            view.dispatch_attempts = attempt + 1;
            if let Some(older) = newest.replace(note) {
                view.dispatch_lapsed.push(older.lane.clone());
            }
        }
        let Some(note) = newest else { continue };
        if !view.issues.is_empty() {
            view.dispatch_lapsed.clear();
            continue;
        }
        let age = (*now.as_datetime() - *note.at.as_datetime()).num_seconds();
        if age < lease_seconds && !gone(&note.lane) {
            view.dispatched_to = Some(note.lane.clone());
        } else {
            view.dispatch_lapsed.push(note.lane.clone());
        }
    }
}

/// #1981: which open request goes to which lane on which attempt, oldest request first. A request
/// waits for a lane while no issue is linked, no hand-off is live and it has attempts left; it goes
/// to the first free lane (in the order given) that holds no live hand-off and never let this
/// request lapse. Pure: the caller decides what free means and appends the notes.
#[must_use]
pub fn plan_dispatch(views: &[RequestView], free_lanes: &[String]) -> Vec<(String, String, u32)> {
    let waiting = |view: &RequestView| view.status == RequestStatus::Open && view.issues.is_empty();
    let mut busy: Vec<&str> = views
        .iter()
        .filter(|view| waiting(view))
        .filter_map(|view| view.dispatched_to.as_deref())
        .collect();
    let mut ordered: Vec<&RequestView> = views
        .iter()
        .filter(|view| {
            waiting(view)
                && view.dispatched_to.is_none()
                && view.dispatch_attempts < MAX_DISPATCH_ATTEMPTS
        })
        .collect();
    ordered.sort_by_key(|view| view.sequence);
    let mut out = Vec::new();
    for view in ordered {
        let Some(lane) = free_lanes
            .iter()
            .find(|lane| !busy.contains(&lane.as_str()) && !view.dispatch_lapsed.contains(lane))
        else {
            continue;
        };
        busy.push(lane);
        out.push((
            view.request_id.clone(),
            lane.clone(),
            view.dispatch_attempts,
        ));
    }
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

    fn at(value: &str) -> PersistedTimestamp {
        PersistedTimestamp::parse(value).expect("timestamp")
    }

    fn note(lane: &str, when: &str) -> DispatchNote {
        DispatchNote {
            lane: lane.to_owned(),
            at: at(when),
        }
    }

    /// #1981: open requests go oldest first to free lanes, one live hand-off per lane; a live
    /// hand-off is not repeated and a linked request never waits. Defect named: nothing handed an
    /// owner request to any lane, so a Send to fix stayed Not started.
    #[test]
    fn open_requests_are_planned_onto_free_lanes_once_each() {
        let records = [
            recorded(1, "old", "a"),
            recorded(2, "mid", "b"),
            recorded(3, "new", "c"),
            recorded(4, "linked", "d"),
            linked(5, "linked", 9),
        ];
        let mut views = requests(&records, &[]);
        let lanes = ["lane-a".to_owned(), "lane-b".to_owned()];
        assert_eq!(
            plan_dispatch(&views, &lanes),
            [
                ("old".to_owned(), "lane-a".to_owned(), 0),
                ("mid".to_owned(), "lane-b".to_owned(), 0)
            ]
        );
        let notes = BTreeMap::from([(
            dispatch_signal_id("old", 0),
            note("lane-a", "2026-10-24T10:00:00Z"),
        )]);
        mark_dispatched(&mut views, &notes, &at("2026-10-24T10:05:00Z"), 600, |_| {
            false
        });
        assert_eq!(views[0].dispatched_to.as_deref(), Some("lane-a"));
        assert_eq!(
            plan_dispatch(&views, &lanes),
            [("mid".to_owned(), "lane-b".to_owned(), 0)],
            "a live hand-off is not repeated and its lane takes no second one"
        );
        assert!(plan_dispatch(&views, &[]).is_empty());
        assert_ne!(dispatch_signal_id("old", 0), dispatch_signal_id("old", 1));
    }

    /// #1981 review BLOCK: a hand-off nobody takes must end. Past the lease, or when its lane goes
    /// silent, the request is no longer held: it is planned again on another lane (never one it
    /// lapsed on), and after `MAX_DISPATCH_ATTEMPTS` only the owner's Hand off is left.
    #[test]
    fn a_hand_off_nobody_takes_lapses_and_goes_to_another_lane() {
        let mut views = requests(&[recorded(1, "r", "a")], &[]);
        let lanes = ["lane-a".to_owned(), "lane-b".to_owned()];
        let mut notes = BTreeMap::from([(
            dispatch_signal_id("r", 0),
            note("lane-a", "2026-10-24T10:00:00Z"),
        )]);
        mark_dispatched(&mut views, &notes, &at("2026-10-24T10:11:00Z"), 600, |_| {
            false
        });
        assert_eq!(views[0].dispatched_to, None, "past the lease");
        assert_eq!(views[0].dispatch_lapsed, ["lane-a"]);
        assert_eq!(
            plan_dispatch(&views, &lanes),
            [("r".to_owned(), "lane-b".to_owned(), 1)]
        );
        mark_dispatched(
            &mut views,
            &notes,
            &at("2026-10-24T10:01:00Z"),
            600,
            |lane| lane == "lane-a",
        );
        assert_eq!(views[0].dispatched_to, None, "its lane went silent");
        notes.insert(
            dispatch_signal_id("r", 1),
            note("lane-b", "2026-10-24T10:12:00Z"),
        );
        notes.insert(
            dispatch_signal_id("r", 2),
            note("lane-c", "2026-10-24T10:30:00Z"),
        );
        mark_dispatched(&mut views, &notes, &at("2026-10-24T11:00:00Z"), 600, |_| {
            false
        });
        assert_eq!(views[0].dispatch_lapsed, ["lane-a", "lane-b", "lane-c"]);
        assert!(
            plan_dispatch(&views, &["lane-d".to_owned()]).is_empty(),
            "attempts spent: only the owner hands it now"
        );
        let mut linked_views = requests(&[recorded(1, "r", "a"), linked(2, "r", 7)], &[]);
        mark_dispatched(
            &mut linked_views,
            &notes,
            &at("2026-10-24T11:00:00Z"),
            600,
            |_| false,
        );
        assert_eq!(linked_views[0].dispatched_to, None);
        assert!(linked_views[0].dispatch_lapsed.is_empty());
        assert!(plan_dispatch(&linked_views, &lanes).is_empty());
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
