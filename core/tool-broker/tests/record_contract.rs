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
