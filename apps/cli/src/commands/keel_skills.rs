//! The `skill.loaded` records of one execution, read from a Runtime for `keel check`.
//!
//! The comparison with a card's skills line is pure and lives in `graphhelm_policy::keel`; this
//! file only fetches. A binding that is missing, a Runtime that does not answer in time, a page
//! or evidence item that cannot be read: all give `Unobserved` with a fixed reason, so the check
//! never warns about a skill from a partial list and never fails because of the network. Reasons
//! carry no URL, token path or transport text.
use std::collections::BTreeSet;
use std::path::PathBuf;
use std::time::{Duration, Instant};

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
    Names(BTreeSet<String>),
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
        // called not loaded.
        Some((0, _)) => LoadedSkills::Unobserved(
            "OBSERVER_MISSING: the execution has events but no skill.loaded record, so its host \
             does not run the skill recorder (plugins/graphhelm/hooks/skill_record_hook.py)",
        ),
        Some((_, names)) => LoadedSkills::Names(names),
        None => LoadedSkills::Unobserved(
            "OBSERVER_MISSING: the Runtime did not give a complete skill.loaded list in time",
        ),
    }
}

/// `None` unless every page of events and every referenced evidence item was read. The count is
/// the number of `skill.loaded` records seen.
fn read_records(client: &ApiClient, execution: &str) -> Option<(usize, BTreeSet<String>)> {
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
    let mut evidence_ids: Vec<String> = Vec::new();
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
            if event["kind"]["type"] == "signal_recorded"
                && event["kind"]["data"]["kind"] == "skill.loaded"
            {
                // A record whose content cannot be named leaves the list incomplete.
                evidence_ids.push(event["evidenceRefs"][0]["evidenceId"].as_str()?.to_owned());
            }
            after = after.max(event["sequence"].as_u64()?);
        }
        if after >= head {
            let names = read_names(&get, execution, &evidence_ids)?;
            return Some((evidence_ids.len(), names));
        }
        if events.is_empty() {
            return None;
        }
    }
    None
}

fn read_names(
    get: &dyn Fn(&str) -> Option<serde_json::Value>,
    execution: &str,
    evidence_ids: &[String],
) -> Option<BTreeSet<String>> {
    let evidence_path = segment_path(&["v1", "executions", execution, "evidence"]);
    let mut names = BTreeSet::new();
    for batch in evidence_ids.chunks(EVIDENCE_BATCH) {
        let ids = serde_json::to_string(batch).ok()?;
        let reply = get(&format!("{evidence_path}?ids={}", query_value(&ids)))?;
        let results = reply["results"].as_array()?;
        if results.len() != batch.len() {
            return None;
        }
        for item in results {
            if item["status"] != "available" {
                return None;
            }
            // A record that does not carry the hook's `Skill loaded: <name>` text names no skill.
            let signal: serde_json::Value = serde_json::from_str(item["content"].as_str()?).ok()?;
            if let Some(name) = signal["description"]
                .as_str()
                .and_then(|text| text.strip_prefix("Skill loaded: "))
            {
                names.insert(name.trim().to_owned());
            }
        }
    }
    Some(names)
}
