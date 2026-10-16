//! The task projection (#1149, optimization plan A1): one fold over an execution's `task.*`
//! records that says, for every task slice, which delivery step is lit, who holds it, its PR and
//! head, its claim and its last heartbeat.
//!
//! Until this module existed the only place that fold ran was the browser
//! (`apps/studio/src/runtime/team-tasks.ts`), so the CLI, the MCP `status` tool and the hooks could
//! not answer "what step is issue N in". The fold here is that algorithm, record for record, so a
//! log the Studio drew yesterday folds to the same slices today: the golden corpus in
//! `tests/tasks.rs` is the Studio's own cell list.
//!
//! The fold is pure. It takes records already opened and verified by the caller (the Runtime
//! opens the sealed envelope, checks its digest against the recorded event and that its signer is
//! the actor that recorded it) and never reads a store, a clock or the network.

use std::collections::BTreeMap;

use graphhelm_protocols::PersistedTimestamp;
use serde::{Deserialize, Serialize};

/// The schema every `task.*` description must name.
pub const TASK_EVENT_SCHEMA: &str = "graphhelm-task-event-v1";
/// Longest description the fold reads; the Runtime admits at most 8 KiB, an old log may hold more.
pub const MAX_ENVELOPE_BYTES: usize = 1024 * 1024;
/// #591: a gap at least this long between two records of one lane is a stall, not active work.
pub const ACTIVE_GAP_MS: i64 = 30 * 60_000;

/// The eleven record kinds a lane (or, for `task.queued`, whoever reads the issue list) writes
/// (DELIVERY.md, "Task records").
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum TaskEventKind {
    #[serde(rename = "task.claimed")]
    Claimed,
    #[serde(rename = "task.planned")]
    Planned,
    #[serde(rename = "task.released")]
    Released,
    #[serde(rename = "task.heartbeat")]
    Heartbeat,
    #[serde(rename = "task.pr_opened")]
    PrOpened,
    #[serde(rename = "task.review_assigned")]
    ReviewAssigned,
    #[serde(rename = "task.review_verdict")]
    ReviewVerdict,
    #[serde(rename = "task.merged")]
    Merged,
    #[serde(rename = "task.closed")]
    Closed,
    #[serde(rename = "task.critic_verdict")]
    CriticVerdict,
    /// #1469: the open wave issues its recorder read; the newest record replaces the earlier ones.
    /// It opens no slice; the tasks route serves the ones no live slice holds as the backlog.
    #[serde(rename = "task.queued")]
    Queued,
}

impl TaskEventKind {
    /// The kind a signal `type` names, or `None` for anything that is not a task record.
    #[must_use]
    pub fn parse(kind: &str) -> Option<Self> {
        Some(match kind {
            "task.claimed" => Self::Claimed,
            "task.planned" => Self::Planned,
            "task.released" => Self::Released,
            "task.heartbeat" => Self::Heartbeat,
            "task.pr_opened" => Self::PrOpened,
            "task.review_assigned" => Self::ReviewAssigned,
            "task.review_verdict" => Self::ReviewVerdict,
            "task.merged" => Self::Merged,
            "task.closed" => Self::Closed,
            "task.critic_verdict" => Self::CriticVerdict,
            "task.queued" => Self::Queued,
            _ => return None,
        })
    }

    /// The signal `type` spelling.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Claimed => "task.claimed",
            Self::Planned => "task.planned",
            Self::Released => "task.released",
            Self::Heartbeat => "task.heartbeat",
            Self::PrOpened => "task.pr_opened",
            Self::ReviewAssigned => "task.review_assigned",
            Self::ReviewVerdict => "task.review_verdict",
            Self::Merged => "task.merged",
            Self::Closed => "task.closed",
            Self::CriticVerdict => "task.critic_verdict",
            Self::Queued => "task.queued",
        }
    }
}

/// A review verdict word, as the comment text spells it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Verdict {
    #[serde(rename = "APPROVE")]
    Approve,
    #[serde(rename = "APPROVE-WITH-RISK")]
    ApproveWithRisk,
    #[serde(rename = "BLOCK")]
    Block,
}

impl Verdict {
    fn parse(value: &serde_json::Value) -> Option<Self> {
        match value.as_str()? {
            "APPROVE" => Some(Self::Approve),
            "APPROVE-WITH-RISK" => Some(Self::ApproveWithRisk),
            "BLOCK" => Some(Self::Block),
            _ => None,
        }
    }

    /// The comment-text spelling.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Approve => "APPROVE",
            Self::ApproveWithRisk => "APPROVE-WITH-RISK",
            Self::Block => "BLOCK",
        }
    }
}

/// #467: what one round of the blind design critic decided.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CriticOutcome {
    Pass,
    Revise,
    Exhausted,
}

/// #467: one round of the design critic, as its `task.critic_verdict` record states it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CriticRound {
    pub round: u64,
    pub score: u64,
    pub pass_score: u64,
    pub max_rounds: u64,
    pub verdict: CriticOutcome,
}

/// #480: the critic a plan's class gets.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CriticConfig {
    pub mode: String,
    pub pass_score: u64,
    pub max_rounds: u64,
}

/// #480: what `task.planned` carries: the keel plan's decided classes, review count, proof, the
/// critic it gets and the plan in one line.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskPlan {
    pub classes: Vec<String>,
    pub reviews: u64,
    pub proof: String,
    pub critic: CriticConfig,
    pub summary: String,
}

/// Why a PR slice was closed without a merge.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CloseReason {
    Superseded,
    Abandoned,
}

/// One verified `task.*` record: the fields its kind carries, the actor that recorded it, its
/// sequence and the Runtime's own append time (`occurred_at`, which a lane cannot write).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskEventRecord {
    pub kind: TaskEventKind,
    pub actor_id: String,
    pub sequence: u64,
    pub task_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub issue: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pr: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lane: Option<String>,
    /// The claimant's report of who assigned the work, never independently verified.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assigned_by: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub head_sha: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub journeys: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reviewer: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verdict: Option<Verdict>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub comment_url: Option<String>,
    /// `owner/name` of the GitHub repository, from the record that opens the task (#420).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repo: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub merge_sha: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub closes: Option<Vec<u64>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub close_reason: Option<CloseReason>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub superseded_by: Option<u64>,
    /// #477: the issue's (claimed) or the PR's (pr_opened) title and one-line summary.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    /// #514: the issue whose work turned this task up (on `task.claimed`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<u64>,
    /// #729: the exact prior claim sequence released by its lane, and its stated reason.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub claim_sequence: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// #1193: how long a claim stays live without a heartbeat, in seconds; a claim without the
    /// field carries the schema's default, 1800.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lease_seconds: Option<u64>,
    /// #1193: `graphhelm` on a release the Stop reconciler signed for the lane the record names;
    /// absent on a lane's own release.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub released_by: Option<String>,
    /// #502: when the Runtime appended the record (its own clock, not the lane's `at`).
    #[serde(default)]
    pub occurred_at: Option<PersistedTimestamp>,
    /// #467: the round a `task.critic_verdict` states.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub critic: Option<CriticRound>,
    /// #480: the lane's keel plan, from `task.planned`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan: Option<TaskPlan>,
    /// #1469: the issues a `task.queued` lists.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub queued: Option<Vec<QueuedIssue>>,
}

/// #1469: one open wave issue as a `task.queued` record lists it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QueuedIssue {
    pub issue: u64,
    pub title: String,
    pub opened_at: String,
}

/// #1469: the most issues one `task.queued` lists (the schema's `maxItems`), so a record stays
/// under the Runtime's 8 KiB description limit.
pub const MAX_QUEUED_ISSUES: usize = 25;

/// #1469: the newest `task.queued` record's issues that no live slice holds as its
/// issue, oldest first. An issue missing from the newest list is not shown.
#[must_use]
pub fn backlog(records: &[TaskEventRecord], tasks: &[TaskState]) -> Vec<QueuedIssue> {
    let Some(newest) = records
        .iter()
        .filter(|r| r.kind == TaskEventKind::Queued)
        .max_by_key(|r| r.sequence)
    else {
        return Vec::new();
    };
    // A slice holds its issue while it is live: not released by its lane (a released claim stays
    // in the fold with `released_by` set) and not merged.
    let held = |n: u64| {
        tasks
            .iter()
            .any(|t| t.issue == Some(n) && t.released_by.is_none() && t.step != TaskStep::Merged)
    };
    let mut out: Vec<QueuedIssue> = newest
        .queued
        .iter()
        .flatten()
        .filter(|q| !held(q.issue))
        .cloned()
        .collect();
    out.sort_by(|a, b| a.opened_at.cmp(&b.opened_at).then(a.issue.cmp(&b.issue)));
    out
}

impl TaskEventRecord {
    fn new(kind: TaskEventKind, actor_id: &str, task_id: &str) -> Self {
        Self {
            kind,
            actor_id: actor_id.to_owned(),
            sequence: 0,
            task_id: task_id.to_owned(),
            issue: None,
            pr: None,
            lane: None,
            assigned_by: None,
            branch: None,
            head_sha: None,
            journeys: None,
            reviewer: None,
            verdict: None,
            comment_url: None,
            repo: None,
            merge_sha: None,
            closes: None,
            close_reason: None,
            superseded_by: None,
            title: None,
            summary: None,
            parent: None,
            claim_sequence: None,
            reason: None,
            lease_seconds: None,
            released_by: None,
            occurred_at: None,
            critic: None,
            plan: None,
            queued: None,
        }
    }
}

// ---- field readers: the same bounds as `task-event.schema.json` and the Runtime's admission ----

fn has_control(value: &str) -> bool {
    value.chars().any(char::is_control)
}

/// A nonempty string of at most `maximum` UTF-16 units without C0/DEL characters (the Studio
/// counted `.length`, which is UTF-16 units; kept so an old log folds the same).
fn text(value: &serde_json::Value, maximum: usize) -> Option<String> {
    let s = value.as_str()?;
    let units = s.encode_utf16().count();
    (units > 0
        && units <= maximum
        && !s
            .chars()
            .any(|c| matches!(c, '\u{0000}'..='\u{001f}' | '\u{007f}')))
    .then(|| s.to_owned())
}

/// A nonempty string of at most `maximum` code points without control characters.
fn words_text(value: &serde_json::Value, maximum: usize) -> Option<String> {
    let s = value.as_str()?;
    (!s.is_empty() && s.chars().count() <= maximum && !has_control(s)).then(|| s.to_owned())
}

/// A positive integer.
fn count(value: &serde_json::Value) -> Option<u64> {
    value.as_u64().filter(|n| *n > 0)
}

fn bounded(value: &serde_json::Value, min: u64, max: u64) -> Option<u64> {
    value.as_u64().filter(|n| (min..=max).contains(n))
}

/// `^[A-Za-z0-9][A-Za-z0-9._:-]{0,127}$`.
fn task_identity(value: &serde_json::Value) -> Option<String> {
    let s = value.as_str()?;
    let bytes = s.as_bytes();
    let first = *bytes.first()?;
    (bytes.len() <= 128
        && first.is_ascii_alphanumeric()
        && bytes
            .iter()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b':' | b'-')))
    .then(|| s.to_owned())
}

fn journey_ids(value: &serde_json::Value) -> Option<Vec<String>> {
    value.as_array()?.iter().map(task_identity).collect()
}

/// `^[0-9a-f]{7,64}$`.
/// #1262: two spellings of one commit. Heads are parsed by `sha` (7..=64 lowercase hex), and a
/// lane may record the same commit short in `pr_opened` and full in `review_verdict`; comparing
/// them exactly made a BLOCK invisible and the stall rule name the reviewer as owing a verdict
/// already given. Equal when one is a prefix of the other; exact equality is the special case.
fn same_head(a: &str, b: &str) -> bool {
    a.len() >= 7 && b.len() >= 7 && (a.starts_with(b) || b.starts_with(a))
}

/// `same_head` over optional heads; two absent heads are equal, as `==` said before.
fn same_opt_head(a: Option<&String>, b: Option<&String>) -> bool {
    match (a, b) {
        (Some(a), Some(b)) => same_head(a, b),
        (None, None) => true,
        _ => false,
    }
}

fn sha(value: &serde_json::Value) -> Option<String> {
    let s = value.as_str()?;
    ((7..=64).contains(&s.len())
        && s.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)))
    .then(|| s.to_owned())
}

/// `owner/name` (#420), the rule of `task-event.schema.json` `$defs.repo`:
/// `^[A-Za-z0-9][A-Za-z0-9-]{0,38}/(\.\.?[A-Za-z0-9_-][A-Za-z0-9_.-]{0,97}|[A-Za-z0-9_-][A-Za-z0-9_.-]{0,99})$`
/// and at most 140 characters.
#[must_use]
pub fn valid_repo(value: &str) -> bool {
    if value.len() > 140 {
        return false;
    }
    let Some((owner, name)) = value.split_once('/') else {
        return false;
    };
    let owner = owner.as_bytes();
    let owner_ok = !owner.is_empty()
        && owner.len() <= 39
        && owner[0].is_ascii_alphanumeric()
        && owner[1..]
            .iter()
            .all(|b| b.is_ascii_alphanumeric() || *b == b'-');
    if !owner_ok {
        return false;
    }
    let name = name.as_bytes();
    let rest_ok = |rest: &[u8], max: usize| {
        !rest.is_empty()
            && rest.len() <= max
            && (rest[0].is_ascii_alphanumeric() || matches!(rest[0], b'_' | b'-'))
            && rest[1..]
                .iter()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b'-'))
    };
    if let Some(dotted) = name.strip_prefix(b"..") {
        rest_ok(dotted, 98)
    } else if let Some(dotted) = name.strip_prefix(b".") {
        rest_ok(dotted, 98)
    } else {
        rest_ok(name, 100)
    }
}

/// Optional `repo`: absent is `Ok(None)`, malformed is `Err(())` (the record is refused).
fn repository(document: &serde_json::Map<String, serde_json::Value>) -> Result<Option<String>, ()> {
    match document.get("repo") {
        None => Ok(None),
        Some(value) => match value.as_str() {
            Some(repo) if valid_repo(repo) => Ok(Some(repo.to_owned())),
            _ => Err(()),
        },
    }
}

/// #477: optional `title` / `summary`; present but malformed refuses the record.
fn words(
    document: &serde_json::Map<String, serde_json::Value>,
) -> Result<(Option<String>, Option<String>), ()> {
    let title = match document.get("title") {
        None => None,
        Some(value) => Some(words_text(value, 200).ok_or(())?),
    };
    let summary = match document.get("summary") {
        None => None,
        Some(value) => Some(words_text(value, 300).ok_or(())?),
    };
    Ok((title, summary))
}

const TASK_CLASSES: [&str; 4] = ["docs", "code", "user_visible", "invariant"];
const PROOFS: [&str; 4] = ["none", "tests", "journey", "both"];

/// The same bounds as `task-event.schema.json`'s `task.planned` and the Runtime's admission.
fn task_plan(document: &serde_json::Map<String, serde_json::Value>) -> Option<TaskPlan> {
    let classes = document.get("classes")?.as_array()?;
    if classes.is_empty() || classes.len() > 4 {
        return None;
    }
    let mut seen: Vec<&str> = Vec::new();
    for class in classes {
        let class = class.as_str()?;
        if !TASK_CLASSES.contains(&class) || seen.contains(&class) {
            return None;
        }
        seen.push(class);
    }
    let reviews = bounded(document.get("reviews")?, 1, 5)?;
    let proof = document.get("proof")?.as_str()?;
    if !PROOFS.contains(&proof) {
        return None;
    }
    let critic = document.get("critic")?.as_object()?;
    if !critic
        .keys()
        .all(|key| matches!(key.as_str(), "mode" | "passScore" | "maxRounds"))
    {
        return None;
    }
    let mode = critic.get("mode")?.as_str()?;
    if !matches!(mode, "none" | "design") {
        return None;
    }
    let pass_score = bounded(critic.get("passScore")?, 1, 10)?;
    let max_rounds = bounded(critic.get("maxRounds")?, 1, 5)?;
    let summary_value = document.get("summary")?;
    let summary = summary_value.as_str()?;
    if summary.chars().count() > 300 {
        return None;
    }
    let summary = text(summary_value, summary.encode_utf16().count())?;
    Some(TaskPlan {
        classes: seen.into_iter().map(str::to_owned).collect(),
        reviews,
        proof: proof.to_owned(),
        critic: CriticConfig {
            mode: mode.to_owned(),
            pass_score,
            max_rounds,
        },
        summary,
    })
}

/// One record, or `None` when the description is not a well-formed `task.*` document for `kind`
/// or names another actor than the one that recorded it (the Runtime refuses those too; this
/// keeps an old log honest). `sequence` and `occurred_at` are the recorded event's.
#[must_use]
pub fn parse_task_event(
    kind: &str,
    actor_id: &str,
    description: &str,
    sequence: u64,
    occurred_at: Option<PersistedTimestamp>,
) -> Option<TaskEventRecord> {
    if description.len() > MAX_ENVELOPE_BYTES {
        return None;
    }
    let kind = TaskEventKind::parse(kind)?;
    let document: serde_json::Value = serde_json::from_str(description).ok()?;
    let document = document.as_object()?;
    let field = |key: &str| document.get(key).unwrap_or(&serde_json::Value::Null);
    let task_id = task_identity(field("taskId"))?;
    if field("schema") != TASK_EVENT_SCHEMA || count(field("revision")).is_none() {
        return None;
    }
    let is_actor = |key: &str| field(key).as_str() == Some(actor_id);
    let mut record = TaskEventRecord::new(kind, actor_id, &task_id);
    record.sequence = sequence;
    record.occurred_at = occurred_at;
    match kind {
        TaskEventKind::Claimed => {
            let repo = repository(document).ok()?;
            let issue = count(field("issue"))?;
            let lane = text(field("lane"), 128);
            let assigned_by = match document.get("assignedBy") {
                None => None,
                Some(value) => {
                    let s = value.as_str()?;
                    if s.chars().count() > 128 || has_control(s) {
                        return None;
                    }
                    let assigner = text(value, s.encode_utf16().count())?;
                    if Some(&assigner) == lane.as_ref() {
                        return None;
                    }
                    Some(assigner)
                }
            };
            let branch = text(field("branch"), 256)?;
            let (title, summary) = words(document).ok()?;
            let parent = match document.get("parent") {
                None => None,
                Some(value) => Some(count(value)?),
            };
            // #577: a claim may name its journeys; a malformed list drops the field, never the claim.
            let journeys = document.get("journeys").and_then(journey_ids);
            if lane.as_deref() != Some(actor_id) {
                return None;
            }
            record.issue = Some(issue);
            record.lane = lane;
            record.branch = Some(branch);
            record.title = title;
            record.summary = summary;
            record.assigned_by = assigned_by;
            record.repo = repo;
            record.parent = parent;
            record.journeys = journeys;
            // #1193: the lease, 60 s to a day; absent means the schema's default.
            record.lease_seconds = Some(match document.get("leaseSeconds") {
                None => 1800,
                Some(value) => bounded(value, 60, 86400)?,
            });
        }
        TaskEventKind::Planned => {
            let plan = task_plan(document)?;
            if !is_actor("lane") {
                return None;
            }
            record.lane = Some(actor_id.to_owned());
            record.plan = Some(plan);
        }
        TaskEventKind::Released => {
            let claim_sequence = count(field("claimSequence"))?;
            let reason = field("reason").as_str()?;
            if reason.chars().count() > 500 || has_control(reason) {
                return None;
            }
            let reason = text(field("reason"), 1000)?;
            let lane = text(field("lane"), 128)?;
            // #1193: `releasedBy: graphhelm` is the Stop reconciler ending another lane's lapsed
            // claim; it is read only when `graphhelm` signed it and `lane` names someone else.
            // Without it, the release is the lane's own (the same identity rule as the Runtime).
            let released_by = match document.get("releasedBy") {
                None => {
                    if lane != actor_id {
                        return None;
                    }
                    None
                }
                Some(value) => {
                    if value.as_str() != Some("graphhelm")
                        || actor_id != "graphhelm"
                        || lane == "graphhelm"
                    {
                        return None;
                    }
                    Some("graphhelm".to_owned())
                }
            };
            record.lane = Some(lane);
            record.claim_sequence = Some(claim_sequence);
            record.reason = Some(reason);
            record.released_by = released_by;
        }
        TaskEventKind::Heartbeat => {
            if !is_actor("lane")
                || !document.keys().all(|key| {
                    matches!(
                        key.as_str(),
                        "schema" | "taskId" | "revision" | "at" | "lane"
                    )
                })
            {
                return None;
            }
            record.lane = Some(actor_id.to_owned());
        }
        TaskEventKind::PrOpened => {
            let repo = repository(document).ok()?;
            let pr = count(field("pr"))?;
            let head_sha = sha(field("headSha"))?;
            let lane = text(field("lane"), 128)?;
            let journeys = journey_ids(field("journeys"))?;
            let (title, summary) = words(document).ok()?;
            if lane != actor_id {
                return None;
            }
            record.pr = Some(pr);
            record.head_sha = Some(head_sha);
            record.lane = Some(lane);
            record.journeys = Some(journeys);
            record.title = title;
            record.summary = summary;
            record.repo = repo;
        }
        TaskEventKind::ReviewAssigned => {
            record.pr = Some(count(field("pr"))?);
            record.head_sha = Some(sha(field("headSha"))?);
            record.reviewer = Some(text(field("reviewer"), 128)?);
        }
        TaskEventKind::ReviewVerdict => {
            let pr = count(field("pr"))?;
            let head_sha = sha(field("headSha"))?;
            let verdict = Verdict::parse(field("verdict"))?;
            let comment_url = text(field("commentUrl"), 512)?;
            if !is_actor("reviewer") {
                return None;
            }
            record.pr = Some(pr);
            record.head_sha = Some(head_sha);
            record.reviewer = Some(actor_id.to_owned());
            record.verdict = Some(verdict);
            record.comment_url = Some(comment_url);
        }
        TaskEventKind::Merged => {
            let pr = count(field("pr"))?;
            let merge_sha = sha(field("mergeSha"))?;
            let closes: Vec<u64> = field("closes")
                .as_array()?
                .iter()
                .map(count)
                .collect::<Option<_>>()?;
            if !is_actor("merger") {
                return None;
            }
            record.pr = Some(pr);
            record.merge_sha = Some(merge_sha);
            record.closes = Some(closes);
        }
        TaskEventKind::Closed => {
            let pr = count(field("pr"))?;
            let reason = match field("reason").as_str()? {
                "superseded" => CloseReason::Superseded,
                "abandoned" => CloseReason::Abandoned,
                _ => return None,
            };
            let by = count(field("by"))?;
            record.pr = Some(pr);
            record.close_reason = Some(reason);
            record.superseded_by = Some(by);
        }
        TaskEventKind::CriticVerdict => {
            // The same rule as the Runtime's admission: the verdict agrees with the score and the
            // round, so an old or foreign log cannot draw an under-threshold pass.
            let round = bounded(field("round"), 1, 5)?;
            let score = bounded(field("score"), 0, 10)?;
            let pass_score = bounded(field("passScore"), 1, 10)?;
            let max_rounds = bounded(field("maxRounds"), 1, 5)?;
            if round > max_rounds || !is_actor("lane") {
                return None;
            }
            let verdict = match field("verdict").as_str()? {
                "pass" if score >= pass_score => CriticOutcome::Pass,
                "revise" if score < pass_score && round < max_rounds => CriticOutcome::Revise,
                "exhausted" if score < pass_score && round == max_rounds => {
                    CriticOutcome::Exhausted
                }
                _ => return None,
            };
            record.lane = Some(actor_id.to_owned());
            record.critic = Some(CriticRound {
                round,
                score,
                pass_score,
                max_rounds,
                verdict,
            });
        }
        TaskEventKind::Queued => {
            let repo = repository(document).ok()?;
            if !document.keys().all(|key| {
                matches!(
                    key.as_str(),
                    "schema" | "taskId" | "revision" | "at" | "repo" | "issues"
                )
            }) {
                return None;
            }
            let items = field("issues").as_array()?;
            if items.len() > MAX_QUEUED_ISSUES {
                return None;
            }
            let mut queued = Vec::with_capacity(items.len());
            for item in items {
                let item = item.as_object()?;
                if !item
                    .keys()
                    .all(|key| matches!(key.as_str(), "issue" | "title" | "openedAt"))
                {
                    return None;
                }
                let get = |key: &str| item.get(key).unwrap_or(&serde_json::Value::Null);
                queued.push(QueuedIssue {
                    issue: count(get("issue"))?,
                    title: words_text(get("title"), 200)?,
                    opened_at: text(get("openedAt"), 64)?,
                });
            }
            record.repo = repo;
            record.queued = Some(queued);
        }
    }
    Some(record)
}

// ---- the fold ----

/// #480: a claimed task is planning until its `task.planned`; a `design` plan then waits on its
/// critic (#467) before implementation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TaskStep {
    Plan,
    Critic,
    Implement,
    Review,
    Merge,
    Merged,
}

impl TaskStep {
    /// The lowercase step name the views draw.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Plan => "plan",
            Self::Critic => "critic",
            Self::Implement => "implement",
            Self::Review => "review",
            Self::Merge => "merge",
            Self::Merged => "merged",
        }
    }
}

/// #502: when a slice entered its step and what it spent in the steps it left, from the Runtime's
/// append time of each record.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StepClock {
    /// When the slice entered its current step, or `None` when no record carried a time.
    pub since: Option<PersistedTimestamp>,
    /// Time of the most recent record in the current step; idle time starts after this point.
    pub last_at: Option<PersistedTimestamp>,
    /// Milliseconds spent in each step the slice has left, summed over every visit.
    pub spent: BTreeMap<String, i64>,
}

const fn timed_step(step: &str) -> bool {
    matches!(
        step.as_bytes(),
        b"plan" | b"critic" | b"implement" | b"review" | b"merge"
    )
}

/// Moves the clock when a record changes the slice's step. A record that leaves the step as it
/// was (an assignment, a stray verdict) only starts the clock when it had no start yet.
pub fn clock_step(
    clock: &mut StepClock,
    before: &str,
    after: &str,
    at: Option<PersistedTimestamp>,
    resume_after_gap: bool,
) {
    let millis = |t: &PersistedTimestamp| t.as_datetime().timestamp_millis();
    let now = at.as_ref().map(millis);
    let since = clock.since.as_ref().map(millis);
    let last_at = clock.last_at.as_ref().or(clock.since.as_ref()).map(millis);
    if resume_after_gap
        && let (Some(since_ms), Some(last_ms), Some(now_ms)) = (since, last_at, now)
        && now_ms >= last_ms
        && now_ms - last_ms >= ACTIVE_GAP_MS
    {
        if last_ms > since_ms && timed_step(before) {
            *clock.spent.entry(before.to_owned()).or_insert(0) += last_ms - since_ms;
        }
        clock.since = at.clone();
        clock.last_at = at;
        return;
    }
    if before == after {
        if clock.since.is_none() && at.is_some() {
            clock.since = at.clone();
        }
        if at.is_some() {
            clock.last_at = at;
        }
        return;
    }
    if let (Some(since_ms), Some(now_ms)) = (since, now)
        && now_ms >= since_ms
        && timed_step(before)
    {
        *clock.spent.entry(before.to_owned()).or_insert(0) += now_ms - since_ms;
    }
    clock.since = at.clone();
    clock.last_at = at;
}

/// #514: one round of the review loop: the BLOCK, and the head the author pushed in answer.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewRound {
    pub reviewer: String,
    pub head_sha: String,
    pub comment_url: String,
    pub fix_head: Option<String>,
    pub blocked_at: Option<PersistedTimestamp>,
    pub fixed_at: Option<PersistedTimestamp>,
}

/// A BLOCK on the current head that no newer head or verdict has answered.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Blocker {
    pub reviewer: String,
    pub head_sha: String,
    pub comment_url: String,
}

/// APPROVE-WITH-RISK on the current head.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RiskApproval {
    pub reviewer: String,
    pub comment_url: String,
}

/// An assignment for a head no `pr_opened` has named yet.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingReviewer {
    pub head_sha: String,
    pub reviewer: String,
}

/// #457: a verdict that does not speak for the current head, with its reason.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "reason", rename_all = "lowercase")]
pub enum StrayVerdict {
    /// A head no `pr_opened` has named yet; applied if that `pr_opened` arrives later.
    #[serde(rename_all = "camelCase")]
    Unrecorded {
        reviewer: String,
        verdict: String,
        head_sha: String,
        record: Box<TaskEventRecord>,
    },
    /// An older recorded head that a newer push superseded.
    #[serde(rename_all = "camelCase")]
    Superseded {
        reviewer: String,
        verdict: String,
        head_sha: String,
        superseded_by: String,
    },
}

/// One slice of a task (#460: its PR once one is recorded, else its claim), with the step that is
/// lit and everything a view needs to draw it.
///
/// #1211: the wire shape is the Studio's `TaskState` contract (`apps/studio/src/runtime/team-tasks.ts`).
/// Fields that type declares `| null` serialise `null` when absent; fields it declares optional
/// (`?:`) and never writes as `null` — `pendingReviewer`, `approvedWithRisk`, `completionRecorded`,
/// `claimSequence`, `releasedBy` — are OMITTED when absent, because the views read
/// `releasedBy === undefined` as "the claim is live"; a `null` there emptied the Lanes kanban.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskState {
    /// Unique in the fold: `<taskId>#pr-<N>` or `<taskId>#claim-<index>`.
    pub key: String,
    pub task_id: String,
    /// The branch the slice's `task.claimed` named, if any.
    pub branch: Option<String>,
    pub issue: Option<u64>,
    pub pr: Option<u64>,
    pub lane: Option<String>,
    pub assigned_by: Option<String>,
    /// Only explicit review assignments; `reviewers` also includes verdict authors.
    pub assigned_reviewers: Vec<String>,
    /// Assignment on the current head only; older reviewers remain in `reviewers`.
    pub current_reviewer: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pending_reviewer: Option<PendingReviewer>,
    pub head_sha: Option<String>,
    pub journeys: Vec<String>,
    /// #480: the lane's recorded keel plan; `None` when none was recorded.
    pub plan: Option<TaskPlan>,
    /// The step that is lit.
    pub step: TaskStep,
    pub blocked_by: Option<Blocker>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approved_with_risk: Option<RiskApproval>,
    /// Sequence of the current unanswered BLOCK.
    pub blocked_sequence: Option<u64>,
    /// Most recent task record written by this slice's author.
    pub author_last_sequence: Option<u64>,
    /// Trusted Runtime time of this open claim's newest heartbeat; never a lifecycle step.
    pub last_heartbeat_at: Option<PersistedTimestamp>,
    pub reviewers: Vec<String>,
    pub merge_sha: Option<String>,
    /// True only when this slice's own PR was merged, not when a sibling merge cascaded over it.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub completion_recorded: bool,
    /// `https://github.com/<owner>/<repo>`; links need it.
    pub repo_url: Option<String>,
    pub stray_verdicts: Vec<StrayVerdict>,
    /// #477: the issue's title and summary (from `task.claimed`) and the PR's (from `pr_opened`).
    pub title: Option<String>,
    pub summary: Option<String>,
    pub pr_title: Option<String>,
    pub pr_summary: Option<String>,
    /// #467: the design critic's newest recorded round.
    pub critic: Option<CriticRound>,
    /// Every head a `pr_opened` named, in order.
    pub recorded_heads: Vec<String>,
    /// #514: the issue whose work turned this task up.
    pub parent: Option<u64>,
    /// #514: the BLOCK rounds of the review loop, oldest first.
    pub rounds: Vec<ReviewRound>,
    pub clock: StepClock,
    pub last_sequence: u64,
    /// #729: sequence of this slice's original claim, used to reject releases of another claim.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub claim_sequence: Option<u64>,
    /// The lane that ended the claim: the lane itself, or `graphhelm` (#1193) when the Stop
    /// reconciler released it as lapsed. Absent, never `null`, while the claim is live (#1211).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub released_by: Option<String>,
    /// #1193: the Runtime's time of this slice's original claim, and that claim's lease in
    /// seconds; the overdue state is computed where the Runtime's clock is, never here. Not in
    /// the Studio's type: omitted when absent so the reply adds nothing the fold never writes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub claimed_at: Option<PersistedTimestamp>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lease_seconds: Option<u64>,
}

/// #562: a design critic round whose task nobody has claimed: no issue, PR or branch.
#[must_use]
pub fn is_unclaimed_critic(task: &TaskState) -> bool {
    task.critic.is_some() && task.issue.is_none() && task.pr.is_none() && task.branch.is_none()
}

#[derive(Clone, Debug)]
struct BlockerAt {
    reviewer: String,
    head_sha: String,
    comment_url: String,
    sequence: u64,
}

struct Slice {
    /// Stable across removals: the key `routes` and `slices` share (#1192).
    id: usize,
    state: TaskState,
    ended: bool,
    first_pr_opening: Option<u64>,
    blockers: Option<BTreeMap<String, BlockerAt>>,
}

/// #1192: where one record landed. `slice` is the id of a [`FoldSlice`], including slices that
/// were later closed or absorbed, so a timeline can draw every record where the fold put it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Route {
    pub sequence: u64,
    pub slice: usize,
}

/// #1192: one slice the fold created, live or not. `ended` is a `task.closed`; `absorbed` is a
/// PR-less claim a later `pr_opened` of the same lane folded into its PR's slice (it is not in
/// `tasks`). Live slices are the ones `tasks` returns, in `tasks`' order.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FoldSlice {
    pub id: usize,
    pub ended: bool,
    pub absorbed: bool,
    pub state: TaskState,
}

/// The whole fold (#1192): the live slices the views list, every slice with its id, and the
/// record-to-slice routes a lane timeline needs.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskFold {
    pub tasks: Vec<TaskState>,
    pub slices: Vec<FoldSlice>,
    pub routes: Vec<Route>,
}

fn new_state(event: &TaskEventRecord) -> TaskState {
    TaskState {
        key: String::new(),
        task_id: event.task_id.clone(),
        branch: None,
        issue: None,
        pr: if event.kind == TaskEventKind::Claimed {
            None
        } else {
            event.pr
        },
        lane: None,
        assigned_by: None,
        assigned_reviewers: Vec::new(),
        current_reviewer: None,
        pending_reviewer: None,
        head_sha: None,
        journeys: Vec::new(),
        plan: None,
        step: if matches!(event.kind, TaskEventKind::Claimed | TaskEventKind::Planned) {
            TaskStep::Plan
        } else {
            TaskStep::Implement
        },
        blocked_by: None,
        approved_with_risk: None,
        blocked_sequence: None,
        author_last_sequence: None,
        last_heartbeat_at: None,
        reviewers: Vec::new(),
        merge_sha: None,
        completion_recorded: false,
        repo_url: None,
        stray_verdicts: Vec::new(),
        title: None,
        summary: None,
        pr_title: None,
        pr_summary: None,
        critic: None,
        recorded_heads: Vec::new(),
        parent: None,
        rounds: Vec::new(),
        clock: StepClock::default(),
        last_sequence: 0,
        claim_sequence: None,
        released_by: None,
        claimed_at: None,
        lease_seconds: None,
    }
}

fn push_unique(list: &mut Vec<String>, value: &str) {
    if !list.iter().any(|known| known == value) {
        list.push(value.to_owned());
    }
}

/// `https://github.com/<owner>/<repo>` read off a verdict's comment URL, when it has that shape.
fn repo_url_of(comment_url: &str) -> Option<String> {
    let rest = comment_url.strip_prefix("https://github.com/")?;
    let mut parts = rest.splitn(3, '/');
    let owner = parts.next()?;
    let repo = parts.next()?;
    parts.next()?; // the pattern needs a `/` after the repository
    let segment_ok = |s: &str| {
        !s.is_empty()
            && s.bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b'-'))
    };
    (segment_ok(owner) && segment_ok(repo)).then(|| format!("https://github.com/{owner}/{repo}"))
}

fn apply_verdict(
    state: &mut TaskState,
    event: &TaskEventRecord,
    blockers: &mut BTreeMap<String, BlockerAt>,
) {
    if let Some(reviewer) = &event.reviewer {
        push_unique(&mut state.reviewers, reviewer);
    }
    // Old logs carry no `repo`: a verdict's comment URL still names the repository.
    if state.repo_url.is_none() {
        state.repo_url = event.comment_url.as_deref().and_then(repo_url_of);
    }
    let reviewer = event.reviewer.clone().unwrap_or_default();
    let head_sha = event.head_sha.clone().unwrap_or_default();
    let comment_url = event.comment_url.clone().unwrap_or_default();
    if event.verdict == Some(Verdict::Block) {
        blockers.insert(
            reviewer.clone(),
            BlockerAt {
                reviewer: reviewer.clone(),
                head_sha: head_sha.clone(),
                comment_url: comment_url.clone(),
                sequence: event.sequence,
            },
        );
        // A second BLOCK on the same unanswered head (another reviewer) is the same round.
        let same_round = state.rounds.last().is_some_and(|last| {
            last.fix_head.is_none()
                && event
                    .head_sha
                    .as_deref()
                    .is_some_and(|head| same_head(&last.head_sha, head))
        });
        if !same_round {
            state.rounds.push(ReviewRound {
                reviewer,
                head_sha,
                comment_url,
                fix_head: None,
                blocked_at: event.occurred_at.clone(),
                fixed_at: None,
            });
        }
    } else {
        if event.verdict == Some(Verdict::ApproveWithRisk) {
            state.approved_with_risk = Some(RiskApproval {
                reviewer: reviewer.clone(),
                comment_url,
            });
        }
        if let Some(reviewer) = &event.reviewer {
            blockers.remove(reviewer);
        }
    }
    let oldest = blockers.values().min_by_key(|blocker| blocker.sequence);
    state.blocked_by = oldest.map(|blocker| Blocker {
        reviewer: blocker.reviewer.clone(),
        head_sha: blocker.head_sha.clone(),
        comment_url: blocker.comment_url.clone(),
    });
    state.blocked_sequence = oldest.map(|blocker| blocker.sequence);
    state.step = if state.blocked_by.is_none() {
        TaskStep::Merge
    } else {
        TaskStep::Review
    };
}

/// #460: the slice of its task a record belongs to (the index into `slices`). An issue can be
/// worked in several PRs; a claim on a branch no slice holds opens a slice, which that lane's next
/// `pr_opened` joins; every later record names its PR and lands in that PR's slice, so a merge
/// ends only its own.
fn slice_for(slices: &mut Vec<Slice>, next_id: &mut usize, event: &TaskEventRecord) -> usize {
    // Merge records may use `pr-N` as their task id even when the author claimed `issue-N`.
    // The PR identifies the existing slice across those task ids.
    let by_pr = event.pr.and_then(|pr| {
        let with_head = event.head_sha.is_some()
            && matches!(
                event.kind,
                TaskEventKind::ReviewVerdict | TaskEventKind::ReviewAssigned
            );
        if with_head {
            slices
                .iter()
                .position(|s| {
                    s.state.pr == Some(pr)
                        && same_opt_head(s.state.head_sha.as_ref(), event.head_sha.as_ref())
                })
                .or_else(|| slices.iter().position(|s| s.state.pr == Some(pr)))
        } else {
            slices.iter().position(|s| s.state.pr == Some(pr))
        }
    });
    // A lane may claim work for a PR another slice already holds. Its pr_opened belongs to
    // that lane's new claim; the PR's eventual merge/close then ends both claims together.
    if let Some(index) = by_pr {
        if event.kind != TaskEventKind::PrOpened {
            return index;
        }
        let held = &slices[index].state;
        if held.lane == event.lane && held.task_id == event.task_id {
            return index;
        }
    }
    let group: Vec<usize> = slices
        .iter()
        .enumerate()
        .filter(|(_, s)| s.state.task_id == event.task_id)
        .map(|(i, _)| i)
        .collect();
    let open = |s: &TaskState| {
        s.pr.is_none()
            && s.step != TaskStep::Merged
            && s.released_by.is_none()
            && (s.lane.is_none() || event.lane.is_none() || s.lane == event.lane)
    };
    let mut add = |slices: &mut Vec<Slice>| {
        let id = *next_id;
        *next_id += 1;
        slices.push(Slice {
            id,
            state: new_state(event),
            ended: false,
            first_pr_opening: None,
            blockers: None,
        });
        slices.len() - 1
    };
    // Each claim has its own sequence, even when a lane reuses a branch.
    if event.kind == TaskEventKind::Claimed {
        // A critic round recorded before the claim belongs to this task: the claim adopts it (#562).
        // A plan recorded before its claim belongs to that same lane's claim (#880).
        let adopted = group
            .iter()
            .copied()
            .find(|&i| {
                let s = &slices[i].state;
                s.lane == event.lane
                    && s.plan.is_some()
                    && s.claim_sequence.is_none()
                    && s.pr.is_none()
                    && s.last_sequence + 1 == event.sequence
            })
            .or_else(|| {
                group
                    .iter()
                    .copied()
                    .find(|&i| is_unclaimed_critic(&slices[i].state))
            });
        if let Some(index) = adopted {
            return index;
        }
        return add(slices);
    }
    // A PR no slice holds yet joins the latest open claim of its task (for pr_opened, the same
    // lane's). A merge recorded with no pr_opened still lands on the issue's claim instead of
    // opening a second graph.
    // #480: a plan or a critic round is the recording lane's own record and names no PR, so it
    // belongs to that lane's newest open claim, never to another lane's claim of the same issue.
    if matches!(
        event.kind,
        TaskEventKind::Planned | TaskEventKind::CriticVerdict
    ) && event.lane.is_some()
    {
        let own: Vec<usize> = group
            .iter()
            .copied()
            .filter(|&i| slices[i].state.lane == event.lane)
            .collect();
        let mine = own
            .iter()
            .copied()
            .rev()
            .find(|&i| slices[i].state.pr.is_none() && slices[i].state.step != TaskStep::Merged)
            .or_else(|| {
                own.iter()
                    .copied()
                    .rev()
                    .find(|&i| slices[i].state.step != TaskStep::Merged)
            });
        if let Some(index) = mine {
            return index;
        }
        // A plan after merge starts a fresh slice for the lane, as a claim would. Critic rounds
        // with no live claim keep #562's unclaimed route.
        if event.kind == TaskEventKind::Planned
            && (own.is_empty()
                || own
                    .last()
                    .is_some_and(|&i| slices[i].state.step == TaskStep::Merged))
        {
            return add(slices);
        }
    }
    let claimed = if event.kind == TaskEventKind::PrOpened {
        group
            .iter()
            .copied()
            .rev()
            .find(|&i| open(&slices[i].state))
    } else {
        group.iter().copied().find(|&i| {
            let s = &slices[i].state;
            s.pr.is_none() && s.step != TaskStep::Merged && s.released_by.is_none()
        })
    };
    if let Some(index) = claimed {
        if event.pr.is_some() {
            slices[index].state.pr = event.pr;
        }
        return index;
    }
    if let Some(index) = by_pr {
        return index;
    }
    // A record without a PR (none is admitted today) stays with the newest slice of its task.
    if event.pr.is_none()
        && let Some(&last) = group.last()
    {
        return last;
    }
    add(slices)
}

fn issue_from_task_id(task_id: &str) -> Option<u64> {
    let digits = task_id.strip_prefix("issue-")?;
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    digits
        .parse::<u64>()
        .ok()
        .filter(|n| *n > 0 && *n <= 9_007_199_254_740_991)
}

/// Folds records in sequence order into one state per task slice (spec §7): a new head re-arms
/// review but keeps the red edge until a verdict lands on a newer head than the BLOCK's. Ended
/// (closed) slices are dropped; the rest come back ordered by their last record.
#[must_use]
pub fn fold_task_events(records: &[TaskEventRecord]) -> Vec<TaskState> {
    fold_task_events_with_routes(records).tasks
}

/// [`fold_task_events`] plus every slice and the route of every record (#1192).
#[must_use]
pub fn fold_task_events_with_routes(records: &[TaskEventRecord]) -> TaskFold {
    let mut ordered: Vec<&TaskEventRecord> = records.iter().collect();
    ordered.sort_by_key(|record| record.sequence);
    let mut slices: Vec<Slice> = Vec::new();
    let mut absorbed: Vec<Slice> = Vec::new();
    let mut routes: Vec<Route> = Vec::new();
    let mut next_id = 0_usize;
    for event in ordered {
        // #1469: the issue list belongs to no slice; `backlog` reads it.
        if event.kind == TaskEventKind::Queued {
            continue;
        }
        if event.kind == TaskEventKind::Heartbeat {
            let claim = slices.iter_mut().rev().find(|s| {
                s.state.task_id == event.task_id
                    && s.state.lane == event.lane
                    && s.state.released_by.is_none()
                    && s.state.claim_sequence.is_some_and(|c| c < event.sequence)
                    && !s.ended
            });
            if let Some(claim) = claim
                && claim.state.lane.as_deref() == Some(event.actor_id.as_str())
            {
                claim.state.last_heartbeat_at = event.occurred_at.clone();
                routes.push(Route {
                    sequence: event.sequence,
                    slice: claim.id,
                });
            }
            continue;
        }
        if event.kind == TaskEventKind::Released {
            let latest = slices.iter_mut().rev().find(|s| {
                s.state.task_id == event.task_id
                    && s.state.lane == event.lane
                    && s.state.pr.is_none()
                    && s.state.released_by.is_none()
                    && s.state.claim_sequence.is_some_and(|c| c < event.sequence)
            });
            if let Some(latest) = latest
                && latest.state.claim_sequence == event.claim_sequence
            {
                latest.state.released_by = event.released_by.clone().or_else(|| event.lane.clone());
                routes.push(Route {
                    sequence: event.sequence,
                    slice: latest.id,
                });
            }
            continue;
        }
        if event.kind == TaskEventKind::PrOpened {
            let existing = slices.iter().position(|s| {
                s.state.pr == event.pr
                    && s.state.task_id == event.task_id
                    && s.state.lane == event.lane
            });
            if let Some(existing) = existing
                && let Some(opened_at) = slices[existing].first_pr_opening
            {
                let mut index = slices.len();
                while index > 0 {
                    index -= 1;
                    if index == existing {
                        continue;
                    }
                    let claim = &slices[index].state;
                    if claim.task_id == event.task_id
                        && claim.lane == event.lane
                        && claim.pr.is_none()
                        && claim.released_by.is_none()
                        && claim.step != TaskStep::Merged
                        && claim
                            .claim_sequence
                            .is_some_and(|c| c > opened_at && c < event.sequence)
                    {
                        absorbed.push(slices.remove(index));
                    }
                }
            }
        }
        let mut index = slice_for(&mut slices, &mut next_id, event);
        if slices[index].blockers.is_none() {
            let mut blockers = BTreeMap::new();
            if let Some(blocked) = &slices[index].state.blocked_by {
                blockers.insert(
                    blocked.reviewer.clone(),
                    BlockerAt {
                        reviewer: blocked.reviewer.clone(),
                        head_sha: blocked.head_sha.clone(),
                        comment_url: blocked.comment_url.clone(),
                        sequence: slices[index].state.blocked_sequence.unwrap_or(0),
                    },
                );
            }
            slices[index].blockers = Some(blockers);
        }
        if event.kind == TaskEventKind::PrOpened
            && slices[index].first_pr_opening.is_none()
            && let Some(claim_sequence) = slices[index].state.claim_sequence
        {
            let mut at = slices.len();
            while at > 0 {
                at -= 1;
                if at == index {
                    continue;
                }
                let claim = &slices[at].state;
                if claim.task_id == event.task_id
                    && claim.lane == event.lane
                    && claim.pr.is_none()
                    && claim.released_by.is_none()
                    && claim.step != TaskStep::Merged
                    && claim.claim_sequence.is_some_and(|c| c < claim_sequence)
                {
                    absorbed.push(slices.remove(at));
                    if at < index {
                        index -= 1;
                    }
                }
            }
        }
        // A takeover: another lane opens a head on a PR of the same task. The PR has one owner,
        // so the superseded lane's slice of it is retired and the task shows one card.
        if event.kind == TaskEventKind::PrOpened && event.pr.is_some() {
            let mut at = slices.len();
            while at > 0 {
                at -= 1;
                let held = &slices[at];
                if at != index
                    && !held.ended
                    && held.state.step != TaskStep::Merged
                    && held.state.pr == event.pr
                    && held.state.task_id == event.task_id
                    && held.state.lane != event.lane
                {
                    absorbed.push(slices.remove(at));
                    if at < index {
                        index -= 1;
                    }
                }
            }
        }
        if event.kind == TaskEventKind::PrOpened && slices[index].first_pr_opening.is_none() {
            slices[index].first_pr_opening = Some(event.sequence);
        }
        routes.push(Route {
            sequence: event.sequence,
            slice: slices[index].id,
        });
        // A later opening by the same author revives this PR's closed slice. A merge remains
        // terminal.
        {
            let slice = &mut slices[index];
            if event.kind == TaskEventKind::PrOpened
                && slice.ended
                && slice.state.step != TaskStep::Merged
                && slice.state.pr == event.pr
                && slice.state.task_id == event.task_id
                && slice.state.lane == event.lane
                && event.lane.as_deref() == Some(event.actor_id.as_str())
            {
                slice.ended = false;
            }
            if slice.ended || slice.state.step == TaskStep::Merged {
                continue;
            }
        }
        let (state, blockers) = {
            let slice = &mut slices[index];
            (
                &mut slice.state,
                slice
                    .blockers
                    .as_mut()
                    .expect("blockers are created before the record is applied"),
            )
        };
        state.last_sequence = event.sequence;
        if state.lane.as_deref() == Some(event.actor_id.as_str()) {
            state.author_last_sequence = Some(event.sequence);
        }
        let before = state.step;
        let same_head_repeat = event.kind == TaskEventKind::PrOpened
            && same_opt_head(event.head_sha.as_ref(), state.head_sha.as_ref());
        match event.kind {
            TaskEventKind::Claimed => {
                state.assigned_by = event.assigned_by.clone();
                if event.branch.is_some() {
                    state.branch = event.branch.clone();
                }
                if event.parent.is_some() {
                    state.parent = event.parent;
                }
                if event.title.is_some() {
                    state.title = event.title.clone();
                }
                if event.summary.is_some() {
                    state.summary = event.summary.clone();
                }
                if let Some(repo) = &event.repo {
                    state.repo_url = Some(format!("https://github.com/{repo}"));
                }
                if event.issue.is_some() {
                    state.issue = event.issue;
                }
                if event.lane.is_some() {
                    state.lane = event.lane.clone();
                }
                if let Some(journeys) = &event.journeys
                    && !journeys.is_empty()
                {
                    state.journeys = journeys.clone();
                }
                if state.claim_sequence.is_none() {
                    state.claim_sequence = Some(event.sequence);
                    state.claimed_at = event.occurred_at.clone();
                    state.lease_seconds = event.lease_seconds;
                }
            }
            TaskEventKind::Planned => {
                if event.lane.is_some() {
                    state.lane = event.lane.clone();
                }
                if event.plan.is_some() {
                    state.plan = event.plan.clone();
                }
                // A plan recorded late (after the PR) informs the graph without moving it back.
                if matches!(state.step, TaskStep::Plan | TaskStep::Critic) {
                    state.step = if state
                        .plan
                        .as_ref()
                        .is_some_and(|plan| plan.critic.mode == "design")
                    {
                        TaskStep::Critic
                    } else {
                        TaskStep::Implement
                    };
                }
            }
            TaskEventKind::PrOpened => {
                if let Some(repo) = &event.repo {
                    state.repo_url = Some(format!("https://github.com/{repo}"));
                }
                if event.pr.is_some() {
                    state.pr = event.pr;
                }
                if event.lane.is_some() {
                    state.lane = event.lane.clone();
                }
                if event.title.is_some() {
                    state.pr_title = event.title.clone();
                }
                if event.summary.is_some() {
                    state.pr_summary = event.summary.clone();
                }
                if !same_opt_head(event.head_sha.as_ref(), state.head_sha.as_ref()) {
                    state.current_reviewer = None;
                    state.approved_with_risk = None;
                    blockers.clear();
                    state.blocked_by = None;
                    state.blocked_sequence = None;
                }
                if event.head_sha.is_some() {
                    state.head_sha = event.head_sha.clone();
                }
                if let Some(pending) = &state.pending_reviewer
                    && event
                        .head_sha
                        .as_deref()
                        .is_some_and(|head| same_head(&pending.head_sha, head))
                {
                    state.current_reviewer = Some(pending.reviewer.clone());
                    state.pending_reviewer = None;
                }
                // #577: an empty list on pr_opened does not erase the journeys named at claim.
                if let Some(journeys) = &event.journeys
                    && !journeys.is_empty()
                {
                    state.journeys = journeys.clone();
                }
                if !same_head_repeat {
                    state.step = TaskStep::Review;
                }
                // #514: a newer head after a BLOCK is the author's fix; its review is the re-review.
                if let Some(round) = state.rounds.last_mut()
                    && let Some(head) = &event.head_sha
                    && !same_head(head, &round.head_sha)
                {
                    round.fix_head = Some(head.clone());
                    round.fixed_at = event.occurred_at.clone();
                }
                // #608: the author answered the BLOCK with a newer head, so nothing is blocked now.
                if let Some(blocked) = &state.blocked_by
                    && let Some(head) = &event.head_sha
                    && !same_head(head, &blocked.head_sha)
                {
                    state.blocked_by = None;
                    state.blocked_sequence = None;
                }
                if let Some(head) = &event.head_sha {
                    push_unique(&mut state.recorded_heads, head);
                }
                // A verdict that arrived before this head's pr_opened (a back-fill) now speaks for
                // it.
                let mut kept = Vec::with_capacity(state.stray_verdicts.len());
                let mut apply = Vec::new();
                for stray in std::mem::take(&mut state.stray_verdicts) {
                    match stray {
                        StrayVerdict::Unrecorded {
                            ref head_sha,
                            ref record,
                            ..
                        } if state
                            .head_sha
                            .as_deref()
                            .is_some_and(|head| same_head(head_sha, head)) =>
                        {
                            apply.push(record.clone());
                        }
                        other => kept.push(other),
                    }
                }
                state.stray_verdicts = kept;
                for record in apply {
                    apply_verdict(state, &record, blockers);
                }
            }
            TaskEventKind::ReviewAssigned => {
                if let Some(reviewer) = &event.reviewer {
                    push_unique(&mut state.assigned_reviewers, reviewer);
                    push_unique(&mut state.reviewers, reviewer);
                }
                if state.head_sha.is_none() {
                    state.head_sha = event.head_sha.clone();
                }
                if same_opt_head(event.head_sha.as_ref(), state.head_sha.as_ref()) {
                    state.current_reviewer = event.reviewer.clone();
                } else if let (Some(head), Some(reviewer)) = (&event.head_sha, &event.reviewer)
                    && !state
                        .recorded_heads
                        .iter()
                        .any(|recorded| same_head(recorded, head))
                {
                    state.pending_reviewer = Some(PendingReviewer {
                        head_sha: head.clone(),
                        reviewer: reviewer.clone(),
                    });
                }
                if matches!(state.step, TaskStep::Implement | TaskStep::Plan) {
                    state.step = TaskStep::Review;
                }
            }
            TaskEventKind::ReviewVerdict => {
                // A verdict on a head other than the current one says nothing about the current
                // one.
                if same_opt_head(event.head_sha.as_ref(), state.head_sha.as_ref()) {
                    apply_verdict(state, event, blockers);
                } else {
                    let reviewer = event.reviewer.clone().unwrap_or_default();
                    let verdict = event
                        .verdict
                        .map(Verdict::as_str)
                        .unwrap_or_default()
                        .to_owned();
                    let head_sha = event.head_sha.clone().unwrap_or_default();
                    let superseded = event.head_sha.as_ref().is_some_and(|head| {
                        state
                            .recorded_heads
                            .iter()
                            .any(|recorded| same_head(recorded, head))
                    });
                    state.stray_verdicts.push(if superseded {
                        StrayVerdict::Superseded {
                            reviewer,
                            verdict,
                            head_sha,
                            superseded_by: state.head_sha.clone().unwrap_or_default(),
                        }
                    } else {
                        StrayVerdict::Unrecorded {
                            reviewer,
                            verdict,
                            head_sha,
                            record: Box::new(event.clone()),
                        }
                    });
                }
            }
            TaskEventKind::CriticVerdict => {
                if event.critic.is_some() {
                    state.critic = event.critic.clone();
                }
                // The round names who recorded it; an unclaimed task has no other source for it.
                if state.lane.is_none() {
                    state.lane = event.lane.clone();
                }
                // #480: a passing round ends the Critic step; revise or exhausted keeps it lit.
                if state.step == TaskStep::Critic
                    && state
                        .critic
                        .as_ref()
                        .is_some_and(|round| round.verdict == CriticOutcome::Pass)
                {
                    state.step = TaskStep::Implement;
                }
            }
            TaskEventKind::Merged => {
                if event.pr.is_some() {
                    state.pr = event.pr;
                }
                state.merge_sha = event.merge_sha.clone();
                state.completion_recorded = true;
                state.blocked_by = None;
                state.blocked_sequence = None;
                state.step = TaskStep::Merged;
            }
            TaskEventKind::Closed => {}
            TaskEventKind::Heartbeat | TaskEventKind::Released | TaskEventKind::Queued => {
                unreachable!("handled above")
            }
        }
        // #480: Plan and Critic hold time only once a plan is recorded. Before that, the claim's
        // time is Implement's. Judged after the record is applied, so the `task.planned` that ends
        // Plan books the claim's time as Plan's.
        let plan_missing = state.plan.is_none();
        let timed = |step: TaskStep| -> &'static str {
            if matches!(step, TaskStep::Plan | TaskStep::Critic) && plan_missing {
                "implement"
            } else {
                step.as_str()
            }
        };
        let after = state.step;
        let resumes_after_gap =
            (matches!(event.kind, TaskEventKind::Claimed | TaskEventKind::Planned)
                || (event.kind == TaskEventKind::PrOpened && !same_head_repeat && before == after))
                && event.lane == state.lane;
        clock_step(
            &mut state.clock,
            timed(before),
            timed(after),
            event.occurred_at.clone(),
            resumes_after_gap,
        );
        if event.kind == TaskEventKind::Closed {
            slices[index].ended = true;
        }
        if matches!(event.kind, TaskEventKind::Merged | TaskEventKind::Closed) {
            for (at, claim) in slices.iter_mut().enumerate() {
                if at == index
                    || claim.state.pr != event.pr
                    || claim.state.step == TaskStep::Merged
                    || claim.ended
                {
                    continue;
                }
                let claim_before = claim.state.step;
                claim.state.last_sequence = event.sequence;
                if event.kind == TaskEventKind::Closed {
                    claim.ended = true;
                } else {
                    claim.state.step = TaskStep::Merged;
                    claim.state.merge_sha = event.merge_sha.clone();
                    claim.state.blocked_by = None;
                }
                let claim_after = claim.state.step;
                clock_step(
                    &mut claim.state.clock,
                    timed(claim_before),
                    timed(claim_after),
                    event.occurred_at.clone(),
                    false,
                );
            }
        }
        if event.kind == TaskEventKind::Merged
            && let Some(closes) = &event.closes
        {
            for (at, claim) in slices.iter_mut().enumerate() {
                let claim_issue = claim
                    .state
                    .issue
                    .or_else(|| issue_from_task_id(&claim.state.task_id));
                if at != index
                    && claim.state.step != TaskStep::Merged
                    && claim.state.pr != event.pr
                    && claim_issue.is_some_and(|issue| closes.contains(&issue))
                {
                    let claim_before = claim.state.step;
                    claim.state.step = TaskStep::Merged;
                    claim.state.merge_sha = event.merge_sha.clone();
                    claim.state.blocked_by = None;
                    claim.state.last_sequence = event.sequence;
                    clock_step(
                        &mut claim.state.clock,
                        timed(claim_before),
                        "merged",
                        event.occurred_at.clone(),
                        false,
                    );
                }
            }
        }
    }
    for index in 0..slices.len() {
        // A slice opened by a PR record (no claim seen) still belongs to its issue.
        if slices[index].state.issue.is_none() {
            let task_id = slices[index].state.task_id.clone();
            slices[index].state.issue = slices
                .iter()
                .find(|other| other.state.task_id == task_id && other.state.issue.is_some())
                .and_then(|other| other.state.issue);
        }
        let state = &mut slices[index].state;
        if state.issue.is_none() && !is_unclaimed_critic(state) {
            state.issue = issue_from_task_id(&state.task_id);
        }
        state.key = match state.pr {
            Some(pr) => format!("{}#pr-{pr}", state.task_id),
            None => format!("{}#claim-{index}", state.task_id),
        };
    }
    let mut tasks: Vec<TaskState> = slices
        .iter()
        .filter(|s| !s.ended)
        .map(|s| s.state.clone())
        .collect();
    tasks.sort_by_key(|state| state.last_sequence);
    let mut all: Vec<FoldSlice> = slices
        .into_iter()
        .map(|s| FoldSlice {
            id: s.id,
            ended: s.ended,
            absorbed: false,
            state: s.state,
        })
        .chain(absorbed.into_iter().map(|s| FoldSlice {
            id: s.id,
            ended: s.ended,
            absorbed: true,
            state: s.state,
        }))
        .collect();
    all.sort_by_key(|slice| slice.id);
    TaskFold {
        tasks,
        slices: all,
        routes,
    }
}
