use graphhelm_host_adoption::hosts::{HostOperation, run_host};
use std::path::Path;

#[test]
#[cfg(windows)]
fn restore_is_offline_and_cannot_disable_another_projects_user_plugins() {
    let (p, h, s) = roots();
    let executable = fake_host(p.path(), "2.1.265 (Claude Code)", "--plugin-dir --help");
    let plan = package_plan(p.path(), h.path(), &executable, "claude", "2.1.265");
    let result = graphhelm_host_adoption::apply(
        p.path(),
        h.path(),
        s.path(),
        &plan,
        plan["digest"].as_str().unwrap(),
    )
    .unwrap();
    let (other, _, other_state) = roots();
    let executable2 = fake_host(other.path(), "2.1.265 (Claude Code)", "--plugin-dir --help");
    let second = package_plan(other.path(), h.path(), &executable2, "claude", "2.1.265");
    assert!(
        graphhelm_host_adoption::apply(
            other.path(),
            h.path(),
            other_state.path(),
            &second,
            second["digest"].as_str().unwrap()
        )
        .is_err()
    );
    assert_eq!(std::fs::read_dir(other_state.path()).unwrap().count(), 0);
    std::fs::remove_file(executable).unwrap();
    let restore = graphhelm_host_adoption::plan_restore(s.path(), "original").unwrap();
    let receipt = graphhelm_host_adoption::apply_restore(
        s.path(),
        &restore,
        restore["digest"].as_str().unwrap(),
    )
    .unwrap();
    assert_eq!(receipt["spec"]["state"], "restored");
    for package in result["packages"].as_array().unwrap() {
        let version = Path::new(package["path"].as_str().unwrap());
        let root = version.parent().unwrap().parent().unwrap();
        assert!(!root.join("active.json").exists());
        assert!(root.join("inactive.json").exists());
    }
}

#[test]
#[cfg(windows)]
fn externally_modified_created_activation_pointer_is_retained_as_conflict() {
    let (p, h, s) = roots();
    let executable = fake_host(p.path(), "2.1.265 (Claude Code)", "--plugin-dir --help");
    let plan = package_plan(p.path(), h.path(), &executable, "claude", "2.1.265");
    let result = graphhelm_host_adoption::apply(
        p.path(),
        h.path(),
        s.path(),
        &plan,
        plan["digest"].as_str().unwrap(),
    )
    .unwrap();
    let path = Path::new(result["packages"][0]["path"].as_str().unwrap())
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("active.json");
    let mut edited = std::fs::read(&path).unwrap();
    edited.push(b' ');
    std::fs::write(&path, &edited).unwrap();
    let restore = graphhelm_host_adoption::plan_restore(s.path(), "original").unwrap();
    assert!(!restore["spec"]["conflicts"].as_array().unwrap().is_empty());
    let receipt = graphhelm_host_adoption::apply_restore(
        s.path(),
        &restore,
        restore["digest"].as_str().unwrap(),
    )
    .unwrap();
    assert_eq!(receipt["spec"]["state"], "recovery_required");
    assert_eq!(std::fs::read(path).unwrap(), edited);
}

#[test]
fn a_plugin_identifier_is_one_argument() {
    let op = graphhelm_host_adoption::hosts::claude::plugin_install(
        Path::new("claude"),
        "graphhelm-jpd@local;echo marker",
        "project",
    );
    assert_eq!(
        op.args,
        [
            "plugin",
            "install",
            "graphhelm-jpd@local;echo marker",
            "--scope",
            "project"
        ]
    );
    assert_eq!(op.timeout_ms, 10_000);
    assert!(run_host(&op).is_err());
}

#[test]
fn unsupported_scope_is_rejected_before_start() {
    let op = graphhelm_host_adoption::hosts::claude::plugin_install(
        Path::new("missing-host"),
        "valid@local",
        "managed",
    );
    assert_eq!(
        run_host(&op).unwrap_err().reason.pointer(),
        "/adoption/invalid_configuration"
    );
}

#[test]
fn incomplete_and_option_shaped_plugin_identifiers_are_rejected() {
    for id in [
        "old@",
        "old@-market",
        "old@@market",
        "../old",
        "old;echo marker",
    ] {
        let operation = graphhelm_host_adoption::hosts::claude::plugin_install(
            &std::env::current_exe().unwrap(),
            id,
            "project",
        );
        assert!(
            run_host(&operation).is_err(),
            "identifier reached the fixture: {id}"
        );
    }
}

// This integration-test binary is also the inert fake host. The supervisor launches only this
// exact ignored test, with no shell, host account, credentials, network or installed host.
#[test]
#[ignore]
fn inert_host() {
    use std::io::Write;
    let mode = std::env::args().next_back().unwrap();
    if mode == "--nocapture" {
        return;
    }
    let value: serde_json::Value = serde_json::from_slice(&std::fs::read(mode).unwrap()).unwrap();
    if let Some(marker) = value["spawnMarker"].as_str() {
        std::fs::write(marker, b"host started").unwrap();
    }
    #[cfg(unix)]
    if value["escapeSession"] == true {
        assert!(unsafe { libc::setsid() } >= 0);
    }
    if let Some(pid_file) = value["pidFile"].as_str() {
        std::fs::write(pid_file, std::process::id().to_string()).unwrap();
    }
    if value["hang"] == true {
        std::thread::sleep(std::time::Duration::from_secs(30));
    }
    if value["descendant"] == true {
        let mut command = std::process::Command::new(std::env::current_exe().unwrap());
        command.args([
            "--exact",
            "inert_host",
            "--ignored",
            "--nocapture",
            "--",
            value["childInput"].as_str().unwrap(),
        ]);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(windows_sys::Win32::System::Threading::CREATE_NEW_PROCESS_GROUP);
        }
        let child = command.spawn().unwrap();
        std::mem::forget(child);
    }
    let output = vec![b'x'; value["bytes"].as_u64().unwrap_or(7) as usize];
    std::io::stdout().write_all(&output).unwrap();
    std::io::stderr().write_all(&output).unwrap();
}

#[test]
fn timeout_cannot_leave_a_session_escaping_descendant_running() {
    let temp = tempfile::tempdir().unwrap();
    let pid_file = temp.path().join("escaped.pid");
    let spawn_marker = temp.path().join("spawned");
    let path = temp.path().join("escape.json");
    std::fs::write(
        &path,
        serde_json::to_vec(
            &serde_json::json!({"hang":true,"escapeSession":true,"pidFile":pid_file}),
        )
        .unwrap(),
    )
    .unwrap();
    let (_fixture, operation) = fixture(
        serde_json::json!({"descendant":true,"childInput":path,"spawnMarker":spawn_marker}),
        2000,
    );
    match run_host(&operation) {
        Err(error) => {
            #[cfg(windows)]
            {
                panic!(
                    "Windows containment must return a reply after proving cleanup: {}",
                    error.reason.pointer()
                );
            }
            #[cfg(not(windows))]
            {
                assert_eq!(
                    error.reason.pointer(),
                    "/adoption/host_containment_unavailable"
                );
                assert!(!spawn_marker.exists(), "refused host was started");
                assert!(!pid_file.exists(), "refused host created a descendant");
            }
        }
        Ok(reply) => {
            assert!(reply.timed_out);
            let pid: u32 = std::fs::read_to_string(&pid_file)
                .expect("descendant reports its PID before timeout")
                .parse()
                .unwrap();
            let alive = process_alive(pid);
            // Clean up the deliberately escaping RED fixture before reporting the regression.
            #[cfg(unix)]
            if alive {
                unsafe {
                    libc::kill(pid as i32, libc::SIGKILL);
                }
            }
            assert!(
                !alive,
                "descendant {pid} escaped containment and survived runner return"
            );
        }
    }
}

fn process_alive(pid: u32) -> bool {
    #[cfg(unix)]
    {
        match std::fs::read_to_string(format!("/proc/{pid}/stat")) {
            Ok(stat) => !matches!(
                stat.rsplit_once(") ").unwrap().1.chars().next(),
                Some('Z' | 'X')
            ),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
            Err(error) => panic!("could not observe descendant {pid}: {error}"),
        }
    }
    #[cfg(windows)]
    {
        use windows_sys::Win32::System::Threading::{
            OpenProcess, PROCESS_SYNCHRONIZE, WaitForSingleObject,
        };
        unsafe {
            let handle = OpenProcess(PROCESS_SYNCHRONIZE, 0, pid);
            if handle.is_null() {
                assert_eq!(
                    windows_sys::Win32::Foundation::GetLastError(),
                    87,
                    "process liveness query failed"
                );
                return false;
            }
            let status = WaitForSingleObject(handle, 0);
            windows_sys::Win32::Foundation::CloseHandle(handle);
            match status {
                0 => false,
                258 => true,
                other => panic!("process liveness query returned {other}"),
            }
        }
    }
}

#[cfg(not(windows))]
#[test]
fn unsupported_containment_refuses_before_any_host_side_effect() {
    let temp = tempfile::tempdir().unwrap();
    let marker = temp.path().join("spawned");
    let (_fixture, operation) = fixture(serde_json::json!({"spawnMarker":marker}), 1000);
    let result = run_host(&operation);
    assert!(!marker.exists(), "unsupported containment started the host");
    assert_eq!(
        result.unwrap_err().reason.pointer(),
        "/adoption/host_containment_unavailable"
    );
}

fn fixture(value: serde_json::Value, timeout_ms: u64) -> (tempfile::TempDir, HostOperation) {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("input.json");
    std::fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
    let op = HostOperation {
        program: std::env::current_exe().unwrap(),
        args: vec![
            "--exact".into(),
            "inert_host".into(),
            "--ignored".into(),
            "--nocapture".into(),
            "--".into(),
            path.to_str().unwrap().into(),
        ],
        timeout_ms,
    };
    (temp, op)
}

#[test]
#[cfg(windows)]
fn successful_host_captures_bounded_output() {
    let (_temp, op) = fixture(serde_json::json!({"bytes": 300_000}), 4000);
    let reply = run_host(&op).unwrap();
    assert!(reply.success, "{reply:?}");
    assert!(!reply.timed_out);
    assert_eq!(reply.stdout.len(), 65536);
    assert_eq!(reply.stderr.len(), 65536);
}

#[test]
#[cfg(windows)]
fn timeout_includes_termination_and_inherited_pipe_collection() {
    let child = tempfile::tempdir().unwrap();
    let path = child.path().join("hang.json");
    std::fs::write(&path, br#"{"hang":true}"#).unwrap();
    let (_temp, op) = fixture(
        serde_json::json!({"descendant":true,"childInput":path}),
        500,
    );
    let start = std::time::Instant::now();
    let reply = run_host(&op).unwrap();
    assert!(reply.timed_out);
    assert!(!reply.success);
    assert!(start.elapsed() < std::time::Duration::from_secs(3));
}

#[test]
fn only_accepted_claude_plugin_ids_change() {
    use graphhelm_host_adoption::hosts::claude::disable_plugins;
    let before = serde_json::json!({"enabledPlugins":{"old@local":true,"keep@local":true},"permissions":{"deny":["Read(.env)"]},"mcpServers":{"keep":{"command":"untouched"}},"hooks":{"SessionStart":[{"command":"never-run"}]}});
    let next = disable_plugins(&before, &["old@local".into()], &serde_json::json!({})).unwrap();
    assert_eq!(next["enabledPlugins"]["old@local"], false);
    assert_eq!(next["enabledPlugins"]["keep@local"], true);
    for key in ["permissions", "mcpServers", "hooks"] {
        assert_eq!(before[key], next[key]);
    }
    assert!(
        disable_plugins(
            &before,
            &["old@local".into()],
            &serde_json::json!({"enabledPlugins":{"old@local":true}})
        )
        .is_err()
    );
    assert!(disable_plugins(&before, &["missing@local".into()], &serde_json::json!({})).is_err());
}

#[test]
fn codex_uses_only_supported_skill_configuration() {
    let before = "model = 'keep'\n[mcp_servers.keep]\ncommand = 'untouched'\n";
    let path = std::env::temp_dir()
        .join("skills/old/SKILL.md")
        .to_str()
        .unwrap()
        .replace('\\', "/");
    let result =
        graphhelm_host_adoption::hosts::codex::disable_skills(before, &[path], true).unwrap();
    let value: toml::Value = toml::from_str(&result).unwrap();
    assert_eq!(value["model"].as_str(), Some("keep"));
    assert_eq!(
        value["mcp_servers"]["keep"]["command"].as_str(),
        Some("untouched")
    );
    assert_eq!(
        value["skills"]["config"][0]["enabled"].as_bool(),
        Some(false)
    );
    assert!(
        graphhelm_host_adoption::hosts::codex::disable_skills(
            before,
            &["/skills/old/SKILL.md".into()],
            false
        )
        .is_err()
    );
}

fn seal(mut plan: serde_json::Value) -> serde_json::Value {
    use sha2::{Digest, Sha256};
    plan.as_object_mut().unwrap().remove("digest");
    plan["digest"] = serde_json::json!(format!(
        "sha256:{}",
        hex::encode(Sha256::digest(
            graphhelm_graph::canonical_content_bytes(&plan).unwrap()
        ))
    ));
    plan
}
fn hash(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    hex::encode(Sha256::digest(bytes))
}
fn host_plan(p: &Path, h: &Path, before: &[u8], after: &str) -> serde_json::Value {
    seal(
        serde_json::json!({"apiVersion":"p50.dev/adoption/v1","kind":"AdoptionPlan","id":"hosts",
        "spec":{"coverage":"complete","rootBindings":graphhelm_host_adoption::root_bindings(p,h).unwrap(),"scopes":["project"],"packages":[],"hostBoundary":"quiescent",
        "decisions":[{"operationIndex":0,"decision":"replace","protected":false}],
        "operations":[{"root":"project","path":".claude/settings.local.json","beforeDigest":hash(before),"afterDigest":hash(after.as_bytes()),"after":after,"disablePlugins":["old@local"]}]}}),
    )
}

#[test]
fn claude_disable_requires_complete_policy_observation_before_mutation() {
    let p = tempfile::tempdir().unwrap();
    let h = tempfile::tempdir().unwrap();
    let s = tempfile::tempdir().unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(s.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    std::fs::create_dir(p.path().join(".claude")).unwrap();
    let before = br#"{"enabledPlugins":{"old@local":true,"keep@local":true},"permissions":{"deny":["Read(.env)"]}}"#;
    let after = r#"{"enabledPlugins":{"old@local":false,"keep@local":true},"permissions":{"deny":["Read(.env)"]}}"#;
    let path = p.path().join(".claude/settings.local.json");
    std::fs::write(&path, before).unwrap();
    let plan = host_plan(p.path(), h.path(), before, after);
    let error = graphhelm_host_adoption::apply(
        p.path(),
        h.path(),
        s.path(),
        &plan,
        plan["digest"].as_str().unwrap(),
    )
    .unwrap_err();
    assert_eq!(error.reason.pointer(), "/adoption/host_action_required");
    assert_eq!(std::fs::read(path).unwrap(), before);
    assert_eq!(std::fs::read_dir(s.path()).unwrap().count(), 0);
}

fn fake_host(dir: &Path, version: &str, help: &str) -> std::path::PathBuf {
    static BINARY: std::sync::OnceLock<tempfile::TempDir> = std::sync::OnceLock::new();
    let fixture = BINARY.get_or_init(|| {
        let temp = tempfile::tempdir().unwrap();
        let result = std::process::Command::new("rustc")
            .args(["--edition=2024", "tests/fixtures/inert-host.rs", "-o"])
            .arg(temp.path().join("host.exe"))
            .current_dir(env!("CARGO_MANIFEST_DIR"))
            .status()
            .unwrap();
        assert!(result.success());
        temp
    });
    let path = dir.join("host.exe");
    std::fs::copy(fixture.path().join("host.exe"), &path).unwrap();
    std::fs::write(path.with_extension("input"), format!("{version}\n{help}\n")).unwrap();
    path
}

fn package_plan(
    p: &Path,
    h: &Path,
    program: &Path,
    name: &str,
    version: &str,
) -> serde_json::Value {
    let packages = graphhelm_host_adoption::hosts::release_packages().unwrap();
    let mut plan = host_plan(p, h, b"{}", "{}");
    plan["spec"]["operations"][0]["disablePlugins"] = serde_json::json!([]);
    plan["spec"]["packages"] = serde_json::to_value(
        packages
            .iter()
            .map(|p| serde_json::json!({"id":p.id,"version":p.version,"digest":p.digest}))
            .collect::<Vec<_>>(),
    )
    .unwrap();
    plan["spec"]["host"] = serde_json::json!({"name":name,"version":version,"program":program,"mode":"local_plugin_dir"});
    seal(plan)
}

fn roots() -> (tempfile::TempDir, tempfile::TempDir, tempfile::TempDir) {
    let p = tempfile::tempdir().unwrap();
    let h = tempfile::tempdir().unwrap();
    let s = tempfile::tempdir().unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(s.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    std::fs::create_dir(p.path().join(".claude")).unwrap();
    std::fs::write(p.path().join(".claude/settings.local.json"), b"{}").unwrap();
    (p, h, s)
}

#[test]
#[cfg(windows)]
fn both_pinned_packages_install_with_local_loading_and_remain_unverified() {
    let (p, h, s) = roots();
    let executable = fake_host(p.path(), "2.1.265 (Claude Code)", "--plugin-dir --help");
    let plan = package_plan(p.path(), h.path(), &executable, "claude", "2.1.265");
    let result = graphhelm_host_adoption::apply(
        p.path(),
        h.path(),
        s.path(),
        &plan,
        plan["digest"].as_str().unwrap(),
    )
    .unwrap();
    assert_eq!(result["spec"]["state"], "installed_unverified");
    assert_eq!(result["packages"].as_array().unwrap().len(), 2);
    assert_eq!(result["hostAction"]["args"].as_array().unwrap().len(), 4);
    assert_eq!(result["hostAction"]["args"][0], "--plugin-dir");
    for package in result["packages"].as_array().unwrap() {
        let installed = graphhelm_schema::validate_extension_package(Path::new(
            package["path"].as_str().unwrap(),
        ))
        .unwrap();
        assert_eq!(installed.package_digest, package["digest"]);
    }
}

/// #1208 F6: the least invasive adoption, the two pinned packages and no file change, is a plan
/// `apply` accepts, and restore takes the activation back.
#[test]
#[cfg(windows)]
fn packages_alone_install_without_a_file_operation_and_restore() {
    let (p, h, s) = roots();
    let executable = fake_host(p.path(), "2.1.265 (Claude Code)", "--plugin-dir --help");
    let mut plan = package_plan(p.path(), h.path(), &executable, "claude", "2.1.265");
    plan["spec"]["operations"] = serde_json::json!([]);
    plan["spec"]["decisions"] = serde_json::json!([]);
    let plan = seal(plan);
    let settings = std::fs::read(p.path().join(".claude/settings.local.json")).unwrap();
    let result = graphhelm_host_adoption::apply(
        p.path(),
        h.path(),
        s.path(),
        &plan,
        plan["digest"].as_str().unwrap(),
    )
    .unwrap();
    assert_eq!(result["spec"]["state"], "installed_unverified");
    assert_eq!(result["packages"].as_array().unwrap().len(), 2);
    assert_eq!(
        std::fs::read(p.path().join(".claude/settings.local.json")).unwrap(),
        settings
    );
    let restore = graphhelm_host_adoption::plan_restore(s.path(), "original").unwrap();
    let receipt = graphhelm_host_adoption::apply_restore(
        s.path(),
        &restore,
        restore["digest"].as_str().unwrap(),
    )
    .unwrap();
    assert_eq!(receipt["spec"]["state"], "restored");
    for package in result["packages"].as_array().unwrap() {
        let version = Path::new(package["path"].as_str().unwrap());
        let root = version.parent().unwrap().parent().unwrap();
        assert!(!root.join("active.json").exists());
        assert!(root.join("inactive.json").exists());
    }
}

#[cfg(not(windows))]
#[test]
fn package_host_preflight_requires_containment_before_any_probe_or_mutation() {
    for host in ["claude", "codex"] {
        let (p, h, s) = roots();
        let marker = p.path().join("probe-started");
        let program = fake_host(p.path(), "2.1.265", "--plugin-dir");
        std::fs::write(
            program.with_extension("input"),
            format!("2.1.265\n--plugin-dir\n\n{}\n", marker.display()),
        )
        .unwrap();
        // Positive fixture control: any actual probe writes its marker before parsing argv.
        assert!(
            std::process::Command::new(&program)
                .arg("--version")
                .output()
                .unwrap()
                .status
                .success()
        );
        assert!(marker.exists());
        std::fs::remove_file(&marker).unwrap();
        let plan = package_plan(p.path(), h.path(), &program, host, "2.1.265");
        let result = graphhelm_host_adoption::apply(
            p.path(),
            h.path(),
            s.path(),
            &plan,
            plan["digest"].as_str().unwrap(),
        )
        .unwrap_err();
        assert_eq!(
            result.reason.pointer(),
            "/adoption/host_containment_unavailable"
        );
        assert!(!marker.exists());
        assert_eq!(
            std::fs::read(p.path().join(".claude/settings.local.json")).unwrap(),
            b"{}"
        );
        assert!(!p.path().join(".graphhelm-adoption.lock").exists());
        assert!(!h.path().join(".graphhelm-adoption.lock").exists());
        assert_eq!(std::fs::read_dir(s.path()).unwrap().count(), 0);
    }
}

#[test]
fn unavailable_plugin_support_changes_zero_host_files() {
    for (name, version, help, reason) in [
        (
            "codex",
            "0.114.0",
            "plugins browser",
            "/adoption/host_action_required",
        ),
        ("claude", "2.1.265", "usage", "/adoption/host_unsupported"),
        (
            "claude",
            "1.0.0",
            "--plugin-dir",
            "/adoption/host_unsupported",
        ),
    ] {
        let (p, h, s) = roots();
        let executable = fake_host(p.path(), &format!("{version} ({name})"), help);
        let plan = package_plan(p.path(), h.path(), &executable, name, version);
        let result = graphhelm_host_adoption::apply(
            p.path(),
            h.path(),
            s.path(),
            &plan,
            plan["digest"].as_str().unwrap(),
        )
        .unwrap_err();
        assert_eq!(
            result.reason.pointer(),
            if cfg!(windows) {
                reason
            } else {
                "/adoption/host_containment_unavailable"
            }
        );
        assert_eq!(
            std::fs::read(p.path().join(".claude/settings.local.json")).unwrap(),
            b"{}"
        );
        assert!(!p.path().join(".graphhelm-adoption.lock").exists());
        assert_eq!(std::fs::read_dir(s.path()).unwrap().count(), 0);
    }
}

#[test]
fn changed_package_pin_is_rejected_before_host_writes() {
    let (p, h, s) = roots();
    let executable = fake_host(p.path(), "2.1.265 (Claude Code)", "--plugin-dir");
    let mut plan = package_plan(p.path(), h.path(), &executable, "claude", "2.1.265");
    plan["spec"]["packages"][0]["digest"] = serde_json::json!(format!("sha256:{}", "0".repeat(64)));
    let plan = seal(plan);
    assert_eq!(
        graphhelm_host_adoption::apply(
            p.path(),
            h.path(),
            s.path(),
            &plan,
            plan["digest"].as_str().unwrap()
        )
        .unwrap_err()
        .reason
        .pointer(),
        "/adoption/plan_stale"
    );
    assert_eq!(
        std::fs::read(p.path().join(".claude/settings.local.json")).unwrap(),
        b"{}"
    );
}

#[test]
fn managed_force_enable_is_refused_and_preserved() {
    let (p, h, s) = roots();
    std::fs::create_dir(h.path().join(".claude")).unwrap();
    std::fs::write(
        h.path().join(".claude/managed-settings.json"),
        br#"{"enabledPlugins":{"old@local":true}}"#,
    )
    .unwrap();
    let before = br#"{"enabledPlugins":{"old@local":true}}"#;
    let after = r#"{"enabledPlugins":{"old@local":false}}"#;
    std::fs::write(p.path().join(".claude/settings.local.json"), before).unwrap();
    let plan = host_plan(p.path(), h.path(), before, after);
    assert!(
        graphhelm_host_adoption::apply(
            p.path(),
            h.path(),
            s.path(),
            &plan,
            plan["digest"].as_str().unwrap()
        )
        .is_err()
    );
    assert_eq!(
        std::fs::read(p.path().join(".claude/settings.local.json")).unwrap(),
        before
    );
}

#[test]
fn codex_config_changes_require_detected_supported_version_and_user_scope() {
    let (p, h, s) = roots();
    std::fs::create_dir(h.path().join(".codex")).unwrap();
    let before = "model = 'keep'\n[mcp_servers.keep]\ncommand = 'untouched'\n";
    let path = h
        .path()
        .join(".agents/skills/old/SKILL.md")
        .to_str()
        .unwrap()
        .replace('\\', "/");
    let after = graphhelm_host_adoption::hosts::codex::disable_skills(
        before,
        std::slice::from_ref(&path),
        true,
    )
    .unwrap();
    std::fs::write(h.path().join(".codex/config.toml"), before).unwrap();
    let executable = fake_host(p.path(), "codex-cli 0.114.0", "mcp features");
    let mut plan = host_plan(p.path(), h.path(), before.as_bytes(), &after);
    plan["spec"]["operations"][0] = serde_json::json!({"root":"home","path":".codex/config.toml","beforeDigest":hash(before.as_bytes()),"afterDigest":hash(after.as_bytes()),"after":after,"disableSkills":[path]});
    plan["spec"]["host"] = serde_json::json!({"name":"codex","version":"0.114.0","program":executable,"mode":"configuration"});
    let rejected = seal(plan.clone());
    assert!(
        graphhelm_host_adoption::apply(
            p.path(),
            h.path(),
            s.path(),
            &rejected,
            rejected["digest"].as_str().unwrap()
        )
        .is_err()
    );
    plan["spec"]["scopes"] = serde_json::json!(["user"]);
    let accepted = seal(plan);
    let result = graphhelm_host_adoption::apply(
        p.path(),
        h.path(),
        s.path(),
        &accepted,
        accepted["digest"].as_str().unwrap(),
    );
    if !cfg!(windows) {
        assert_eq!(
            result.unwrap_err().reason.pointer(),
            "/adoption/host_containment_unavailable"
        );
        assert_eq!(
            std::fs::read(h.path().join(".codex/config.toml")).unwrap(),
            before.as_bytes()
        );
        assert!(!h.path().join(".graphhelm-adoption.lock").exists());
        assert_eq!(std::fs::read_dir(s.path()).unwrap().count(), 0);
        return;
    }
    let result = result.unwrap();
    assert_eq!(result["spec"]["state"], "installed_unverified");
    let value: toml::Value =
        toml::from_str(&std::fs::read_to_string(h.path().join(".codex/config.toml")).unwrap())
            .unwrap();
    assert_eq!(
        value["skills"]["config"][0]["enabled"].as_bool(),
        Some(false)
    );
    assert_eq!(
        value["mcp_servers"]["keep"]["command"].as_str(),
        Some("untouched")
    );
}

fn copy_package(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let path = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_package(&entry.path(), &path);
        } else {
            std::fs::copy(entry.path(), path).unwrap();
        }
    }
}

#[test]
fn unlisted_host_hook_cannot_enter_a_pinned_package() {
    let (p, h, s) = roots();
    let input = tempfile::tempdir().unwrap();
    let executable = fake_host(p.path(), "2.1.265 (Claude Code)", "--plugin-dir");
    let plan = package_plan(p.path(), h.path(), &executable, "claude", "2.1.265");
    let sources = graphhelm_host_adoption::hosts::release_packages().unwrap();
    let paths: Vec<_> = sources
        .iter()
        .map(|source| {
            let to = input.path().join(&source.id);
            copy_package(&source.path, &to);
            to
        })
        .collect();
    std::fs::create_dir(paths[0].join("hooks")).unwrap();
    std::fs::write(
        paths[0].join("hooks/hooks.json"),
        br#"{"hooks":{"SessionStart":[{"command":"never-run"}]}}"#,
    )
    .unwrap();
    assert!(
        graphhelm_host_adoption::apply_with_packages(
            p.path(),
            h.path(),
            s.path(),
            &plan,
            plan["digest"].as_str().unwrap(),
            &paths
        )
        .is_err()
    );
    assert!(!p.path().join(".graphhelm-adoption.lock").exists());
}

#[test]
#[cfg(windows)]
fn second_package_drift_rolls_back_first_activation_and_leaves_config_intact() {
    let (p, h, s) = roots();
    let input = tempfile::tempdir().unwrap();
    let executable = fake_host(p.path(), "2.1.265 (Claude Code)", "--plugin-dir");
    let plan = package_plan(p.path(), h.path(), &executable, "claude", "2.1.265");
    let sources = graphhelm_host_adoption::hosts::release_packages().unwrap();
    let paths: Vec<_> = sources
        .iter()
        .map(|source| {
            let to = input.path().join(&source.id);
            copy_package(&source.path, &to);
            to
        })
        .collect();
    std::fs::write(
        executable.with_extension("input"),
        format!(
            "2.1.265 (Claude Code)\n--plugin-dir\n{}\n",
            paths[1].join("extension.json").display()
        ),
    )
    .unwrap();
    assert!(
        graphhelm_host_adoption::apply_with_packages(
            p.path(),
            h.path(),
            s.path(),
            &plan,
            plan["digest"].as_str().unwrap(),
            &paths
        )
        .is_err()
    );
    assert_eq!(
        std::fs::read(p.path().join(".claude/settings.local.json")).unwrap(),
        b"{}"
    );
    let transaction = hash(plan["digest"].as_str().unwrap().as_bytes());
    let activation = s.path().join(format!("{transaction}-graphhelm-jpd"));
    assert!(
        graphhelm_extension_host::active_versions(&activation)
            .unwrap()
            .is_none()
    );
    assert!(activation.join("inactive.json").is_file());
    let restored = graphhelm_host_adoption::recover(s.path(), &transaction).unwrap();
    assert_eq!(restored["spec"]["state"], "restored");
}

#[test]
fn unknown_hooks_are_inventoried_without_execution() {
    let (p, h, _s) = roots();
    std::fs::create_dir(h.path().join(".claude")).unwrap();
    let marker = p.path().join("hook-ran");
    std::fs::write(h.path().join(".claude/settings.json"),serde_json::to_vec(&serde_json::json!({"hooks":{"SessionStart":[{"hooks":[{"type":"command","command":format!("write {}",marker.display())}]}]}})).unwrap()).unwrap();
    let inventory = graphhelm_host_adoption::inventory(p.path(), h.path()).unwrap();
    let items = inventory["spec"]["hosts"][0]["items"].as_array().unwrap();
    let settings = items
        .iter()
        .find(|v| v["kind"] == ".claude/settings.json" && v["status"] != "absent")
        .unwrap();
    assert_eq!(settings["hookEffects"], "unknown_not_executed");
    assert!(!marker.exists());
}

#[test]
fn inventory_enumerates_bounded_skill_roots_and_configured_servers() {
    let (p, h, _s) = roots();
    std::fs::create_dir_all(p.path().join(".claude/skills/factory")).unwrap();
    std::fs::write(p.path().join(".claude/skills/factory/SKILL.md"), b"factory").unwrap();
    std::fs::create_dir_all(p.path().join(".claude")).unwrap();
    std::fs::write(
        p.path().join(".claude/settings.json"),
        br#"{"enabledPlugins":{"demo@local":false},"mcpServers":{"safe":{"command":"ignored"}}}"#,
    )
    .unwrap();
    let inventory = graphhelm_host_adoption::inventory(p.path(), h.path()).unwrap();
    let items = inventory["spec"]["hosts"][0]["items"].as_array().unwrap();
    assert!(
        items
            .iter()
            .any(|item| item["kind"] == "skill" && item["name"] == "factory")
    );
    assert!(
        items
            .iter()
            .any(|item| item["kind"] == "plugin" && item["enabled"] == false)
    );
    assert!(
        items
            .iter()
            .any(|item| item["kind"] == "mcp_server" && item["name"] == "safe")
    );
    assert_eq!(inventory["spec"]["hosts"][0]["coverage"], "incomplete");
    assert!(
        inventory["spec"]["hosts"][0]["coverageDetails"]
            .as_array()
            .unwrap()
            .iter()
            .any(|detail| detail["reason"] == "plugin_browser_unavailable")
    );
}

#[test]
fn inventory_records_shared_absence_and_managed_policy_without_claiming_complete() {
    let (p, h, _s) = roots();
    std::fs::create_dir_all(h.path().join(".claude")).unwrap();
    std::fs::write(h.path().join(".claude/managed-settings.json"), br#"{}"#).unwrap();
    std::fs::write(p.path().join("AGENTS.override.md"), b"override").unwrap();
    let inventory = graphhelm_host_adoption::inventory(p.path(), h.path()).unwrap();
    let hosts = inventory["spec"]["hosts"].as_array().unwrap();
    let claude = hosts.iter().find(|host| host["host"] == "claude").unwrap();
    let codex = hosts.iter().find(|host| host["host"] == "codex").unwrap();
    assert!(claude["items"].as_array().unwrap().iter().any(|item| {
        item["path"] == ".claude/managed-settings.json"
            && item["status"] == "parsed"
            && item["managed"] == true
    }));
    assert!(
        claude["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| { item["path"] == ".claude/settings.json" && item["status"] == "absent" })
    );
    assert!(
        codex["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| { item["path"] == "AGENTS.override.md" && item["status"] == "observed" })
    );
    assert_eq!(inventory["spec"]["coverage"], "incomplete");
}

#[test]
fn inventory_keeps_same_named_packages_distinct_and_mcp_disabled_state() {
    let (p, h, _s) = roots();
    std::fs::create_dir_all(p.path().join(".claude/skills/shared")).unwrap();
    std::fs::create_dir_all(h.path().join(".claude/skills/shared")).unwrap();
    std::fs::write(p.path().join(".claude/skills/shared/SKILL.md"), b"project").unwrap();
    std::fs::write(h.path().join(".claude/skills/shared/SKILL.md"), b"home").unwrap();
    std::fs::create_dir_all(h.path().join(".codex")).unwrap();
    std::fs::write(
        h.path().join(".codex/config.toml"),
        "[mcp_servers.local]\ncommand = 'ignored'\nenabled = false\n",
    )
    .unwrap();
    let inventory = graphhelm_host_adoption::inventory(p.path(), h.path()).unwrap();
    let items = inventory["spec"]["hosts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|host| host["host"] == "claude")
        .unwrap()["items"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|item| item["kind"] == "skill" && item["name"] == "shared")
        .cloned()
        .collect::<Vec<_>>();
    assert_eq!(items.len(), 2);
    assert_ne!(items[0]["id"], items[1]["id"]);
    let codex = inventory["spec"]["hosts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|host| host["host"] == "codex")
        .unwrap();
    assert!(codex["items"].as_array().unwrap().iter().any(|item| {
        item["kind"] == "mcp_server" && item["name"] == "local" && item["enabled"] == false
    }));
}

#[test]
fn preexisting_activation_is_never_claimed_or_retired_by_a_failed_apply() {
    let (p, h, s) = roots();
    let executable = fake_host(p.path(), "2.1.265 (Claude Code)", "--plugin-dir");
    let plan = package_plan(p.path(), h.path(), &executable, "claude", "2.1.265");
    let transaction = hash(plan["digest"].as_str().unwrap().as_bytes());
    let activation = s.path().join(format!("{transaction}-graphhelm-jpd"));
    std::fs::create_dir(&activation).unwrap();
    let claim = graphhelm_extension_host::ActivationClaim::acquire(&activation).unwrap();
    let package = &graphhelm_host_adoption::hosts::release_packages().unwrap()[0];
    let installed = graphhelm_extension_host::install_package(&claim, &package.path).unwrap();
    graphhelm_extension_host::switch_active(&claim, &installed.digest).unwrap();
    drop(claim);
    let before = std::fs::read(activation.join("active.json")).unwrap();
    assert!(
        graphhelm_host_adoption::apply(
            p.path(),
            h.path(),
            s.path(),
            &plan,
            plan["digest"].as_str().unwrap()
        )
        .is_err()
    );
    assert_eq!(
        std::fs::read(activation.join("active.json")).unwrap(),
        before
    );
}

#[test]
fn claude_capability_cannot_authorize_codex_configuration() {
    let (p, h, s) = roots();
    let executable = fake_host(p.path(), "2.1.265 (Claude Code)", "--plugin-dir");
    std::fs::create_dir(h.path().join(".codex")).unwrap();
    std::fs::write(h.path().join(".codex/config.toml"), "model='keep'\n").unwrap();
    let mut plan = package_plan(p.path(), h.path(), &executable, "claude", "2.1.265");
    plan["spec"]["scopes"] = serde_json::json!(["user"]);
    plan["spec"]["operations"][0] = serde_json::json!({"root":"home","path":".codex/config.toml","beforeDigest":hash(b"model='keep'\n"),"afterDigest":hash(b"model='keep'\n"),"after":"model='keep'\n","disableSkills":[]});
    let plan = seal(plan);
    assert!(
        graphhelm_host_adoption::apply(
            p.path(),
            h.path(),
            s.path(),
            &plan,
            plan["digest"].as_str().unwrap()
        )
        .is_err()
    );
    assert!(!h.path().join(".graphhelm-adoption.lock").exists());
}
