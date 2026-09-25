//! The pure §11.2 decision pipeline: identity → capability → program allowlist → effect → tier.
//! Every refusal is typed and content-free; deny-by-default is the shape of the lease, not a
//! setting.

use graphhelm_tool_broker::call::{RepositoryAction, ShellAction, TestsAction, ToolCall};
use graphhelm_tool_broker::effect::IsolationTier;
use graphhelm_tool_broker::lease::{
    BrokerRefusal, Capability, MAX_PROGRAM_ALLOWLIST_MEMBERS, ToolLease, authorize,
};
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

/// #247: a malformed name is refused by the SHAPE rule, not reported as an allowlist miss.
///
/// Both refusals are correct answers to "is this call permitted"; they are different answers to
/// "what do I do now". A path, a dotted name, an uppercase letter can never be in any allowlist,
/// so telling the caller to change the lease sends them to the wrong place.
#[test]
fn a_malformed_program_name_is_refused_by_shape_not_reported_as_unlisted() {
    let lease = full_lease("agent-builder");
    for malformed in ["Curl", "bin/curl", "curl.exe", "", "C:\\tools\\git"] {
        assert!(
            matches!(
                authorize(&shell_call(malformed), &lease, "agent-builder").unwrap_err(),
                BrokerRefusal::ProgramNameInvalid
            ),
            "{malformed:?} must be refused by shape"
        );
    }
}

/// The split must not weaken the no-echo rule: a malformed NAME is caller content too, and the
/// shape refusal names the rule without repeating the string that broke it.
#[test]
fn a_shape_refusal_never_echoes_the_malformed_program_name() {
    let lease = full_lease("agent-builder");
    let sentinel = "SENTINEL-program-name/with-path";
    let refusal = authorize(&shell_call(sentinel), &lease, "agent-builder").unwrap_err();
    assert!(matches!(refusal, BrokerRefusal::ProgramNameInvalid));
    let rendered = format!("{refusal} {refusal:?}");
    assert!(
        !rendered.contains("SENTINEL"),
        "the malformed name travelled into the refusal: {rendered}"
    );
}

#[test]
fn an_invalid_member_makes_the_complete_program_allowlist_fail_closed() {
    let mut lease = full_lease("agent-builder");
    lease.programs.insert("TOKEN=secret".to_owned());

    assert!(matches!(
        authorize(&read_call(), &lease, "agent-builder").unwrap_err(),
        BrokerRefusal::ProgramAllowlistInvalid
    ));
}

#[test]
fn the_program_allowlist_member_bound_accepts_the_limit_and_refuses_limit_plus_one() {
    let programs = (0..MAX_PROGRAM_ALLOWLIST_MEMBERS)
        .map(|index| format!("p{index}"))
        .collect();
    let mut lease = full_lease("agent-builder");
    lease.programs = programs;

    assert!(authorize(&read_call(), &lease, "agent-builder").is_ok());

    lease
        .programs
        .insert(format!("p{MAX_PROGRAM_ALLOWLIST_MEMBERS}"));
    assert!(matches!(
        authorize(&read_call(), &lease, "agent-builder").unwrap_err(),
        BrokerRefusal::ProgramAllowlistInvalid
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

/// One row of the no-echo population: an input built to PRODUCE one refusal variant, with the
/// caller-controlled sentinel placed where that variant's input goes.
struct NoEchoRow {
    name: &'static str,
    sentinel: &'static str,
    call: ToolCall,
    caller: &'static str,
    lease: ToolLease,
    expect: fn(&BrokerRefusal) -> bool,
}

/// A refusal names the rule and the tool, never content: a denied patch, argument list, program
/// name or caller identifier must not travel into logs through the error path.
///
/// ONE population, not one per variant (#860). The first version of this guard drove a listed
/// program with a sentinel ARGUMENT, so it could only ever obtain the allowlist refusal -- when
/// #845 added `ProgramNameInvalid`, the new door was protected by a cell of its own and this guard
/// stayed green about a variant it never constructed. Two guards, one property, two disjoint
/// populations: if a variant gains a field, the guard that never builds it cannot notice.
///
/// Each row asserts the variant it expects BEFORE asserting absence: a row that quietly stops
/// producing its door would otherwise be testing the wrong one and passing. That precondition
/// caught this file's own author on the first run -- a caller spelled in uppercase fails actor
/// validation before mismatch is checked -- which is why every row carries a sentinel shaped to
/// reach ITS door. The row name travels into every failure, so the output says which population
/// caught it.
#[test]
fn refusals_never_echo_call_arguments() {
    let shell = |program: &str, argument: &str| {
        ToolCall::Shell(ShellAction {
            program: program.to_owned(),
            arguments: vec![argument.to_owned()],
        })
    };
    let rows = [
        NoEchoRow {
            name: "unlisted program, sentinel in the argument (the original population)",
            sentinel: "SENTINEL-argument-value",
            call: shell("curl", "SENTINEL-argument-value"),
            caller: "agent-builder",
            lease: full_lease("agent-builder"),
            expect: |r| matches!(r, BrokerRefusal::ProgramDenied),
        },
        NoEchoRow {
            name: "malformed program: the sentinel IS the name (#845's new door)",
            sentinel: "SENTINEL/bin",
            call: shell("SENTINEL/bin", "status"),
            caller: "agent-builder",
            lease: full_lease("agent-builder"),
            expect: |r| matches!(r, BrokerRefusal::ProgramNameInvalid),
        },
        NoEchoRow {
            name: "capability missing, sentinel in the argument",
            sentinel: "SENTINEL-argument-value",
            call: shell("git", "SENTINEL-argument-value"),
            caller: "agent-builder",
            lease: ToolLease {
                capabilities: [Capability::RepositoryRead].into_iter().collect(),
                ..full_lease("agent-builder")
            },
            expect: |r| matches!(r, BrokerRefusal::CapabilityMissing { .. }),
        },
        NoEchoRow {
            name: "actor mismatch: a VALID foreign caller is the sentinel (lease_actor may show; the caller must not)",
            sentinel: "sentinel-foreign-caller",
            call: shell("git", "status"),
            caller: "sentinel-foreign-caller",
            lease: full_lease("agent-builder"),
            expect: |r| matches!(r, BrokerRefusal::ActorMismatch { .. }),
        },
        NoEchoRow {
            name: "actor invalid: an INVALID caller is the sentinel",
            sentinel: "SENTINEL-CALLER",
            call: shell("git", "status"),
            caller: "SENTINEL-CALLER",
            lease: full_lease("agent-builder"),
            expect: |r| matches!(r, BrokerRefusal::ActorInvalid),
        },
    ];
    for row in rows {
        let refusal = authorize(&row.call, &row.lease, row.caller).expect_err(&format!(
            "row [{}] must be refused, or it tests nothing",
            row.name
        ));
        assert!(
            (row.expect)(&refusal),
            "row [{}] produced a different variant than it was built for: {refusal:?}",
            row.name
        );
        let rendered = format!("{refusal} {refusal:?}");
        assert!(
            !rendered.contains(row.sentinel),
            "row [{}]: caller content travelled into the refusal: {rendered}",
            row.name
        );
    }
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
