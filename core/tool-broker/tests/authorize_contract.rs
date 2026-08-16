//! The pure §11.2 decision pipeline: identity → capability → program allowlist → effect → tier.
//! Every refusal is typed and content-free; deny-by-default is the shape of the lease, not a
//! setting.

use graphhelm_tool_broker::call::{RepositoryAction, ShellAction, TestsAction, ToolCall};
use graphhelm_tool_broker::effect::IsolationTier;
use graphhelm_tool_broker::lease::{BrokerRefusal, Capability, ToolLease, authorize};
use graphhelm_tool_broker::path::RelativePath;

fn full_lease(actor: &str) -> ToolLease {
    ToolLease {
        actor: actor.to_owned(),
        capabilities: [
            Capability::RepositoryRead,
            Capability::RepositoryWrite,
            Capability::ShellExecute,
            Capability::TestsExecute,
        ]
        .into_iter()
        .collect(),
        programs: ["git", "cargo"].map(str::to_owned).into_iter().collect(),
    }
}

fn read_call() -> ToolCall {
    ToolCall::Repository(RepositoryAction::ReadFile {
        path: RelativePath::parse("src/lib.rs").unwrap(),
    })
}

fn shell_call(program: &str) -> ToolCall {
    ToolCall::Shell(ShellAction {
        program: program.to_owned(),
        arguments: vec!["status".to_owned()],
    })
}

#[test]
fn a_read_call_routes_to_tier_0_and_a_write_call_to_tier_1() {
    let lease = full_lease("agent-builder");
    let read = authorize(&read_call(), &lease, "agent-builder").unwrap();
    assert_eq!(read.tier, IsolationTier::Tier0);

    let write = authorize(
        &ToolCall::Repository(RepositoryAction::ApplyPatch {
            patch: "diff".into(),
        }),
        &lease,
        "agent-builder",
    )
    .unwrap();
    assert_eq!(write.tier, IsolationTier::Tier1);
}

#[test]
fn the_actor_must_match_the_lease() {
    let lease = full_lease("agent-builder");
    assert!(matches!(
        authorize(&read_call(), &lease, "agent-impostor").unwrap_err(),
        BrokerRefusal::ActorMismatch { .. }
    ));
}

#[test]
fn a_malformed_actor_is_refused_before_any_other_check() {
    // Review finding 5: ActorInvalid existed with no test. Charset is [a-z][a-z0-9-]{0,63};
    // the empty string, uppercase, and separators are all outside it. The lease NAMES the
    // same bad actor, so the only refusal that can fire is the validity check itself —
    // pinning that it runs before the ActorMismatch comparison.
    for bad in [
        "",
        "Agent-Builder",
        "agent_builder",
        "agent builder",
        "1agent",
    ] {
        let lease = full_lease(bad);
        assert!(
            matches!(
                authorize(&read_call(), &lease, bad).unwrap_err(),
                BrokerRefusal::ActorInvalid
            ),
            "{bad:?} must be ActorInvalid"
        );
    }
}

#[test]
fn a_missing_capability_is_denied_by_default() {
    let mut lease = full_lease("agent-builder");
    lease.capabilities.remove(&Capability::ShellExecute);
    assert!(matches!(
        authorize(&shell_call("git"), &lease, "agent-builder").unwrap_err(),
        BrokerRefusal::CapabilityMissing { .. }
    ));
}

#[test]
fn a_program_outside_the_lease_allowlist_is_denied() {
    let lease = full_lease("agent-builder");
    assert!(matches!(
        authorize(&shell_call("curl"), &lease, "agent-builder").unwrap_err(),
        BrokerRefusal::ProgramDenied
    ));
}

#[test]
fn shell_and_tests_are_always_tier_1_even_for_an_innocent_looking_program() {
    // A process can write; the broker cannot know less. Deny-by-default means classifying
    // every spawned program as ReversibleWrite, so no shell call ever lands in Tier 0.
    let lease = full_lease("agent-builder");
    let shell = authorize(&shell_call("git"), &lease, "agent-builder").unwrap();
    assert_eq!(shell.tier, IsolationTier::Tier1);
    let tests = authorize(
        &ToolCall::Tests(TestsAction {
            arguments: vec!["--lib".into()],
        }),
        &lease,
        "agent-builder",
    )
    .unwrap();
    assert_eq!(tests.tier, IsolationTier::Tier1);
}

#[test]
fn refusals_never_echo_call_arguments() {
    // A refusal names the rule and the tool, never content: a denied patch or argument list
    // must not travel into logs through the error path.
    let lease = full_lease("agent-builder");
    let sentinel = "SENTINEL-argument-value";
    let refusal = authorize(
        &ToolCall::Shell(ShellAction {
            program: "curl".to_owned(),
            arguments: vec![sentinel.to_owned()],
        }),
        &lease,
        "agent-builder",
    )
    .unwrap_err();
    let rendered = format!("{refusal} {refusal:?}");
    assert!(!rendered.contains(sentinel));
}

#[test]
fn an_unknown_field_in_a_serialized_call_is_refused() {
    // The plan's anticipated serde limitation is real, observed in this task's TDD red:
    // `deny_unknown_fields` cannot fire through internal tagging (the tag machinery buffers
    // and ignores leftovers), so plain `serde_json::from_str::<ToolCall>` ACCEPTS the
    // "extra" key. `ToolCall::from_json` is the checked trust-boundary door, and this test
    // pins both halves: the refusal, and that the happy shapes still parse through it.
    let json = r#"{"tool":"repository","action":"read_file","path":"src/lib.rs","extra":true}"#;
    assert!(matches!(
        ToolCall::from_json(json),
        Err(graphhelm_tool_broker::call::CallParseError::UnknownField)
    ));

    let clean = r#"{"tool":"repository","action":"read_file","path":"src/lib.rs"}"#;
    assert_eq!(ToolCall::from_json(clean).unwrap(), read_call());
    let shell = r#"{"tool":"shell","program":"git","arguments":["status"]}"#;
    assert_eq!(ToolCall::from_json(shell).unwrap(), shell_call("git"));
    // The smuggle paths stay closed on every shape, tagged or plain:
    for bad in [
        r#"{"tool":"shell","program":"git","cwd":"C:/"}"#,
        r#"{"tool":"tests","arguments":[],"runner":"sh"}"#,
        r#"{"tool":"repository","action":"diff","path":"x"}"#,
    ] {
        assert!(ToolCall::from_json(bad).is_err(), "{bad} must be refused");
    }
}

#[test]
fn a_commit_message_over_the_bound_or_with_control_bytes_is_refused() {
    // The message travels in argv (`git commit -m <message>`, Task 7): the bound keeps argv
    // small and printable, and control bytes have no business in a commit subject a broker
    // mints. 512 bytes exactly is accepted; 513 and an embedded newline are refused at the
    // trust-boundary door.
    use graphhelm_tool_broker::call::CallParseError;
    let ok = format!(
        r#"{{"tool":"repository","action":"commit","message":"{}"}}"#,
        "m".repeat(512)
    );
    assert!(ToolCall::from_json(&ok).is_ok());
    let long = format!(
        r#"{{"tool":"repository","action":"commit","message":"{}"}}"#,
        "m".repeat(513)
    );
    assert!(matches!(
        ToolCall::from_json(&long),
        Err(CallParseError::MessageBound)
    ));
    let control = r#"{"tool":"repository","action":"commit","message":"a\nb"}"#;
    assert!(matches!(
        ToolCall::from_json(control),
        Err(CallParseError::MessageBound)
    ));
}

#[test]
fn freshness_is_declared_per_action_and_only_reads_have_one() {
    use graphhelm_tool_broker::call::FreshnessClass;
    // Repository reads are exact within a source snapshot — the project HEAD pins them:
    for call in [
        read_call(),
        ToolCall::Repository(RepositoryAction::ListFiles { prefix: None }),
        ToolCall::Repository(RepositoryAction::Diff),
    ] {
        assert_eq!(call.freshness(), Some(FreshnessClass::SnapshotClosed));
    }
    // Anything that writes or spawns caller-shaped work is never cache-eligible:
    for call in [
        ToolCall::Repository(RepositoryAction::ApplyPatch { patch: "d".into() }),
        ToolCall::Repository(RepositoryAction::Commit {
            message: "m".into(),
        }),
        shell_call("git"),
        ToolCall::Tests(TestsAction { arguments: vec![] }),
    ] {
        assert_eq!(call.freshness(), None);
    }
}

#[test]
fn freshness_wire_names_match_the_amended_spec_vocabulary() {
    use graphhelm_tool_broker::call::FreshnessClass;
    // AGENTS_SKILLS_PLUGINS §11.3 as amended by #33: the three-word closed vocabulary.
    for (class, name) in [
        (FreshnessClass::ImmutableByInput, "immutable_by_input"),
        (FreshnessClass::SnapshotClosed, "snapshot_closed"),
        (FreshnessClass::Drifting, "drifting"),
    ] {
        assert_eq!(
            serde_json::to_value(class).unwrap(),
            serde_json::json!(name)
        );
    }
}
