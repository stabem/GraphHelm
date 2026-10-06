//! Owner-only records: `actor_alias` names an agent of this run, `owner_refusal` declines an
//! agent's signal. Both are sealed owner signals validated before any evidence write or append.
use std::collections::BTreeSet;

use graphhelm_protocols::{EventEnvelope, EventKind, PersistedActor, PersistedActorType};
use serde::Deserialize;

use super::Failure;

const ALIAS_PROTOCOL: &str = "graphhelm-actor-alias-v1";
const REFUSAL_PROTOCOL: &str = "graphhelm-owner-refusal-v1";
/// The shared native identity is one actor for many conversations; it cannot carry one name.
const SHARED_AGENT: &str = "codex";

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ActorAlias {
    protocol: String,
    display_name: String,
    #[serde(default)]
    persona_thread_id: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct OwnerRefusal {
    protocol: String,
    #[serde(default)]
    reason: Option<String>,
}

/// Returns `Ok(())` for every kind other than `actor_alias` and `owner_refusal`.
pub(super) fn validate_owner_record(
    envelope: &serde_json::Value,
    actor: &PersistedActor,
    sealed: bool,
    history: &[EventEnvelope],
) -> Result<(), Failure> {
    let kind = envelope["type"].as_str().unwrap_or("");
    if !matches!(kind, "actor_alias" | "owner_refusal") {
        return Ok(());
    }
    let mut agents = BTreeSet::new();
    let mut agent_signals = BTreeSet::new();
    for event in history {
        if event.actor.actor_type() != PersistedActorType::Agent {
            continue;
        }
        agents.insert(event.actor.id().to_string());
        if let EventKind::SignalRecorded(signal) = &event.kind {
            agent_signals.insert(signal.signal_id.to_string());
        }
    }
    check(envelope, actor, sealed, &agents, &agent_signals)
}

fn check(
    envelope: &serde_json::Value,
    actor: &PersistedActor,
    sealed: bool,
    agents: &BTreeSet<String>,
    agent_signals: &BTreeSet<String>,
) -> Result<(), Failure> {
    let kind = envelope["type"].as_str().unwrap_or("");
    let invalid = || {
        super::signal_invalid(
            "owner records require a sealed, bounded owner signal about this run",
            "/signal",
        )
    };
    if !sealed
        || actor.actor_type() != PersistedActorType::Owner
        || envelope
            .pointer("/source/type")
            .and_then(serde_json::Value::as_str)
            != Some("user")
    {
        return Err(invalid());
    }
    let text = envelope["description"].as_str().ok_or_else(invalid)?;
    if text.len() > 8192 {
        return Err(invalid());
    }
    let secret = graphhelm_runtime::context::secret_shaped;
    if kind == "actor_alias" {
        let to = envelope["to"].as_str().ok_or_else(invalid)?;
        if to == SHARED_AGENT || !agents.contains(to) {
            return Err(invalid());
        }
        let alias: ActorAlias = serde_json::from_str(text).map_err(|_| invalid())?;
        let name = alias.display_name.trim();
        let name_length = name.chars().count();
        if alias.protocol != ALIAS_PROTOCOL
            || !(1..=80).contains(&name_length)
            || name.chars().any(char::is_control)
            || secret(name)
            || alias
                .persona_thread_id
                .as_deref()
                .is_some_and(|id| !(1..=128).contains(&id.chars().count()) || secret(id))
        {
            return Err(invalid());
        }
    } else {
        let reply_to = envelope["replyTo"].as_str().ok_or_else(invalid)?;
        if !agent_signals.contains(reply_to) {
            return Err(invalid());
        }
        let refusal: OwnerRefusal = serde_json::from_str(text).map_err(|_| invalid())?;
        if refusal.protocol != REFUSAL_PROTOCOL
            || refusal
                .reason
                .as_deref()
                .is_some_and(|reason| reason.len() > 2048 || secret(reason))
        {
            return Err(invalid());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use graphhelm_protocols::ActorId;
    use serde_json::json;

    use super::*;

    fn actor(actor_type: PersistedActorType, id: &str) -> PersistedActor {
        PersistedActor::new(actor_type, ActorId::parse(id).unwrap())
    }

    fn owner() -> PersistedActor {
        actor(PersistedActorType::Owner, "owner-cli")
    }

    fn set(items: &[&str]) -> BTreeSet<String> {
        items.iter().map(|item| (*item).to_owned()).collect()
    }

    fn alias(to: &str, description: &serde_json::Value) -> serde_json::Value {
        json!({"id":"alias-1","type":"actor_alias","source":{"type":"user","id":"owner"},
            "to":to,"description":description.to_string()})
    }

    fn refusal(reply_to: Option<&str>, description: &serde_json::Value) -> serde_json::Value {
        let mut value = json!({"id":"refusal-1","type":"owner_refusal",
            "source":{"type":"user","id":"owner"},"description":description.to_string()});
        if let Some(reply_to) = reply_to {
            value["replyTo"] = json!(reply_to);
        }
        value
    }

    fn named(name: &str) -> serde_json::Value {
        json!({"protocol":ALIAS_PROTOCOL,"displayName":name})
    }

    fn run(value: &serde_json::Value, by: &PersistedActor, sealed: bool) -> Result<(), Failure> {
        check(
            value,
            by,
            sealed,
            &set(&["claude-7", "codex"]),
            &set(&["signal-agent-1"]),
        )
    }

    #[test]
    fn other_kinds_pass_through() {
        let value = json!({"type":"operator_note","description":"x"});
        assert!(validate_owner_record(&value, &owner(), false, &[]).is_ok());
    }

    #[test]
    fn an_agent_cannot_alias_any_agent() {
        let agent = actor(PersistedActorType::Agent, "claude-7");
        assert!(run(&alias("claude-7", &named("Planner")), &agent, true).is_err());
    }

    #[test]
    fn the_shared_codex_identity_cannot_be_aliased() {
        assert!(run(&alias("codex", &named("Planner")), &owner(), true).is_err());
    }

    #[test]
    fn an_actor_absent_from_the_run_cannot_be_aliased() {
        assert!(run(&alias("claude-9", &named("Planner")), &owner(), true).is_err());
        // The full entry point with an empty history sees no agent at all.
        assert!(
            validate_owner_record(&alias("claude-7", &named("Planner")), &owner(), true, &[])
                .is_err()
        );
    }

    #[test]
    fn a_valid_alias_is_accepted() {
        assert!(run(&alias("claude-7", &named(" Planner ")), &owner(), true).is_ok());
        let with_thread =
            json!({"protocol":ALIAS_PROTOCOL,"displayName":"Planner","personaThreadId":"t-1"});
        assert!(run(&alias("claude-7", &with_thread), &owner(), true).is_ok());
    }

    #[test]
    fn unsealed_or_non_user_source_is_refused() {
        assert!(run(&alias("claude-7", &named("Planner")), &owner(), false).is_err());
        let mut value = alias("claude-7", &named("Planner"));
        value["source"]["type"] = json!("node");
        assert!(run(&value, &owner(), true).is_err());
    }

    #[test]
    fn blank_long_unknown_or_wrong_protocol_alias_is_refused() {
        for description in [
            named("   "),
            named(""),
            named(&"n".repeat(81)),
            named("bad\nname"),
            json!({"protocol":ALIAS_PROTOCOL,"displayName":"Planner","extra":true}),
            json!({"protocol":"other","displayName":"Planner"}),
            json!({"protocol":ALIAS_PROTOCOL,"displayName":"Planner","personaThreadId":""}),
        ] {
            assert!(
                run(&alias("claude-7", &description), &owner(), true).is_err(),
                "{description}"
            );
        }
    }

    #[test]
    fn refusal_needs_reply_to_an_agent_signal_of_this_run() {
        let body = json!({"protocol":REFUSAL_PROTOCOL,"reason":"Not now"});
        assert!(run(&refusal(None, &body), &owner(), true).is_err());
        assert!(run(&refusal(Some("signal-unknown"), &body), &owner(), true).is_err());
        let agent = actor(PersistedActorType::Agent, "claude-7");
        assert!(run(&refusal(Some("signal-agent-1"), &body), &agent, true).is_err());
        let unknown = json!({"protocol":REFUSAL_PROTOCOL,"extra":1});
        assert!(run(&refusal(Some("signal-agent-1"), &unknown), &owner(), true).is_err());
        let long = json!({"protocol":REFUSAL_PROTOCOL,"reason":"r".repeat(2049)});
        assert!(run(&refusal(Some("signal-agent-1"), &long), &owner(), true).is_err());
    }

    fn signal_event(by: &PersistedActor, signal_id: &str) -> EventEnvelope {
        use chrono::{TimeZone, Utc};
        use graphhelm_protocols::{
            EventHash, ExecutionId, NewEvent, OpaqueId, PersistedTimestamp, ProjectId, RawSha256,
            RepositoryScope, Sensitivity, SignalRecorded, SignalSeverity, SignalSourceKind,
            WorkspaceId,
        };
        let genesis = EventHash::parse(
            "sha256:35c8ab0717bef1684ad07efcf3bedd4648c778a2c944cbd2c7e6a4802e2237b3",
        )
        .unwrap();
        EventEnvelope::new(
            OpaqueId::parse(format!("event-{signal_id}")).unwrap(),
            RepositoryScope::new(
                WorkspaceId::parse("workspace-1").unwrap(),
                ProjectId::parse("project-1").unwrap(),
                Some(ExecutionId::parse("run-1").unwrap()),
            ),
            OpaqueId::parse("run-1").unwrap(),
            1,
            PersistedTimestamp::from_datetime(Utc.with_ymd_and_hms(2026, 10, 5, 12, 0, 0).unwrap())
                .unwrap(),
            NewEvent::new(
                OpaqueId::parse(format!("request-{signal_id}")).unwrap(),
                by.clone(),
                Sensitivity::Internal,
                EventKind::SignalRecorded(SignalRecorded {
                    scoped_agent_authenticated: None,
                    execution_id: OpaqueId::parse("run-1").unwrap(),
                    signal_id: OpaqueId::parse(signal_id).unwrap(),
                    source_kind: SignalSourceKind::User,
                    source_id: OpaqueId::parse("source-1").unwrap(),
                    kind: "operator_note".to_owned(),
                    severity: SignalSeverity::Low,
                    envelope_sha256: RawSha256::parse("a".repeat(64)).unwrap(),
                }),
                vec![],
                vec![],
            ),
            genesis.clone(),
            genesis,
        )
    }

    #[test]
    fn history_decides_present_agents_and_answerable_signals() {
        let agent = actor(PersistedActorType::Agent, "claude-7");
        let history = [
            signal_event(&agent, "signal-agent-1"),
            signal_event(&owner(), "signal-owner-1"),
        ];
        let body = json!({"protocol":REFUSAL_PROTOCOL});
        let validate = |value: &serde_json::Value| {
            validate_owner_record(value, &owner(), true, &history).is_ok()
        };
        assert!(validate(&alias("claude-7", &named("Planner"))));
        assert!(!validate(&alias("owner-cli", &named("Planner"))));
        assert!(validate(&refusal(Some("signal-agent-1"), &body)));
        assert!(!validate(&refusal(Some("signal-owner-1"), &body)));
        assert!(!validate(&refusal(Some("signal-unknown"), &body)));
    }

    #[test]
    fn a_valid_refusal_is_accepted() {
        let body = json!({"protocol":REFUSAL_PROTOCOL,"reason":"Not now"});
        assert!(run(&refusal(Some("signal-agent-1"), &body), &owner(), true).is_ok());
        let bare = json!({"protocol":REFUSAL_PROTOCOL});
        assert!(run(&refusal(Some("signal-agent-1"), &bare), &owner(), true).is_ok());
    }
}
