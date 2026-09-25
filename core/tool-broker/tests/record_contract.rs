use graphhelm_tool_broker::record::{ToolCallRecord, ToolDisposition, digest_hex};

#[test]
fn the_digest_is_sha256_hex_of_the_bytes() {
    // The empty-input SHA-256 vector, pinned so the helper can never silently change algorithm.
    assert_eq!(
        digest_hex(b""),
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
}

#[test]
fn a_record_serializes_without_any_free_form_stream_content() {
    const STREAM: &[u8] = b"THE-STREAM-CONTENT-SENTINEL";
    let record = ToolCallRecord {
        tool: "shell".into(),
        action: "run".into(),
        actor: "agent-builder".into(),
        program_allowlist: ["rustc", "cargo", "git"]
            .map(str::to_owned)
            .into_iter()
            .collect(),
        tier: graphhelm_tool_broker::effect::IsolationTier::Tier1,
        disposition: ToolDisposition::Completed { exit_code: 0 },
        stdout_sha256: digest_hex(STREAM),
        stdout_bytes: STREAM.len() as u64,
        stderr_sha256: digest_hex(b""),
        stderr_bytes: 0,
        truncated: false,
        reused: false,
        verified_executable: None,
        contained_session: None,
        commit: None,
        landed_ref: None,
        recovered_workspace: false,
    };
    let json = serde_json::to_string(&record).unwrap();
    // The record is what 05d will externalize beside Evidence; the streams themselves must
    // never ride in it (D-036's discipline applied one layer early). A digest-only type has
    // nowhere to put the bytes — this test keeps it that way.
    assert!(
        !json.contains("THE-STREAM-CONTENT-SENTINEL"),
        "stream bytes leaked into the record"
    );
    for key in [
        "stdoutSha256",
        "stderrSha256",
        "disposition",
        "tier",
        "programAllowlist",
    ] {
        assert!(json.contains(key), "{key} missing");
    }
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&json).unwrap()["programAllowlist"],
        serde_json::json!(["cargo", "git", "rustc"]),
        "equivalent grants need one canonical wire order"
    );
}

#[test]
fn a_pre_allowlist_record_decodes_with_an_empty_authority_set() {
    let legacy = serde_json::json!({
        "tool": "shell",
        "action": "run",
        "actor": "agent-builder",
        "tier": "tier_1",
        "disposition": { "kind": "completed", "exit_code": 0 },
        "stdoutSha256": digest_hex(b""),
        "stdoutBytes": 0,
        "stderrSha256": digest_hex(b""),
        "stderrBytes": 0,
        "truncated": false,
        "reused": false
    });

    let record: ToolCallRecord = serde_json::from_value(legacy).unwrap();

    assert!(record.program_allowlist.is_empty());
}

#[test]
fn dispositions_cover_refusal_timeout_and_host_error() {
    for disposition in [
        ToolDisposition::Completed { exit_code: 1 },
        ToolDisposition::Denied {
            rule: "capability_missing".into(),
        },
        ToolDisposition::TimedOut,
        ToolDisposition::HostError {
            code: "GHTOOL001_WORKSPACE".into(),
        },
    ] {
        let json = serde_json::to_value(&disposition).unwrap();
        assert!(json.get("kind").is_some(), "tagged form required: {json}");
    }
}

/// #1066: the ref an execution's commits land under is derived from the id, deterministically,
/// and never leaves the `refs/graphhelm/executions/` namespace.
#[test]
fn an_execution_ref_uses_the_id_when_it_is_a_legal_ref_segment() {
    use graphhelm_tool_broker::record::{EXECUTION_REF_NAMESPACE, execution_ref};
    assert_eq!(
        execution_ref("exec-useful-change"),
        "refs/graphhelm/executions/exec-useful-change"
    );
    assert_eq!(
        execution_ref("exec_27fe0b8b"),
        format!("{EXECUTION_REF_NAMESPACE}exec_27fe0b8b")
    );
}

/// An id git would refuse in a ref name takes the digest spelling instead of being mangled,
/// so two distinct ids can never collide on one ref and the record still names what was written.
#[test]
fn an_execution_id_git_refuses_takes_the_digest_spelling() {
    use graphhelm_tool_broker::record::{digest_hex, execution_ref};
    for hostile in [
        "a..b",
        ".hidden",
        "trailing.",
        "x.lock",
        "has~tilde",
        "has:colon",
        "q?",
        "",
        "Build-1",
    ] {
        let reference = execution_ref(hostile);
        let expected = format!(
            "refs/graphhelm/executions/sha256-{}",
            &digest_hex(hostile.as_bytes())[..32]
        );
        assert_eq!(reference, expected, "{hostile:?}");
    }
    assert_ne!(execution_ref("a..b"), execution_ref("a.b"));
}

/// Two ids that differ only by case land under two refs (#1073): loose refs are files, and on a
/// case-insensitive filesystem a verbatim `Build-1` would alias `build-1`.
#[test]
fn ids_that_differ_only_by_case_land_under_distinct_refs() {
    use graphhelm_tool_broker::record::execution_ref;
    let lower = execution_ref("build-1");
    let upper = execution_ref("Build-1");
    assert_eq!(lower, "refs/graphhelm/executions/build-1");
    assert!(
        upper.starts_with("refs/graphhelm/executions/sha256-"),
        "{upper}"
    );
    assert_ne!(lower.to_ascii_lowercase(), upper.to_ascii_lowercase());
    assert!(
        upper.bytes().all(|byte| !byte.is_ascii_uppercase()),
        "the digest spelling must itself be lowercase: {upper}"
    );
}

/// An id spelled like a digest ref takes the digest branch too (#1073): verbatim,
/// `sha256-<32 hex>` IS the digest spelling of some other id, so the literal id
/// `sha256-a5bb0dc632b3b0d78905c50440ef0c31` and `Build-1` (whose digest that is) would share
/// one ref. The verbatim and digest spellings must be disjoint.
#[test]
fn an_id_spelled_like_a_digest_ref_cannot_collide_with_the_id_it_digests() {
    use graphhelm_tool_broker::record::{digest_hex, execution_ref};
    let colliding = "Build-1";
    let digest_of_build = &digest_hex(colliding.as_bytes())[..32];
    assert_eq!(digest_of_build, "a5bb0dc632b3b0d78905c50440ef0c31");
    let impostor = format!("sha256-{digest_of_build}");
    let impostor_ref = execution_ref(&impostor);
    let colliding_ref = execution_ref(colliding);
    assert_ne!(impostor_ref, colliding_ref);
    assert_eq!(
        impostor_ref,
        format!(
            "refs/graphhelm/executions/sha256-{}",
            &digest_hex(impostor.as_bytes())[..32]
        ),
        "a sha256- id is digested, never spelled verbatim"
    );
    // Any `sha256-` prefix is enough — not only the 32-hex shape.
    assert!(
        execution_ref("sha256-x")
            .trim_start_matches("refs/graphhelm/executions/sha256-")
            .len()
            == 32
    );
}

/// The two landing fields are optional on the wire: an old record decodes with both absent, and a
/// record that carries neither serializes neither (the record stays digest-only and content-free).
#[test]
fn a_record_without_a_landing_decodes_and_serializes_without_the_fields() {
    let json = r#"{"tool":"shell","action":"run","actor":"agent-a","tier":"tier_1","disposition":{"kind":"completed","exit_code":0},"stdoutSha256":"","stdoutBytes":0,"stderrSha256":"","stderrBytes":0,"truncated":false,"reused":false}"#;
    let record: ToolCallRecord = serde_json::from_str(json).unwrap();
    assert_eq!(record.commit, None);
    assert_eq!(record.landed_ref, None);
    let wire = serde_json::to_string(&record).unwrap();
    assert!(!wire.contains("\"commit\""), "{wire}");
    assert!(!wire.contains("\"landedRef\""), "{wire}");
}
