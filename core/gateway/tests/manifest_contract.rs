use graphhelm_gateway::manifest::{
    BillingMode, DEFAULT_TIMEOUT_SECONDS, ManifestError, RouteManifest,
};

fn valid_manifest_json() -> serde_json::Value {
    serde_json::json!({
        "manifestVersion": 1,
        "routes": [
            {
                "id": "anthropic_byok",
                "provider": "anthropic",
                "transport": "direct_api",
                "authentication": "api_key",
                "billingMode": "per_token",
                "baseUrl": "https://api.anthropic.com",
                "model": "claude-sonnet-5",
                "credentialRef": "secret_anthropic_primary",
                "profiles": ["critical_reasoning"],
                "enabled": true
            },
            {
                "id": "claude_subscription",
                "provider": "anthropic",
                "transport": "native_runtime",
                "runtime": "claude_code",
                "authentication": "account_subscription",
                "billingMode": "subscription_quota",
                "command": { "program": "claude", "args": ["-p", "--output-format", "json"] },
                "profiles": ["software_execution"],
                "enabled": true
            }
        ]
    })
}

#[test]
fn a_valid_manifest_parses_and_reports_both_routes() {
    let manifest = RouteManifest::from_json(&valid_manifest_json().to_string()).unwrap();
    assert_eq!(manifest.routes().len(), 2);
    assert_eq!(manifest.routes()[0].billing_mode(), BillingMode::PerToken);
    assert_eq!(
        manifest.routes()[1].billing_mode(),
        BillingMode::SubscriptionQuota
    );
}

#[test]
fn billing_and_authentication_must_agree_with_the_transport() {
    // §20: BYOK and subscription are DISTINCT billing modes. A direct_api route claiming
    // subscription_quota, or a native_runtime route claiming per_token, is a category error.
    let mut bad = valid_manifest_json();
    bad["routes"][0]["billingMode"] = "subscription_quota".into();
    let err = RouteManifest::from_json(&bad.to_string()).unwrap_err();
    assert!(matches!(
        err,
        ManifestError::BillingTransportMismatch { .. }
    ));
}

#[test]
fn direct_api_requires_credential_ref_and_native_runtime_forbids_it() {
    let mut missing = valid_manifest_json();
    missing["routes"][0]
        .as_object_mut()
        .unwrap()
        .remove("credentialRef");
    assert!(RouteManifest::from_json(&missing.to_string()).is_err());

    let mut leaky = valid_manifest_json();
    leaky["routes"][1]["credentialRef"] = "secret_smuggled".into();
    // credential_export: false is structural — a native runtime owns its own auth and the
    // manifest cannot route a broker secret into it.
    assert!(RouteManifest::from_json(&leaky.to_string()).is_err());
}

#[test]
fn duplicate_route_ids_unknown_fields_and_oversize_are_refused() {
    let mut dup = valid_manifest_json();
    dup["routes"][1]["id"] = "anthropic_byok".into();
    assert!(matches!(
        RouteManifest::from_json(&dup.to_string()).unwrap_err(),
        ManifestError::DuplicateRouteId { .. }
    ));

    let mut unknown = valid_manifest_json();
    unknown["routes"][0]["extra"] = true.into();
    assert!(RouteManifest::from_json(&unknown.to_string()).is_err());

    let oversize = "x".repeat(graphhelm_gateway::manifest::MAX_MANIFEST_BYTES + 1);
    assert!(matches!(
        RouteManifest::from_json(&oversize).unwrap_err(),
        ManifestError::Oversize { .. }
    ));
}

#[test]
fn a_direct_api_route_with_an_unrecognized_provider_is_refused() {
    // Milestone 05b Task 4: the BYOK adapters (adapters/model-gateway/src/byok.rs) speak exactly
    // two chat provider wire formats, and the System One adapter
    // (adapters/model-gateway/src/systemone.rs) one more. A direct_api route naming any other
    // provider would parse but have no adapter able to place its call — refused at load time
    // instead.
    let mut bad = valid_manifest_json();
    bad["routes"][0]["provider"] = "cohere".into();
    assert!(matches!(
        RouteManifest::from_json(&bad.to_string()).unwrap_err(),
        ManifestError::StructuralViolation { .. }
    ));
}

#[test]
fn typesafe_is_a_legal_direct_api_provider() {
    let json = serde_json::json!({ "manifestVersion": 1, "routes": [{
        "id": "judge", "provider": "typesafe", "transport": "direct_api",
        "authentication": "api_key", "billingMode": "per_token",
        "baseUrl": "https://api.typesafe.ai", "model": "jev-latest",
        "credentialRef": "secret_typesafe", "profiles": ["balanced_reasoning"], "enabled": true
    }]});
    assert!(RouteManifest::from_json(&json.to_string()).is_ok());
}

#[test]
fn timeout_seconds_defaults_and_can_be_overridden() {
    // Milestone 05b Task 5 (gateway-slice plan): Task 1 shipped
    // no per-route timeout; the native-runtime adapter needs one, so Task 5 added
    // `timeoutSeconds` here with a default rather than requiring every existing manifest to
    // declare it.
    let manifest = RouteManifest::from_json(&valid_manifest_json().to_string()).unwrap();
    assert_eq!(
        manifest.routes()[0].timeout_seconds(),
        DEFAULT_TIMEOUT_SECONDS
    );
    assert_eq!(
        manifest.routes()[1].timeout_seconds(),
        DEFAULT_TIMEOUT_SECONDS
    );

    let mut overridden = valid_manifest_json();
    overridden["routes"][1]["timeoutSeconds"] = 45.into();
    let manifest = RouteManifest::from_json(&overridden.to_string()).unwrap();
    assert_eq!(
        manifest.routes()[0].timeout_seconds(),
        DEFAULT_TIMEOUT_SECONDS
    );
    assert_eq!(manifest.routes()[1].timeout_seconds(), 45);
}

/// `serde_json`'s own error `Display` embeds manifest content verbatim — an unknown field's
/// *name* is quoted directly into `"unknown field \"...\", expected ..."`. If `ManifestError::Parse`
/// wrapped that message as-is (the pre-fix shape), a distinctive value used as a field name would
/// be echoed straight through `ManifestError::Display`/`Debug` — and from there, unredacted, into
/// `GHCLI009`'s CLI output (`apps/cli/src/commands/gateway/mod.rs::load_manifest`). `Parse` must
/// instead carry only `line`/`column`/`category`, never the raw message.
#[test]
fn a_parse_error_from_an_unknown_field_name_never_echoes_it_but_carries_position() {
    const MARKER: &str = "MARKER-UNKNOWN-FIELD-7f3c1a9e";
    let mut bad = valid_manifest_json();
    bad["routes"][0][MARKER] = true.into();

    let error = RouteManifest::from_json(&bad.to_string()).unwrap_err();
    match &error {
        ManifestError::Parse {
            line,
            column,
            category,
        } => {
            assert!(*line > 0, "{error:?}");
            assert!(*column > 0, "{error:?}");
            assert_eq!(*category, "data", "{error:?}");
        }
        other => panic!("expected ManifestError::Parse, got {other:?}"),
    }

    let display = error.to_string();
    let debug = format!("{error:?}");
    assert!(!display.contains(MARKER), "Display leaked: {display}");
    assert!(!debug.contains(MARKER), "Debug leaked: {debug}");
}

/// The other half of `serde_json`'s content-echoing messages: a wrong-typed value (a string where
/// `timeoutSeconds` requires a number) is quoted into `"invalid type: string \"...\", expected
/// u64"` — the exact shape an operator who pastes a credential into the wrong field would trigger.
#[test]
fn a_parse_error_from_a_wrong_typed_value_never_echoes_it_but_carries_position() {
    const MARKER: &str = "MARKER-WRONG-TYPE-3e9b7c1f";
    let mut bad = valid_manifest_json();
    bad["routes"][0]["timeoutSeconds"] = MARKER.into();

    let error = RouteManifest::from_json(&bad.to_string()).unwrap_err();
    match &error {
        ManifestError::Parse {
            line,
            column,
            category,
        } => {
            assert!(*line > 0, "{error:?}");
            assert!(*column > 0, "{error:?}");
            assert_eq!(*category, "data", "{error:?}");
        }
        other => panic!("expected ManifestError::Parse, got {other:?}"),
    }

    let display = error.to_string();
    let debug = format!("{error:?}");
    assert!(!display.contains(MARKER), "Display leaked: {display}");
    assert!(!debug.contains(MARKER), "Debug leaked: {debug}");
}

#[test]
fn timeout_seconds_beyond_24_hours_is_refused_and_24_hours_exactly_is_accepted() {
    // Milestone 05b final review: `runtime.rs`'s deadline loop computes
    // `Instant::now() + Duration::from_secs(route.timeout_seconds())`, and `Instant` addition can
    // overflow (panicking) for a large enough duration. 24 hours is comfortably beyond any
    // legitimate native-runtime invocation and applies to both transports, since `timeoutSeconds`
    // itself is not restricted to `native_runtime` routes structurally.
    let mut too_large = valid_manifest_json();
    too_large["routes"][1]["timeoutSeconds"] = 86_401.into();
    assert!(matches!(
        RouteManifest::from_json(&too_large.to_string()).unwrap_err(),
        ManifestError::TimeoutTooLarge { .. }
    ));

    let mut boundary = valid_manifest_json();
    boundary["routes"][1]["timeoutSeconds"] = 86_400.into();
    assert!(RouteManifest::from_json(&boundary.to_string()).is_ok());
}

#[test]
fn a_non_loopback_http_base_url_is_refused() {
    // TLS is the default posture: http:// is allowed only for loopback (local fakes and
    // OpenAI-compatible local endpoints later); a cleartext remote URL is a misconfiguration.
    let mut bad = valid_manifest_json();
    bad["routes"][0]["baseUrl"] = "http://api.anthropic.com".into();
    assert!(matches!(
        RouteManifest::from_json(&bad.to_string()).unwrap_err(),
        ManifestError::CleartextRemoteUrl { .. }
    ));
    let mut ok = valid_manifest_json();
    ok["routes"][0]["baseUrl"] = "http://127.0.0.1:9999".into();
    assert!(RouteManifest::from_json(&ok.to_string()).is_ok());
}

/// PR review BLOCKER 1: `is_loopback_authority` must strip userinfo (`user:pass@`) BEFORE
/// inspecting the host. Pre-fix, `http://[::1]@evil.com` reads the bracketed `::1` as the host
/// (the bracket branch discarded everything after `]`) and `http://localhost:tok@attacker.example`
/// reads `localhost` as the host (the naive `:`-split takes the userinfo's `user` segment) — both
/// pass as loopback while the connection a real HTTP client makes lands on the attacker's host
/// after the LAST `@`, sending the route's api key there in cleartext.
#[test]
fn userinfo_cannot_be_used_to_disguise_a_remote_host_as_loopback() {
    let mut bracket_bypass = valid_manifest_json();
    bracket_bypass["routes"][0]["baseUrl"] = "http://[::1]@evil.com".into();
    assert!(
        matches!(
            RouteManifest::from_json(&bracket_bypass.to_string()).unwrap_err(),
            ManifestError::UserinfoBaseUrl { .. }
        ),
        "http://[::1]@evil.com must be refused as userinfo"
    );

    let mut localhost_bypass = valid_manifest_json();
    localhost_bypass["routes"][0]["baseUrl"] = "http://localhost:tok@attacker.example".into();
    assert!(
        matches!(
            RouteManifest::from_json(&localhost_bypass.to_string()).unwrap_err(),
            ManifestError::UserinfoBaseUrl { .. }
        ),
        "http://localhost:tok@attacker.example must be refused as userinfo"
    );

    // Userinfo is forbidden even for loopback: the URL may be listed or logged by a caller.
    let mut genuine_loopback = valid_manifest_json();
    genuine_loopback["routes"][0]["baseUrl"] = "http://user:pass@127.0.0.1:9".into();
    assert!(matches!(
        RouteManifest::from_json(&genuine_loopback.to_string()).unwrap_err(),
        ManifestError::UserinfoBaseUrl { .. }
    ));

    // https:// is also forbidden from carrying userinfo.
    let mut https_untouched = valid_manifest_json();
    https_untouched["routes"][0]["baseUrl"] = "https://user:pass@evil.com".into();
    assert!(matches!(
        RouteManifest::from_json(&https_untouched.to_string()).unwrap_err(),
        ManifestError::UserinfoBaseUrl { .. }
    ));

    let mut query_at = valid_manifest_json();
    query_at["routes"][0]["baseUrl"] = "https://api.example.com?email=a@b".into();
    assert!(RouteManifest::from_json(&query_at.to_string()).is_ok());

    let mut fragment_at = valid_manifest_json();
    fragment_at["routes"][0]["baseUrl"] = "https://api.example.com#contact=a@b".into();
    assert!(RouteManifest::from_json(&fragment_at.to_string()).is_ok());
}

/// PR review MEDIUM 11: a trailing `/` on `baseUrl` is refused rather than silently normalized
/// away, so nothing downstream has to guess whether joining a path onto `base_url` can produce a
/// double slash.
#[test]
fn a_trailing_slash_on_base_url_is_refused() {
    let mut ok = valid_manifest_json();
    ok["routes"][0]["baseUrl"] = "https://api.anthropic.com".into();
    assert!(RouteManifest::from_json(&ok.to_string()).is_ok());

    let mut trailing = valid_manifest_json();
    trailing["routes"][0]["baseUrl"] = "https://api.anthropic.com/".into();
    assert!(matches!(
        RouteManifest::from_json(&trailing.to_string()).unwrap_err(),
        ManifestError::TrailingSlashBaseUrl { .. }
    ));
}

/// PR review SECONDARY (a): `CleartextRemoteUrl`/`TrailingSlashBaseUrl` must name the route id and
/// rule only — never the `baseUrl` value itself, since a credential pasted into `baseUrl` by
/// mistake must not reach CI logs.
#[test]
fn cleartext_and_trailing_slash_errors_never_echo_the_base_url_value() {
    const MARKER: &str = "MARKER-BASEURL-SECRET-9a1c";

    let mut cleartext = valid_manifest_json();
    cleartext["routes"][0]["baseUrl"] = format!("http://{MARKER}.example.com").into();
    let error = RouteManifest::from_json(&cleartext.to_string()).unwrap_err();
    assert!(matches!(error, ManifestError::CleartextRemoteUrl { .. }));
    assert!(!error.to_string().contains(MARKER), "{error}");
    assert!(!format!("{error:?}").contains(MARKER), "{error:?}");

    let mut trailing = valid_manifest_json();
    trailing["routes"][0]["baseUrl"] = format!("https://{MARKER}.example.com/").into();
    let error = RouteManifest::from_json(&trailing.to_string()).unwrap_err();
    assert!(matches!(error, ManifestError::TrailingSlashBaseUrl { .. }));
    assert!(!error.to_string().contains(MARKER), "{error}");
    assert!(!format!("{error:?}").contains(MARKER), "{error:?}");
}
