//! `graphhelm gateway` CLI suite (gateway-slice plan, Task 6):
//! `routes`, `probe`, and `credential set|remove`, spawned as a compiled binary exactly like every
//! other CLI suite (`execution_cli.rs`'s `command()` pattern).

use std::path::{Path, PathBuf};

use assert_cmd::Command;
use serde_json::Value;

/// A distinctive credential value planted by `credential set`'s tests. Must never appear in any
/// command's stdout or stderr (plan rule 6).
const SENTINEL: &str = "sk-ant-SENTINEL-0123456789abcdef";
/// A distinctive marker embedded in the invalid-manifest fixture. Must never appear in `routes`'s
/// output when the manifest is refused (plan Task 6, test 2).
const DISTINCTIVE_MARKER: &str = "MARKER-9F8E7D2C1B-NEVER-LEAK";

fn command() -> Command {
    Command::new(assert_cmd::cargo::cargo_bin!("graphhelm"))
}

fn json(output: &[u8]) -> Value {
    serde_json::from_slice(output).unwrap_or_else(|error| {
        panic!(
            "stdout must be valid JSON ({error}): {:?}",
            String::from_utf8_lossy(output)
        )
    })
}

/// 64 lowercase hexadecimal characters — a well-formed `GRAPHHELM_GATEWAY_KEY`.
fn passphrase() -> String {
    "0123456789abcdef".repeat(4)
}

/// The compiled `graphhelm` binary's own absolute path, reused as a `native_runtime` route's
/// `command.program`: its real `--version` exits `0`, so `gateway probe` can exercise a genuine
/// subprocess spawn/wait/deadline cycle without depending on `claude`/`codex` being installed.
/// Manifest validation only requires `command.program` to be a non-empty string (Task 1) — nothing
/// restricts it to a bare name on `PATH`, so the absolute path is accepted as-is.
fn native_program() -> String {
    assert_cmd::cargo::cargo_bin!("graphhelm")
        .to_str()
        .unwrap()
        .to_owned()
}

/// A manifest declaring one `direct_api` route (`anthropic_byok`, credential `cred_anthropic`) and
/// one `native_runtime` route (`native_probe`, spawning the `graphhelm` binary itself).
fn valid_manifest_value() -> serde_json::Value {
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
                "credentialRef": "cred_anthropic",
                "profiles": ["critical_reasoning"],
                "enabled": true
            },
            {
                "id": "native_probe",
                "provider": "anthropic",
                "transport": "native_runtime",
                "runtime": "claude_code",
                "authentication": "account_subscription",
                "billingMode": "subscription_quota",
                "command": { "program": native_program(), "args": [] },
                "profiles": ["software_execution"],
                "enabled": true
            }
        ]
    })
}

/// Structurally valid per-route, but both routes share the id `anthropic_byok` — refused by
/// `RouteManifest::from_json`'s duplicate-id scan, which runs only after every route's own
/// structural rules already passed. Carries [`DISTINCTIVE_MARKER`] in the second route's `model`
/// field so the "never leaks file contents" assertion has something concrete to check for.
fn duplicate_id_manifest_value() -> serde_json::Value {
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
                "credentialRef": "cred_anthropic",
                "profiles": ["critical_reasoning"],
                "enabled": true
            },
            {
                "id": "anthropic_byok",
                "provider": "anthropic",
                "transport": "direct_api",
                "authentication": "api_key",
                "billingMode": "per_token",
                "baseUrl": "https://api.anthropic.com",
                "model": format!("claude-sonnet-5-{DISTINCTIVE_MARKER}"),
                "credentialRef": "cred_anthropic_2",
                "profiles": ["critical_reasoning"],
                "enabled": true
            }
        ]
    })
}

/// Structurally would be valid, but `timeoutSeconds` on the first route is a string instead of a
/// number — refused by `serde_json` itself while parsing (a `ManifestError::Parse`), not by any
/// of `RouteManifest::from_json`'s own structural rules. Carries [`DISTINCTIVE_MARKER`] as that
/// string value: `serde_json`'s own "invalid type" message would otherwise quote it verbatim, the
/// same failure mode test 2 proves for the `DuplicateRouteId` path, exercised here for the
/// `Parse` path specifically.
fn parse_error_manifest_value() -> serde_json::Value {
    let mut value = valid_manifest_value();
    value["routes"][0]["timeoutSeconds"] = DISTINCTIVE_MARKER.into();
    value
}

fn write_manifest(directory: &Path, value: &serde_json::Value) -> PathBuf {
    let path = directory.join("manifest.json");
    std::fs::write(&path, serde_json::to_vec_pretty(value).unwrap()).unwrap();
    path
}

fn combined_output(output: &std::process::Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

/// Runs `gateway credential set`, piping `value` on stdin. Panics (with stderr attached) on a
/// non-zero exit so a bad test fixture fails loudly at the setup step rather than a later,
/// confusing assertion.
#[allow(clippy::too_many_arguments)]
fn credential_set(
    broker: &Path,
    keyring: &Path,
    key_id: &str,
    reference: &str,
    provider: &str,
    usable_by: &str,
    key: &str,
    value: &str,
) -> Value {
    let output = command()
        .args([
            "gateway",
            "credential",
            "set",
            "--broker",
            broker.to_str().unwrap(),
            "--keyring",
            keyring.to_str().unwrap(),
            "--key-id",
            key_id,
            "--ref",
            reference,
            "--provider",
            provider,
            "--usable-by",
            usable_by,
        ])
        .env("GRAPHHELM_GATEWAY_KEY", key)
        .write_stdin(value)
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", combined_output(&output));
    json(&output.stdout)
}

fn probe(
    manifest: &Path,
    route: &str,
    broker: Option<&Path>,
    keyring: Option<&Path>,
    key_id: Option<&str>,
    key: Option<&str>,
) -> std::process::Output {
    let mut cmd = command();
    cmd.args([
        "gateway",
        "probe",
        "--manifest",
        manifest.to_str().unwrap(),
        "--route",
        route,
    ]);
    if let Some(broker) = broker {
        cmd.args(["--broker", broker.to_str().unwrap()]);
    }
    if let Some(keyring) = keyring {
        cmd.args(["--keyring", keyring.to_str().unwrap()]);
    }
    if let Some(key_id) = key_id {
        cmd.args(["--key-id", key_id]);
    }
    if let Some(key) = key {
        cmd.env("GRAPHHELM_GATEWAY_KEY", key);
    }
    cmd.output().unwrap()
}

// ---------------------------------------------------------------------------
// Test 1: happy `routes` on a valid manifest.
// ---------------------------------------------------------------------------

#[test]
fn routes_reports_both_routes_from_a_valid_manifest() {
    let directory = tempfile::tempdir().unwrap();
    let manifest = write_manifest(directory.path(), &valid_manifest_value());

    let output = command()
        .args([
            "gateway",
            "routes",
            "--manifest",
            manifest.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", combined_output(&output));
    let value = json(&output.stdout);
    assert_eq!(value["ok"], true);
    assert_eq!(value["command"], "gateway.routes");

    let routes = value["data"]["routes"].as_array().unwrap();
    assert_eq!(routes.len(), 2, "{value}");
    let ids: Vec<&str> = routes
        .iter()
        .map(|route| route["id"].as_str().unwrap())
        .collect();
    assert!(ids.contains(&"anthropic_byok"), "{value}");
    assert!(ids.contains(&"native_probe"), "{value}");

    let byok = routes
        .iter()
        .find(|route| route["id"] == "anthropic_byok")
        .unwrap();
    assert_eq!(byok["provider"], "anthropic");
    assert_eq!(byok["transport"], "direct_api");
    assert_eq!(byok["billingMode"], "per_token");
    assert_eq!(byok["profiles"], serde_json::json!(["critical_reasoning"]));
    assert_eq!(byok["enabled"], true);
}

// ---------------------------------------------------------------------------
// Test 2: invalid manifest (duplicate id) is refused without leaking file contents.
// ---------------------------------------------------------------------------

#[test]
fn routes_refuses_an_invalid_manifest_without_leaking_its_contents() {
    let directory = tempfile::tempdir().unwrap();
    let manifest = write_manifest(directory.path(), &duplicate_id_manifest_value());

    let output = command()
        .args([
            "gateway",
            "routes",
            "--manifest",
            manifest.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let value = json(&output.stdout);
    assert_eq!(value["ok"], false);
    assert_eq!(
        value["diagnostics"][0]["code"], "GHCLI009_GATEWAY_INVALID",
        "{value}"
    );

    let combined = combined_output(&output);
    assert!(
        !combined.contains(DISTINCTIVE_MARKER),
        "the manifest's file contents must never reach output: {combined}"
    );
}

// ---------------------------------------------------------------------------
// Test 2b: a manifest that fails to *parse* (as opposed to test 2's structurally-valid-but-
// duplicate-id manifest) is refused the same way, without leaking its contents either.
// ---------------------------------------------------------------------------

#[test]
fn routes_refuses_a_manifest_that_fails_to_parse_without_leaking_its_contents() {
    let directory = tempfile::tempdir().unwrap();
    let manifest = write_manifest(directory.path(), &parse_error_manifest_value());

    let output = command()
        .args([
            "gateway",
            "routes",
            "--manifest",
            manifest.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let value = json(&output.stdout);
    assert_eq!(value["ok"], false);
    assert_eq!(
        value["diagnostics"][0]["code"], "GHCLI009_GATEWAY_INVALID",
        "{value}"
    );

    let combined = combined_output(&output);
    assert!(
        !combined.contains(DISTINCTIVE_MARKER),
        "the manifest's file contents must never reach output: {combined}"
    );
}

// ---------------------------------------------------------------------------
// Test 3: `credential set` via stdin, then `probe` is green for the BYOK route.
// ---------------------------------------------------------------------------

#[test]
fn credential_set_then_probe_is_green_for_the_byok_route_without_leaking_the_value() {
    let directory = tempfile::tempdir().unwrap();
    let broker_dir = directory.path().join("broker");
    let keyring_dir = directory.path().join("keyring");
    std::fs::create_dir_all(&keyring_dir).unwrap();
    let manifest = write_manifest(directory.path(), &valid_manifest_value());
    let key = passphrase();

    let set_value = credential_set(
        &broker_dir,
        &keyring_dir,
        "test-key",
        "cred_anthropic",
        "anthropic",
        "anthropic_byok",
        &key,
        SENTINEL,
    );
    assert_eq!(set_value["ok"], true);
    assert_eq!(set_value["command"], "gateway.credential.set");
    assert_eq!(set_value["data"]["id"], "cred_anthropic");
    assert_eq!(set_value["data"]["provider"], "anthropic");
    assert_eq!(
        set_value["data"]["routes"],
        serde_json::json!(["anthropic_byok"])
    );
    assert!(!set_value.to_string().contains(SENTINEL));

    let probe_output = probe(
        &manifest,
        "anthropic_byok",
        Some(&broker_dir),
        Some(&keyring_dir),
        Some("test-key"),
        Some(&key),
    );
    assert!(
        probe_output.status.success(),
        "{}",
        combined_output(&probe_output)
    );
    let probe_value = json(&probe_output.stdout);
    assert_eq!(probe_value["ok"], true);
    assert_eq!(probe_value["data"]["route"], "anthropic_byok");
    assert_eq!(probe_value["data"]["health"], "available", "{probe_value}");
    assert_eq!(
        probe_value["data"]["checks"],
        serde_json::json!([{"name": "credential", "ok": true}])
    );
    assert!(!combined_output(&probe_output).contains(SENTINEL));
}

// ---------------------------------------------------------------------------
// Test 4: `probe` on a native runtime route whose `command.program` is `graphhelm` itself.
// ---------------------------------------------------------------------------

#[test]
fn probe_on_a_native_runtime_route_spawns_version_and_reports_available() {
    let directory = tempfile::tempdir().unwrap();
    let manifest = write_manifest(directory.path(), &valid_manifest_value());

    let output = probe(&manifest, "native_probe", None, None, None, None);
    assert!(output.status.success(), "{}", combined_output(&output));
    let value = json(&output.stdout);
    assert_eq!(value["ok"], true);
    assert_eq!(value["data"]["route"], "native_probe");
    assert_eq!(value["data"]["health"], "available", "{value}");
    assert_eq!(
        value["data"]["checks"],
        serde_json::json!([{"name": "runtime", "ok": true}])
    );
}

// ---------------------------------------------------------------------------
// Test 5: `credential remove` then `probe` reports `auth_required`.
// ---------------------------------------------------------------------------

#[test]
fn credential_remove_then_probe_reports_auth_required() {
    let directory = tempfile::tempdir().unwrap();
    let broker_dir = directory.path().join("broker");
    let keyring_dir = directory.path().join("keyring");
    std::fs::create_dir_all(&keyring_dir).unwrap();
    let manifest = write_manifest(directory.path(), &valid_manifest_value());
    let key = passphrase();

    credential_set(
        &broker_dir,
        &keyring_dir,
        "test-key",
        "cred_anthropic",
        "anthropic",
        "anthropic_byok",
        &key,
        SENTINEL,
    );

    let remove_output = command()
        .args([
            "gateway",
            "credential",
            "remove",
            "--broker",
            broker_dir.to_str().unwrap(),
            "--keyring",
            keyring_dir.to_str().unwrap(),
            "--key-id",
            "test-key",
            "--ref",
            "cred_anthropic",
        ])
        .env("GRAPHHELM_GATEWAY_KEY", &key)
        .output()
        .unwrap();
    assert!(
        remove_output.status.success(),
        "{}",
        combined_output(&remove_output)
    );
    let remove_value = json(&remove_output.stdout);
    assert_eq!(remove_value["ok"], true);
    assert_eq!(remove_value["command"], "gateway.credential.remove");
    assert_eq!(remove_value["data"]["id"], "cred_anthropic");
    assert_eq!(remove_value["data"]["revoked"], true);

    let probe_output = probe(
        &manifest,
        "anthropic_byok",
        Some(&broker_dir),
        Some(&keyring_dir),
        Some("test-key"),
        Some(&key),
    );
    assert!(
        probe_output.status.success(),
        "{}",
        combined_output(&probe_output)
    );
    let probe_value = json(&probe_output.stdout);
    assert_eq!(probe_value["ok"], true);
    assert_eq!(
        probe_value["data"]["health"], "auth_required",
        "{probe_value}"
    );
    assert_eq!(
        probe_value["data"]["checks"],
        serde_json::json!([{"name": "credential", "ok": false}])
    );
}

// ---------------------------------------------------------------------------
// Test 6: a missing `GRAPHHELM_GATEWAY_KEY` on `credential set` is refused as `GHCLI010`.
// ---------------------------------------------------------------------------

#[test]
fn credential_set_without_the_gateway_key_env_var_is_refused() {
    let directory = tempfile::tempdir().unwrap();
    let broker_dir = directory.path().join("broker");
    let keyring_dir = directory.path().join("keyring");
    std::fs::create_dir_all(&keyring_dir).unwrap();

    let output = command()
        .args([
            "gateway",
            "credential",
            "set",
            "--broker",
            broker_dir.to_str().unwrap(),
            "--keyring",
            keyring_dir.to_str().unwrap(),
            "--key-id",
            "test-key",
            "--ref",
            "cred_anthropic",
            "--provider",
            "anthropic",
            "--usable-by",
            "anthropic_byok",
        ])
        .env_remove("GRAPHHELM_GATEWAY_KEY")
        .write_stdin(SENTINEL)
        .output()
        .unwrap();
    assert!(!output.status.success());
    let value = json(&output.stdout);
    assert_eq!(value["ok"], false);
    assert_eq!(
        value["diagnostics"][0]["code"], "GHCLI010_GATEWAY_CREDENTIAL",
        "{value}"
    );
    assert!(
        value["diagnostics"][0]["message"]
            .as_str()
            .unwrap()
            .contains("GRAPHHELM_GATEWAY_KEY"),
        "the message must name the missing env var: {value}"
    );

    let combined = combined_output(&output);
    assert!(
        !combined.contains(SENTINEL),
        "the stdin value must never reach output: {combined}"
    );
    assert!(
        !broker_dir.join("credentials.json").exists(),
        "nothing may be stored when the passphrase could not be read"
    );
}

// ---------------------------------------------------------------------------
// Test 7 (IMPORTANT 9): `credential set` twice against the same directories both succeeds and
// preserves the first entry — `CredentialBroker::open_or_create` replaces a hand-copied
// exists-check that used to live in this CLI's own `credential.rs`.
// ---------------------------------------------------------------------------

#[test]
fn credential_set_twice_against_the_same_dirs_both_succeed_and_preserve_the_first() {
    let directory = tempfile::tempdir().unwrap();
    let broker_dir = directory.path().join("broker");
    let keyring_dir = directory.path().join("keyring");
    std::fs::create_dir_all(&keyring_dir).unwrap();
    let manifest = write_manifest(directory.path(), &valid_manifest_value());
    let key = passphrase();

    let first = credential_set(
        &broker_dir,
        &keyring_dir,
        "test-key",
        "cred_anthropic",
        "anthropic",
        "anthropic_byok",
        &key,
        SENTINEL,
    );
    assert_eq!(first["ok"], true);

    // A second `credential set`, for an UNRELATED reference id, against the exact same
    // `--broker`/`--keyring` directories. This must open the existing store, not re-create it.
    let second = credential_set(
        &broker_dir,
        &keyring_dir,
        "test-key",
        "cred_second",
        "anthropic",
        "anthropic_byok",
        &key,
        "sk-ant-SECOND-DISTINCT-VALUE",
    );
    assert_eq!(second["ok"], true);
    assert_eq!(second["data"]["id"], "cred_second");

    // The FIRST credential must still be present and leasable — never wiped by the second call.
    let probe_output = probe(
        &manifest,
        "anthropic_byok",
        Some(&broker_dir),
        Some(&keyring_dir),
        Some("test-key"),
        Some(&key),
    );
    assert!(
        probe_output.status.success(),
        "{}",
        combined_output(&probe_output)
    );
    let probe_value = json(&probe_output.stdout);
    assert_eq!(probe_value["data"]["health"], "available", "{probe_value}");
}

// ---------------------------------------------------------------------------
// Test 8 (IMPORTANT 5a): invalid UTF-8 on `credential set`'s stdin is refused explicitly, naming
// the encoding, under `GHCLI010` — never silently repaired via a lossy conversion.
// ---------------------------------------------------------------------------

#[test]
fn credential_set_with_invalid_utf8_on_stdin_is_refused_naming_the_encoding() {
    let directory = tempfile::tempdir().unwrap();
    let broker_dir = directory.path().join("broker");
    let keyring_dir = directory.path().join("keyring");
    std::fs::create_dir_all(&keyring_dir).unwrap();
    let key = passphrase();

    let output = command()
        .args([
            "gateway",
            "credential",
            "set",
            "--broker",
            broker_dir.to_str().unwrap(),
            "--keyring",
            keyring_dir.to_str().unwrap(),
            "--key-id",
            "test-key",
            "--ref",
            "cred_anthropic",
            "--provider",
            "anthropic",
            "--usable-by",
            "anthropic_byok",
        ])
        .env("GRAPHHELM_GATEWAY_KEY", &key)
        .write_stdin(vec![0xFFu8, 0xFE, 0x00, 0x01])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let value = json(&output.stdout);
    assert_eq!(value["ok"], false);
    assert_eq!(
        value["diagnostics"][0]["code"], "GHCLI010_GATEWAY_CREDENTIAL",
        "{value}"
    );
    let message = value["diagnostics"][0]["message"].as_str().unwrap();
    assert!(
        message.to_lowercase().contains("utf-8"),
        "the message must name the encoding: {message}"
    );
    assert!(
        !broker_dir.join("credentials.json").exists(),
        "nothing may be stored when the stdin value was not valid UTF-8"
    );
}

// ---------------------------------------------------------------------------
// Test 9 (MEDIUM 15): a manifest just over MAX_MANIFEST_BYTES is refused via its metadata length,
// not a downstream parse failure.
// ---------------------------------------------------------------------------

#[test]
fn routes_refuses_an_oversize_manifest_via_its_metadata_length() {
    let directory = tempfile::tempdir().unwrap();
    let manifest_path = directory.path().join("manifest.json");
    // One byte over the 256 KiB bound, and not valid JSON at all — if the bound were enforced
    // only after a full read+parse, this would still be refused, but for the wrong reason
    // (`ManifestError::Parse`, not the size bound); the message assertion below distinguishes
    // the two.
    let oversize = vec![b'x'; 256 * 1024 + 1];
    std::fs::write(&manifest_path, &oversize).unwrap();

    let output = command()
        .args([
            "gateway",
            "routes",
            "--manifest",
            manifest_path.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let value = json(&output.stdout);
    assert_eq!(value["ok"], false);
    assert_eq!(
        value["diagnostics"][0]["code"], "GHCLI009_GATEWAY_INVALID",
        "{value}"
    );
    let message = value["diagnostics"][0]["message"].as_str().unwrap();
    assert!(
        message.contains("maximum supported size"),
        "expected the metadata-based size refusal, not a parse error: {message}"
    );
}

// ---------------------------------------------------------------------------------------------
// #1171: `gateway route set` — the write `setup` cannot do, and the CLI half of
// `PUT /v1/gateway/routes`. Every cell here asserts on the FILE as well as on the envelope: a
// reply that says "written" while the manifest still holds the old bytes is the failure this
// surface can actually produce, and an envelope-only assertion cannot see it.
// ---------------------------------------------------------------------------------------------

fn route_set(manifest: &Path, extra: &[&str]) -> std::process::Output {
    command()
        .args(["gateway", "route", "set", "--manifest"])
        .arg(manifest)
        .args(extra)
        .output()
        .unwrap()
}

/// The arguments of one well-formed DeepSeek-shaped route: an `openai` WIRE FORMAT pointed at a
/// different vendor's endpoint, which is the case a provider-keyed credential default gets wrong.
fn deepseek_arguments(id: &str) -> Vec<String> {
    vec![
        "--id".to_owned(),
        id.to_owned(),
        "--provider".to_owned(),
        "openai".to_owned(),
        "--base-url".to_owned(),
        "https://api.deepseek.com".to_owned(),
        "--model".to_owned(),
        "deepseek-v4-pro".to_owned(),
    ]
}

fn as_str_arguments(arguments: &[String]) -> Vec<&str> {
    arguments.iter().map(String::as_str).collect()
}

#[test]
fn a_route_set_creates_the_manifest_and_the_listing_names_the_route() {
    let directory = tempfile::tempdir().unwrap();
    let manifest = directory.path().join("manifest.json");
    let arguments = deepseek_arguments("deepseek_official");

    let output = route_set(&manifest, &as_str_arguments(&arguments));
    assert!(output.status.success(), "{}", combined_output(&output));
    let value = json(&output.stdout);
    assert_eq!(value["ok"], true, "{value}");
    assert_eq!(value["data"]["manifest"]["state"], "created");
    assert_eq!(
        value["data"]["route"]["credentialRef"],
        "secret_deepseek_official"
    );
    assert_eq!(value["data"]["route"]["enabled"], true);
    assert_eq!(value["data"]["routes"][0]["id"], "deepseek_official");

    // The file is the claim, not the reply: `gateway routes` reads it back through the loader
    // every other surface uses, so a document only this command can parse would fail here.
    let listed = command()
        .args(["gateway", "routes", "--manifest"])
        .arg(&manifest)
        .output()
        .unwrap();
    assert!(listed.status.success(), "{}", combined_output(&listed));
    let listed = json(&listed.stdout);
    assert_eq!(listed["data"]["routes"][0]["id"], "deepseek_official");
    assert_eq!(listed["data"]["routes"][0]["model"], "deepseek-v4-pro");
}

#[test]
fn route_set_refuses_an_oversize_manifest_before_parsing_it() {
    let directory = tempfile::tempdir().unwrap();
    let manifest = directory.path().join("manifest.json");
    std::fs::write(&manifest, vec![b'x'; 256 * 1024 + 1]).unwrap();

    let arguments = deepseek_arguments("deepseek_official");
    let output = route_set(&manifest, &as_str_arguments(&arguments));
    assert!(!output.status.success(), "{}", combined_output(&output));
    let value = json(&output.stdout);
    assert!(
        value["diagnostics"][0]["message"]
            .as_str()
            .unwrap_or_default()
            .contains("maximum supported size"),
        "the write must use the bounded manifest reader: {value}"
    );
}

#[cfg(unix)]
#[test]
fn route_set_never_follows_a_preexisting_temporary_symlink() {
    use std::os::unix::fs::symlink;

    let directory = tempfile::tempdir().unwrap();
    let manifest = directory.path().join("manifest.json");
    let victim = directory.path().join("victim.txt");
    std::fs::write(&victim, b"sentinel").unwrap();
    symlink(&victim, directory.path().join(".manifest.json.tmp")).unwrap();

    let arguments = deepseek_arguments("deepseek_official");
    let output = route_set(&manifest, &as_str_arguments(&arguments));
    assert!(output.status.success(), "{}", combined_output(&output));
    assert_eq!(std::fs::read(&victim).unwrap(), b"sentinel");
    assert!(!std::fs::symlink_metadata(&manifest).unwrap().is_symlink());
}

#[cfg(unix)]
#[test]
fn gateway_manifest_reads_refuse_a_final_symlink() {
    use std::os::unix::fs::symlink;

    let directory = tempfile::tempdir().unwrap();
    let target = write_manifest(directory.path(), &valid_manifest_value());
    let link = directory.path().join("linked-manifest.json");
    symlink(&target, &link).unwrap();

    let output = command()
        .args(["gateway", "routes", "--manifest"])
        .arg(&link)
        .output()
        .unwrap();
    assert!(!output.status.success(), "{}", combined_output(&output));
    assert_eq!(json(&output.stdout)["diagnostics"][0]["path"], "/manifest");
}

#[cfg(unix)]
#[test]
fn route_set_refuses_a_symlinked_lock_file() {
    use std::os::unix::fs::symlink;

    let directory = tempfile::tempdir().unwrap();
    let manifest = directory.path().join("manifest.json");
    let victim = directory.path().join("victim.txt");
    std::fs::write(&victim, b"sentinel").unwrap();
    symlink(&victim, directory.path().join(".manifest.json.lock")).unwrap();

    let arguments = deepseek_arguments("deepseek_official");
    let output = route_set(&manifest, &as_str_arguments(&arguments));
    assert!(!output.status.success(), "{}", combined_output(&output));
    assert_eq!(std::fs::read(&victim).unwrap(), b"sentinel");
    assert!(!manifest.exists());
}

#[test]
fn concurrent_route_sets_preserve_every_distinct_route() {
    let directory = tempfile::tempdir().unwrap();
    let manifest = directory.path().join("manifest.json");
    let mut children = Vec::new();
    for index in 0..12 {
        let id = format!("route_{index}");
        let mut child = std::process::Command::new(assert_cmd::cargo::cargo_bin!("graphhelm"));
        child
            .args(["gateway", "route", "set", "--manifest"])
            .arg(&manifest)
            .args(deepseek_arguments(&id));
        children.push(child.spawn().unwrap());
    }
    for child in &mut children {
        assert!(child.wait().unwrap().success());
    }

    let document: Value = serde_json::from_slice(&std::fs::read(&manifest).unwrap()).unwrap();
    let routes = document["routes"].as_array().unwrap();
    assert_eq!(
        routes.len(),
        12,
        "no concurrent update may be lost: {document}"
    );
}

#[test]
fn a_second_write_with_the_same_id_is_refused_and_the_file_is_byte_identical() {
    let directory = tempfile::tempdir().unwrap();
    let manifest = directory.path().join("manifest.json");
    let arguments = deepseek_arguments("deepseek_official");
    let first = route_set(&manifest, &as_str_arguments(&arguments));
    assert!(first.status.success(), "{}", combined_output(&first));
    let before = std::fs::read(&manifest).unwrap();

    let second = route_set(&manifest, &as_str_arguments(&arguments));
    assert!(!second.status.success(), "{}", combined_output(&second));
    let value = json(&second.stdout);
    assert_eq!(value["ok"], false, "{value}");
    assert_eq!(value["diagnostics"][0]["path"], "/id");
    assert_eq!(
        std::fs::read(&manifest).unwrap(),
        before,
        "a refused write must leave the manifest byte-identical"
    );
}

#[test]
fn a_replace_rewrites_the_whole_entry_and_does_not_patch_the_fields_it_was_given() {
    let directory = tempfile::tempdir().unwrap();
    // The seeded route carries a field this command never writes. A replace that PATCHED the
    // named fields would leave it behind; one that rewrites the entry cannot. Without this field
    // the cell would pass against either implementation.
    let manifest = write_manifest(
        directory.path(),
        &serde_json::json!({
            "manifestVersion": 1,
            "routes": [{
                "id": "deepseek_official",
                "provider": "openai",
                "transport": "direct_api",
                "authentication": "api_key",
                "billingMode": "per_token",
                "baseUrl": "https://api.deepseek.com",
                "model": "deepseek-v4-pro",
                "credentialRef": "secret_deepseek_official",
                "profiles": ["balanced_reasoning"],
                "enabled": true,
                "timeoutSeconds": 900
            }]
        }),
    );

    let output = route_set(
        &manifest,
        &[
            "--id",
            "deepseek_official",
            "--provider",
            "openai",
            "--base-url",
            "https://api.deepseek.com",
            "--model",
            "deepseek-flash",
            "--replace",
        ],
    );
    assert!(output.status.success(), "{}", combined_output(&output));
    assert_eq!(
        json(&output.stdout)["data"]["manifest"]["state"],
        "replaced"
    );

    let written: Value = serde_json::from_slice(&std::fs::read(&manifest).unwrap()).unwrap();
    let route = &written["routes"][0];
    assert_eq!(route["model"], "deepseek-flash");
    assert!(
        route.get("timeoutSeconds").is_none(),
        "a replace must rewrite the entry, not patch the named fields: {route}"
    );
}

#[test]
fn the_credential_reference_is_keyed_to_the_route_id_not_the_provider() {
    let directory = tempfile::tempdir().unwrap();
    let manifest = directory.path().join("manifest.json");
    for id in ["deepseek_official", "openai_direct"] {
        let arguments = deepseek_arguments(id);
        let output = route_set(&manifest, &as_str_arguments(&arguments));
        assert!(output.status.success(), "{}", combined_output(&output));
    }

    let written: Value = serde_json::from_slice(&std::fs::read(&manifest).unwrap()).unwrap();
    let routes = written["routes"].as_array().unwrap();
    let references: Vec<&str> = routes
        .iter()
        .map(|route| route["credentialRef"].as_str().unwrap())
        .collect();
    assert_eq!(
        references,
        ["secret_deepseek_official", "secret_openai_direct"]
    );
    // Both routes declare `provider: "openai"`. A provider-keyed default would have named one
    // secret twice, and the second key stored under it would have replaced the first one's.
    let providers: Vec<&str> = routes
        .iter()
        .map(|route| route["provider"].as_str().unwrap())
        .collect();
    assert_eq!(providers, ["openai", "openai"]);
}

#[test]
fn a_native_runtime_route_is_never_rewritten_as_a_direct_api_one() {
    let directory = tempfile::tempdir().unwrap();
    let manifest = write_manifest(directory.path(), &valid_manifest_value());
    let before = std::fs::read(&manifest).unwrap();

    let output = route_set(
        &manifest,
        &[
            "--id",
            "native_probe",
            "--provider",
            "openai",
            "--base-url",
            "https://api.openai.com",
            "--model",
            "gpt-5",
            "--replace",
        ],
    );
    assert!(!output.status.success(), "{}", combined_output(&output));
    let value = json(&output.stdout);
    assert!(
        value["diagnostics"][0]["message"]
            .as_str()
            .unwrap_or_default()
            .contains("native_runtime"),
        "the refusal must name the transport it refused: {value}"
    );
    assert_eq!(
        std::fs::read(&manifest).unwrap(),
        before,
        "a refused replace must leave the manifest byte-identical"
    );
}

#[test]
fn a_route_the_loader_would_reject_never_reaches_disk() {
    let directory = tempfile::tempdir().unwrap();
    let manifest = directory.path().join("manifest.json");

    // `deepseek` is the VENDOR, not a wire format the BYOK adapters speak, so the loader refuses
    // it. What this cell pins is WHERE that refusal happens: before the write, not after.
    let output = route_set(
        &manifest,
        &[
            "--id",
            "deepseek_official",
            "--provider",
            "deepseek",
            "--base-url",
            "https://api.deepseek.com",
            "--model",
            "deepseek-v4-pro",
        ],
    );
    assert!(!output.status.success(), "{}", combined_output(&output));
    assert!(
        !manifest.exists(),
        "a refused write must not create the manifest it was about to hold"
    );
}

#[test]
fn a_route_written_disabled_is_listed_and_says_so() {
    let directory = tempfile::tempdir().unwrap();
    let manifest = directory.path().join("manifest.json");
    let mut arguments = deepseek_arguments("deepseek_official");
    arguments.push("--disabled".to_owned());

    let output = route_set(&manifest, &as_str_arguments(&arguments));
    assert!(output.status.success(), "{}", combined_output(&output));
    assert_eq!(json(&output.stdout)["data"]["route"]["enabled"], false);

    let listed = command()
        .args(["gateway", "routes", "--manifest"])
        .arg(&manifest)
        .output()
        .unwrap();
    assert!(listed.status.success(), "{}", combined_output(&listed));
    assert_eq!(json(&listed.stdout)["data"]["routes"][0]["enabled"], false);
}

/// #1305 follow-up: `gateway keyring init --keyring ~` must not chmod a home directory. A
/// directory that is not `0700` and already holds anything is refused with the message naming
/// the `0700` rule, and its mode is left as it was.
#[cfg(unix)]
#[test]
fn keyring_init_refuses_a_non_empty_0755_directory_naming_the_0700_rule() {
    use std::os::unix::fs::PermissionsExt;

    let directory = tempfile::tempdir().unwrap();
    let keyring_dir = directory.path().join("home");
    std::fs::create_dir_all(&keyring_dir).unwrap();
    std::fs::write(keyring_dir.join("notes.txt"), b"unrelated").unwrap();
    std::fs::set_permissions(&keyring_dir, std::fs::Permissions::from_mode(0o755)).unwrap();

    let output = command()
        .args(["gateway", "keyring", "init", "--keyring"])
        .arg(&keyring_dir)
        .args(["--key-id", "studio"])
        .env("GRAPHHELM_EVENTS_KEY", passphrase())
        .output()
        .unwrap();
    assert!(!output.status.success());
    let value = json(&output.stdout);
    assert_eq!(
        value["diagnostics"][0]["code"], "GHCLI010_GATEWAY_CREDENTIAL",
        "{value}"
    );
    assert!(
        value["diagnostics"][0]["message"]
            .as_str()
            .unwrap()
            .contains("mode 0700 (owner-only)"),
        "{value}"
    );
    let mode = std::fs::metadata(&keyring_dir)
        .unwrap()
        .permissions()
        .mode()
        & 0o7777;
    assert_eq!(mode, 0o755);
}
