//! Only this unit-test module may construct synthetic custody. It proves a boundary, not a host.
use super::*;
#[path = "activation_fixture.rs"]
mod fixture;

fn setup() -> (
    tempfile::TempDir,
    tempfile::TempDir,
    tempfile::TempDir,
    Value,
    Value,
) {
    let p = tempfile::tempdir().unwrap();
    let h = tempfile::tempdir().unwrap();
    let s = tempfile::tempdir().unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(s.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    std::fs::write(
        p.path().join("AGENTS.md"),
        b"factory\nPrefer concise replies\nDeny secrets\n",
    )
    .unwrap();
    let plan = fixture::plan(crate::root_bindings(p.path(), h.path()).unwrap());
    let installed = crate::apply(
        p.path(),
        h.path(),
        s.path(),
        &plan,
        plan["digest"].as_str().unwrap(),
    )
    .unwrap();
    let receipt = fixture::receipt(
        &plan,
        installed["installedAtUnixMs"]
            .as_u64()
            .expect("durable installation timestamp"),
    );
    (p, h, s, plan, receipt)
}

fn custody(receipt: &Value) -> Custody {
    Custody {
        receipt_digest: receipt["digest"].clone(),
        observer: receipt["spec"]["observer"].clone(),
        session: receipt["spec"]["session"].clone(),
        environment: receipt["spec"]["environment"].clone(),
        now_ms: receipt["spec"]["timestamps"]["observedAtUnixMs"]
            .as_u64()
            .unwrap()
            + 1,
    }
}

#[test]
fn fixture_custody_persists_only_after_current_anchored_files_match() {
    let (p, _h, s, plan, receipt) = setup();
    let trusted = custody(&receipt);
    let result = verify_at(s.path(), &plan, &receipt, Some(&trusted)).unwrap();
    assert_eq!(result["spec"]["state"], "verified");
    assert_eq!(result["verification"]["fixtureOnly"], true);
    let store = crate::journal::Store::reader(crate::storage::Root::open(s.path(), false).unwrap())
        .unwrap();
    assert_eq!(store.active().unwrap().unwrap().receipt.unwrap(), result);
    std::fs::write(p.path().join("AGENTS.md"), b"factory reenabled\n").unwrap();
    assert!(verify_at(s.path(), &plan, &receipt, Some(&trusted)).is_err());
}

#[test]
fn no_custody_reports_observer_missing_without_promoting_journal() {
    let (_p, _h, s, plan, receipt) = setup();
    let result = verify_activation_at(s.path(), &plan, &receipt).unwrap();
    assert_eq!(result["spec"]["state"], "installed_unverified");
    assert_eq!(result["verification"]["status"], "observer_missing");
}

#[test]
fn custody_rejects_resealed_receipts_expired_sessions_and_wrong_environment() {
    let (_p, _h, s, plan, receipt) = setup();
    let trusted = custody(&receipt);
    for pointer in [
        "/spec/observer/identity",
        "/spec/mcp/runtimeObservationDigest",
        "/spec/methodology/evidenceDigest",
        "/spec/session/id",
        "/spec/environment/home",
    ] {
        let mut changed = receipt.clone();
        *changed.pointer_mut(pointer).unwrap() = if pointer.ends_with("Digest") {
            json!(format!("sha256:{}", "f".repeat(64)))
        } else if pointer == "/spec/environment/home" {
            json!("f".repeat(64))
        } else {
            json!("changed")
        };
        assert!(
            verify_at(s.path(), &plan, &fixture::seal(changed), Some(&trusted)).is_err(),
            "{pointer}"
        );
    }
    let mut expired = custody(&receipt);
    expired.now_ms = receipt["spec"]["timestamps"]["expiresAtUnixMs"]
        .as_u64()
        .unwrap()
        + 1;
    assert!(verify_at(s.path(), &plan, &receipt, Some(&expired)).is_err());
    let mut old = receipt.clone();
    old["spec"]["timestamps"]["installedAtUnixMs"] = json!(1);
    let old = fixture::seal(old);
    assert!(verify_at(s.path(), &plan, &old, Some(&custody(&old))).is_err());
}

#[cfg(windows)]
fn fake_host(root: &std::path::Path, codex: bool) -> std::path::PathBuf {
    let executable = root.join("inert-host.exe");
    let source =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/inert-host.rs");
    assert!(
        std::process::Command::new("rustc")
            .args(["--edition=2024"])
            .arg(source)
            .arg("-o")
            .arg(&executable)
            .status()
            .unwrap()
            .success()
    );
    std::fs::write(
        executable.with_extension("input"),
        if codex {
            "codex-cli 0.114.0\nplugins browser\n"
        } else {
            "2.1.265 (Claude Code)\n--plugin-dir\n"
        },
    )
    .unwrap();
    executable
}

#[test]
#[cfg(windows)]
fn full_fixture_journey_preserves_two_skills_preferences_denies_and_later_user_key() {
    let (p, h, s, mut plan, _) = setup();
    // Restore the first fixture so the complete accepted transaction starts from its original.
    let restore = crate::plan_restore(s.path(), "original").unwrap();
    crate::apply_restore(s.path(), &restore, restore["digest"].as_str().unwrap()).unwrap();
    let program = fake_host(p.path(), true);
    std::fs::create_dir(h.path().join(".codex")).unwrap();
    let config = "personal = \"concise\"\n[permissions]\ndeny = [\"secrets\"]\n[[skills.config]]\npath = \"compatible-a\"\nenabled = true\n[[skills.config]]\npath = \"compatible-b\"\nenabled = true\n[[skills.config]]\npath = \"factory\"\nenabled = true\n";
    let skill = h
        .path()
        .join("factory/SKILL.md")
        .to_string_lossy()
        .replace(char::from(92), "/");
    let config = config.replace("path = \"factory\"", &format!("path = \"{skill}\""));
    let after =
        crate::hosts::codex::disable_skills(&config, std::slice::from_ref(&skill), true).unwrap();
    std::fs::write(h.path().join(".codex/config.toml"), &config).unwrap();
    plan["spec"]["host"] =
        json!({"name":"codex","version":"0.114.0","program":program,"mode":"configuration"});
    plan["spec"]["scopes"] = json!(["project", "user"]);
    plan["spec"]["decisions"]
        .as_array_mut()
        .unwrap()
        .push(json!({"operationIndex":1,"decision":"replace","protected":false}));
    plan["spec"]["operations"].as_array_mut().unwrap().push(json!({"root":"home","path":".codex/config.toml","beforeDigest":fixture::hash(config.as_bytes()),"afterDigest":fixture::hash(after.as_bytes()),"after":after,"disableSkills":[skill]}));
    let plan = fixture::seal(plan);
    let installed = crate::apply(
        p.path(),
        h.path(),
        s.path(),
        &plan,
        plan["digest"].as_str().unwrap(),
    )
    .unwrap();
    let baseline = std::fs::read(s.path().join("original.json")).unwrap();
    let receipt = fixture::receipt(&plan, installed["installedAtUnixMs"].as_u64().unwrap());
    let verified = verify_at(s.path(), &plan, &receipt, Some(&custody(&receipt))).unwrap();
    assert_eq!(verified["spec"]["state"], "verified");
    assert_eq!(verified["verification"]["fixtureOnly"], true);
    assert_eq!(
        crate::apply(
            p.path(),
            h.path(),
            s.path(),
            &plan,
            plan["digest"].as_str().unwrap()
        )
        .unwrap(),
        verified
    );
    assert_eq!(
        std::fs::read(s.path().join("original.json")).unwrap(),
        baseline
    );
    let installed: toml::Value =
        toml::from_str(&std::fs::read_to_string(h.path().join(".codex/config.toml")).unwrap())
            .unwrap();
    let skills = installed["skills"]["config"].as_array().unwrap();
    assert_eq!(skills[0]["enabled"].as_bool(), Some(true));
    assert_eq!(skills[1]["enabled"].as_bool(), Some(true));
    assert_eq!(skills[2]["enabled"].as_bool(), Some(false));
    let mut edited = installed;
    edited
        .as_table_mut()
        .unwrap()
        .insert("user_added".into(), toml::Value::String("keep me".into()));
    std::fs::write(
        h.path().join(".codex/config.toml"),
        toml::to_string(&edited).unwrap(),
    )
    .unwrap();
    let restore = crate::plan_restore(s.path(), "original").unwrap();
    let restored =
        crate::apply_restore(s.path(), &restore, restore["digest"].as_str().unwrap()).unwrap();
    assert_eq!(restored["spec"]["state"], "restored");
    let restored: toml::Value =
        toml::from_str(&std::fs::read_to_string(h.path().join(".codex/config.toml")).unwrap())
            .unwrap();
    assert_eq!(restored["user_added"].as_str(), Some("keep me"));
    assert_eq!(
        restored["skills"]["config"][2]["enabled"].as_bool(),
        Some(true)
    );
    assert_eq!(
        std::fs::read(p.path().join("AGENTS.md")).unwrap(),
        b"factory\nPrefer concise replies\nDeny secrets\n"
    );
}

#[test]
#[cfg(windows)]
fn changed_package_bytes_prevent_even_fixture_verification() {
    let (p, h, s, mut plan, _) = setup();
    let restore = crate::plan_restore(s.path(), "original").unwrap();
    crate::apply_restore(s.path(), &restore, restore["digest"].as_str().unwrap()).unwrap();
    let program = fake_host(p.path(), false);
    let packages = crate::hosts::release_packages().unwrap();
    plan["spec"]["host"] =
        json!({"name":"claude","version":"2.1.265","program":program,"mode":"local_plugin_dir"});
    plan["spec"]["packages"] = json!(
        packages
            .iter()
            .map(|p| json!({"id":p.id,"version":p.version,"digest":p.digest}))
            .collect::<Vec<_>>()
    );
    let plan = fixture::seal(plan);
    let installed = crate::apply(
        p.path(),
        h.path(),
        s.path(),
        &plan,
        plan["digest"].as_str().unwrap(),
    )
    .unwrap();
    let receipt = fixture::receipt(&plan, installed["installedAtUnixMs"].as_u64().unwrap());
    let trusted = custody(&receipt);
    verify_at(s.path(), &plan, &receipt, Some(&trusted)).unwrap();
    let package = std::path::Path::new(installed["packages"][0]["path"].as_str().unwrap());
    std::fs::write(package.join("extension.json"), b"changed package").unwrap();
    assert!(verify_at(s.path(), &plan, &receipt, Some(&trusted)).is_err());
}
