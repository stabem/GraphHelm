//! The `skill.loaded` records of one execution, read from a Runtime for `keel check`.
//!
//! The comparison with a card's skills line is pure and lives in `graphhelm_policy::keel`; this
//! file only fetches. A binding that is missing, a Runtime that does not answer in time, a page
//! or evidence item that cannot be read: all give `Unobserved` with a fixed reason, so the check
//! never warns about a skill from a partial list and never fails because of the network. Reasons
//! carry no URL, token path or transport text.
use std::path::PathBuf;
use std::time::{Duration, Instant};

use graphhelm_policy::keel::{PrOpened, SkillHistory, SkillRecord, TaskClaim};
use zeroize::Zeroizing;

use super::mcp::client::{ApiClient, TokenSource, is_loopback_url};
use super::mcp::url::{query_value, segment_path};

const DEADLINE: Duration = Duration::from_secs(20);
const EVENTS_PAGE: u64 = 1000;
const MAX_PAGES: u32 = 200;
const EVIDENCE_BATCH: usize = 64;
const DEFAULT_URL: &str = "http://127.0.0.1:8791";

/// `--runtime-url`, `--runtime-token-file`, `--runtime-execution`; each falls back to the variable
/// the hooks read (`GRAPHHELM_RUNTIME_URL`, `GRAPHHELM_TOKEN_FILE`, `GRAPHHELM_EXECUTION_ID`).
#[derive(Default)]
pub(super) struct RuntimeBinding {
    pub(super) url: Option<String>,
    pub(super) token_file: Option<PathBuf>,
    pub(super) execution: Option<String>,
}

pub(super) enum LoadedSkills {
    History(SkillHistory),
    Unobserved(&'static str),
}

pub(super) fn loaded_skills(binding: &RuntimeBinding) -> LoadedSkills {
    let execution = binding
        .execution
        .clone()
        .or_else(|| std::env::var("GRAPHHELM_EXECUTION_ID").ok())
        .filter(|value| !value.is_empty());
    let token_file = binding
        .token_file
        .clone()
        .or_else(|| std::env::var_os("GRAPHHELM_TOKEN_FILE").map(PathBuf::from))
        .filter(|path| !path.as_os_str().is_empty());
    let (Some(execution), Some(token_file)) = (execution, token_file) else {
        return LoadedSkills::Unobserved(
            "OBSERVER_MISSING: no Runtime binding (execution and token file), so the skills line \
             was not compared with the skill.loaded records",
        );
    };
    let url = binding
        .url
        .clone()
        .or_else(|| std::env::var("GRAPHHELM_RUNTIME_URL").ok())
        .unwrap_or_else(|| DEFAULT_URL.to_owned());
    let valid_id = execution.len() <= 128
        && execution
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"._:-".contains(&c));
    if !valid_id || !is_loopback_url(&url) {
        return LoadedSkills::Unobserved(
            "OBSERVER_MISSING: the Runtime binding is not usable (execution id or loopback URL)",
        );
    }
    let Ok(token) = super::secret_file::read_existing(&token_file, "agent token") else {
        return LoadedSkills::Unobserved("OBSERVER_MISSING: the token file could not be read");
    };
    let actor = std::env::var("GRAPHHELM_ACTOR")
        .or_else(|_| std::env::var("GRAPHHELM_NODE_ID"))
        .unwrap_or_else(|_| "keel-check".to_owned());
    let client = ApiClient::new(
        url,
        TokenSource::Fixed(Zeroizing::new(token)),
        actor,
        "agent".to_owned(),
        None,
        None,
        "keel-check-skills".to_owned(),
    );
    match read_records(&client, &execution) {
        // Events but not one `skill.loaded`: the host never ran the recorder, so no skill can be
        // called not loaded, whichever lane the identity line names.
        Some(history) if history.records.is_empty() => LoadedSkills::Unobserved(
            "OBSERVER_MISSING: the execution has events but no skill.loaded record, so its host \
             does not run the skill recorder (plugins/graphhelm/hooks/skill_record_hook.py)",
        ),
        Some(history) => LoadedSkills::History(history),
        None => LoadedSkills::Unobserved(
            "OBSERVER_MISSING: the Runtime did not give a complete skill.loaded list in time",
        ),
    }
}

/// `None` unless every page of events and every referenced evidence item was read.
fn read_records(client: &ApiClient, execution: &str) -> Option<SkillHistory> {
    let started = Instant::now();
    let get = |path: &str| -> Option<serde_json::Value> {
        let left = DEADLINE.checked_sub(started.elapsed())?;
        // The key makes the client attach the actor headers, which a lane credential needs.
        match client.request_waiting("GET", path, None, Some("keel-check-skills"), None, left) {
            Ok((200, reply)) if reply["ok"] == true => Some(reply["data"].clone()),
            _ => None,
        }
    };
    let events_path = segment_path(&["v1", "executions", execution, "events"]);
    // The evidence of each `skill.loaded`, `task.claimed` and `task.pr_opened` event, in order.
    let mut pending: Vec<Pending> = Vec::new();
    let mut history = SkillHistory::default();
    let mut after = 0_u64;
    for _ in 0..MAX_PAGES {
        let page = get(&format!("{events_path}?after={after}&limit={EVENTS_PAGE}"))?;
        let head = page["head"].as_u64()?;
        let events = page["events"].as_array()?;
        // The Runtime answers `head` 0 with no events both for an execution it does not know and
        // for a stream with nothing in it. A started execution always has events, so head 0 is
        // no observation, never an empty complete list.
        if head == 0 {
            return None;
        }
        for event in events {
            let sequence = event["sequence"].as_u64()?;
            // An event whose writer cannot be named leaves the history incomplete.
            let actor = event["actor"]["id"].as_str()?;
            history.actors.insert(actor.to_owned());
            if event["kind"]["type"] == "signal_recorded" {
                let kind = match event["kind"]["data"]["kind"].as_str() {
                    Some("skill.loaded") => Some(Kind::Skill),
                    Some("task.claimed") => Some(Kind::Claim),
                    Some("task.pr_opened") => Some(Kind::PrOpened),
                    _ => None,
                };
                if let Some(kind) = kind {
                    // A record whose content cannot be named leaves the history incomplete.
                    let id = event["evidenceRefs"][0]["evidenceId"].as_str()?;
                    pending.push(Pending {
                        id: id.to_owned(),
                        actor: actor.to_owned(),
                        sequence,
                        kind,
                    });
                }
            }
            after = after.max(sequence);
        }
        if after >= head {
            read_records_content(&get, execution, &pending, &mut history)?;
            return Some(history);
        }
        if events.is_empty() {
            return None;
        }
    }
    None
}

enum Kind {
    Skill,
    Claim,
    PrOpened,
}

struct Pending {
    id: String,
    actor: String,
    sequence: u64,
    kind: Kind,
}

/// Fills `history` from the evidence of each pending event; `None` unless all of it was read.
fn read_records_content(
    get: &dyn Fn(&str) -> Option<serde_json::Value>,
    execution: &str,
    pending: &[Pending],
    history: &mut SkillHistory,
) -> Option<()> {
    let evidence_path = segment_path(&["v1", "executions", execution, "evidence"]);
    for batch in pending.chunks(EVIDENCE_BATCH) {
        let ids: Vec<&str> = batch.iter().map(|item| item.id.as_str()).collect();
        let ids = serde_json::to_string(&ids).ok()?;
        let reply = get(&format!("{evidence_path}?ids={}", query_value(&ids)))?;
        let results = reply["results"].as_array()?;
        if results.len() != batch.len() {
            return None;
        }
        for item in results {
            if item["status"] != "available" {
                return None;
            }
            // Each result is matched to its event by the evidence id it carries, never by
            // position; a result for an id that was not asked for leaves the list incomplete.
            let event = batch
                .iter()
                .find(|event| item["evidenceId"].as_str() == Some(event.id.as_str()))?;
            let signal: serde_json::Value = serde_json::from_str(item["content"].as_str()?).ok()?;
            // A task record's description is the JSON the task recorder wrote; fields it lacks
            // stay unnamed, and the check then falls back to the lane's newest claim.
            let task_doc = || -> serde_json::Value {
                signal["description"]
                    .as_str()
                    .and_then(|text| serde_json::from_str(text).ok())
                    .unwrap_or_default()
            };
            match event.kind {
                Kind::Claim => {
                    history.claims.push(TaskClaim {
                        actor: event.actor.clone(),
                        sequence: event.sequence,
                        issue: task_doc()["issue"].as_u64(),
                    });
                    continue;
                }
                Kind::PrOpened => {
                    if let Some(head_sha) = task_doc()["headSha"].as_str() {
                        history.pr_opens.push(PrOpened {
                            actor: event.actor.clone(),
                            sequence: event.sequence,
                            head_sha: head_sha.to_owned(),
                        });
                    }
                    continue;
                }
                Kind::Skill => {}
            }
            // A record that does not carry the hook's `Skill loaded: <name>` text names no skill.
            let Some(name) = signal["description"]
                .as_str()
                .and_then(|text| text.strip_prefix("Skill loaded: "))
            else {
                continue;
            };
            let line = |key: &str| {
                signal["evidence"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(serde_json::Value::as_str)
                    .find_map(|text| text.strip_prefix(key))
                    .map(|value| value.trim().to_owned())
            };
            history.records.push(SkillRecord {
                name: name.trim().to_owned(),
                actor: event.actor.clone(),
                session: line("session: "),
                agent: line("agent: "),
                sequence: event.sequence,
            });
        }
    }
    Some(())
}
