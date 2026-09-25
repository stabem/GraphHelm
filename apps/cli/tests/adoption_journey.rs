use serde_json::{Value, json};
use std::path::Path;
#[path = "../../../adapters/host-adoption/tests/support/activation_fixture.rs"]
mod fixture;

fn command(p: &Path, h: &Path, s: &Path) -> assert_cmd::Command {
    let mut command = assert_cmd::Command::new(assert_cmd::cargo::cargo_bin!("graphhelm"));
    command
        .arg("setup")
        .arg("--project")
        .arg(p)
        .arg("--home")
        .arg(h)
        .arg("--state-root")
        .arg(s);
    command
}
fn output(output: std::process::Output) -> Value {
    assert!(
        output.status.success(),
        "{} {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        output.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

/// #1208 after #1281: a plan that registers `mcpServers.graphhelm` in the project `.mcp.json`
/// previews the command and args this test put in that `after`, from the real binary in every
/// face a pipe can ask for, and never a sentinel placed elsewhere in the same file.
#[test]
fn plan_preview_shows_the_mcp_registration_it_would_write_and_nothing_else_of_the_file() {
    let p = tempfile::tempdir().unwrap();
    let h = tempfile::tempdir().unwrap();
    let s = tempfile::tempdir().unwrap();
    const SENTINEL: &str = "PRIVATE-MCP-SENTINEL-1208";
    let binary = p
        .path()
        .join(if cfg!(windows) {
            "graphhelm.exe"
        } else {
            "graphhelm"
        })
        .to_string_lossy()
        .into_owned();
    let args = json!([
        "mcp",
        "--url",
        "http://127.0.0.1:7433",
        "--token-file",
        "token",
        "--actor",
        "owner"
    ]);
    let after = serde_json::to_string(&json!({"mcpServers":{
        "graphhelm":{"command":binary,"args":args},
        "other":{"command":"other-server","env":{"API_KEY":SENTINEL}}}}))
    .unwrap();
    let mut plan =
        fixture::plan(graphhelm_host_adoption::root_bindings(p.path(), h.path()).unwrap());
    plan["spec"]["decisions"]
        .as_array_mut()
        .unwrap()
        .push(json!({"operationIndex":1,"decision":"replace","protected":false}));
    plan["spec"]["operations"].as_array_mut().unwrap().push(json!({"root":"project","path":".mcp.json",
        "beforeDigest":fixture::hash(b"{}"),"afterDigest":fixture::hash(after.as_bytes()),"after":after}));
    let plan = fixture::seal(plan);
    let planfile = s.path().join("mcp-plan.json");
    std::fs::write(&planfile, serde_json::to_vec(&plan).unwrap()).unwrap();
    for face in [None, Some("--json"), Some("--pretty")] {
        let mut run = command(p.path(), h.path(), s.path());
        run.arg("--plan").arg(&planfile);
        if let Some(flag) = face {
            run.arg(flag);
        }
        let raw = run.output().unwrap();
        for stream in [&raw.stdout, &raw.stderr] {
            let stream = String::from_utf8_lossy(stream);
            assert!(!stream.contains(SENTINEL), "{face:?}: {stream}");
            assert!(!stream.contains("other-server"), "{face:?}: {stream}");
        }
        let preview = output(raw);
        assert_eq!(
            preview["data"]["plan"]["spec"]["operations"][1]["registration"],
            json!({"command": binary, "args": args}),
            "{face:?}"
        );
    }
}

#[test]
fn pipe_requires_exact_acceptance_and_never_promotes_self_asserted_observation() {
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
    // #1208: the resolved `after` text is private. A sentinel planted in it (and the digest over
    // it re-computed here, not by the code under test) must reach the plan file and never stdout
    // or stderr of the `--plan` preview, in any face a pipe can ask for.
    const SENTINEL: &str = "PRIVATE-AFTER-SENTINEL-1208";
    let after = format!("GraphHelm JPD\nPrefer concise replies\nDeny secrets\n{SENTINEL}\n");
    let mut plan =
        fixture::plan(graphhelm_host_adoption::root_bindings(p.path(), h.path()).unwrap());
    plan["spec"]["operations"][0]["after"] = json!(after);
    plan["spec"]["operations"][0]["afterDigest"] = json!(fixture::hash(after.as_bytes()));
    let plan = fixture::seal(plan);
    let planfile = s.path().join("reviewed.json");
    std::fs::write(&planfile, serde_json::to_vec(&plan).unwrap()).unwrap();
    assert!(
        String::from_utf8(std::fs::read(&planfile).unwrap())
            .unwrap()
            .contains(SENTINEL),
        "control: the sentinel is in the private plan file"
    );
    let digest = plan["digest"].as_str().unwrap();
    for face in [None, Some("--json"), Some("--pretty")] {
        let mut run = command(p.path(), h.path(), s.path());
        run.arg("--plan").arg(&planfile);
        if let Some(flag) = face {
            run.arg(flag);
        }
        let raw = run.output().unwrap();
        for stream in [&raw.stdout, &raw.stderr] {
            let stream = String::from_utf8_lossy(stream);
            assert!(!stream.contains(SENTINEL), "{face:?}: {stream}");
            assert!(
                !stream.contains("Prefer concise replies"),
                "{face:?}: {stream}"
            );
        }
        let preview = output(raw);
        let view = &preview["data"]["plan"];
        assert_eq!(view["digest"], digest, "{face:?}");
        assert_eq!(view["spec"]["scopes"], json!(["project"]), "{face:?}");
        assert_eq!(view["spec"]["coverage"], "complete", "{face:?}");
        assert_eq!(view["spec"]["packages"], json!([]), "{face:?}");
        assert_eq!(
            view["spec"]["decisions"],
            json!([{"operationIndex":0,"decision":"replace","protected":false}]),
            "{face:?}"
        );
        assert_eq!(
            view["spec"]["operations"],
            json!([{"root":"project","path":"AGENTS.md",
                "beforeDigest":fixture::hash(b"factory\nPrefer concise replies\nDeny secrets\n"),
                "afterDigest":fixture::hash(after.as_bytes()),
                "afterBytes":after.len()}]),
            "{face:?}"
        );
    }
    let mut changed = plan.clone();
    changed["id"] = json!("unreviewed");
    std::fs::write(&planfile, serde_json::to_vec(&changed).unwrap()).unwrap();
    assert!(
        !command(p.path(), h.path(), s.path())
            .arg("--plan")
            .arg(&planfile)
            .output()
            .unwrap()
            .status
            .success()
    );
    std::fs::write(&planfile, serde_json::to_vec(&plan).unwrap()).unwrap();
    assert!(
        !command(p.path(), h.path(), s.path())
            .arg("--apply")
            .arg(&planfile)
            .output()
            .unwrap()
            .status
            .success()
    );
    let applied = output(
        command(p.path(), h.path(), s.path())
            .arg("--apply")
            .arg(&planfile)
            .arg("--accept")
            .arg(plan["digest"].as_str().unwrap())
            .output()
            .unwrap(),
    );
    let baseline = std::fs::read(s.path().join("original.json")).unwrap();
    let receipt = fixture::receipt(
        &plan,
        applied["data"]["receipt"]["installedAtUnixMs"]
            .as_u64()
            .unwrap(),
    );
    let receiptfile = s.path().join("observation.json");
    std::fs::write(&receiptfile, serde_json::to_vec(&receipt).unwrap()).unwrap();
    let verified = output(
        command(p.path(), h.path(), s.path())
            .arg("--plan")
            .arg(&planfile)
            .arg("--verify")
            .arg(&receiptfile)
            .output()
            .unwrap(),
    );
    assert_eq!(
        verified["data"]["receipt"]["spec"]["state"],
        "installed_unverified"
    );
    assert_eq!(
        verified["data"]["receipt"]["verification"]["status"],
        "observer_missing"
    );
    for attack in ["digest", "mcp", "method"] {
        let mut bad = receipt.clone();
        match attack {
            "digest" => bad["digest"] = json!("sha256:bad"),
            "mcp" => {
                bad["spec"].as_object_mut().unwrap().remove("mcp");
                bad = fixture::seal(bad);
            }
            _ => {
                bad["spec"]["methodology"]["oldMethodInactive"] = json!(false);
                bad = fixture::seal(bad);
            }
        }
        std::fs::write(&receiptfile, serde_json::to_vec(&bad).unwrap()).unwrap();
        let refused = command(p.path(), h.path(), s.path())
            .arg("--plan")
            .arg(&planfile)
            .arg("--verify")
            .arg(&receiptfile)
            .output()
            .unwrap();
        assert!(!refused.status.success(), "{attack}");
        let refused: Value = serde_json::from_slice(&refused.stdout).unwrap();
        assert_eq!(
            refused["diagnostics"][0]["path"],
            "/adoption/activation_invalid"
        );
    }
    let again = output(
        command(p.path(), h.path(), s.path())
            .arg("--apply")
            .arg(&planfile)
            .arg("--accept")
            .arg(plan["digest"].as_str().unwrap())
            .output()
            .unwrap(),
    );
    assert_eq!(
        again["data"]["receipt"]["spec"]["state"],
        "installed_unverified"
    );
    assert_eq!(
        std::fs::read(s.path().join("original.json")).unwrap(),
        baseline
    );
}

#[test]
#[cfg(windows)]
fn offline_cli_journey_keeps_compatible_skills_and_restores_with_later_user_key() {
    let p = tempfile::tempdir().unwrap();
    let h = tempfile::tempdir().unwrap();
    let s = tempfile::tempdir().unwrap();
    let original = b"factory\nPrefer concise replies\nDeny secrets\n";
    std::fs::write(p.path().join("AGENTS.md"), original).unwrap();
    let mut skill_paths = Vec::new();
    for name in ["compatible-a", "compatible-b", "factory"] {
        let dir = h.path().join(name);
        std::fs::create_dir(&dir).unwrap();
        std::fs::write(dir.join("SKILL.md"), format!("# {name}\nFixture only\n")).unwrap();
        skill_paths.push(dir.join("SKILL.md").to_string_lossy().replace('\\', "/"));
    }
    let before = toml::to_string(&json!({"personal":"concise","permissions":{"deny":["secrets"]},"skills":{"config":skill_paths.iter().map(|path|json!({"path":path,"enabled":true})).collect::<Vec<_>>()}})).unwrap();
    let after = graphhelm_host_adoption::hosts::codex::disable_skills(
        &before,
        &[skill_paths[2].clone()],
        true,
    )
    .unwrap();
    std::fs::create_dir(h.path().join(".codex")).unwrap();
    let configfile = h.path().join(".codex/config.toml");
    std::fs::write(&configfile, &before).unwrap();
    let executable = s.path().join("inert-host.exe");
    assert!(
        std::process::Command::new("rustc")
            .args(["--edition=2024"])
            .arg(
                Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("../../adapters/host-adoption/tests/fixtures/inert-host.rs")
            )
            .arg("-o")
            .arg(&executable)
            .status()
            .unwrap()
            .success()
    );
    std::fs::write(
        executable.with_extension("input"),
        "codex-cli 0.114.0\nplugins browser\n",
    )
    .unwrap();
    let mut plan =
        fixture::plan(graphhelm_host_adoption::root_bindings(p.path(), h.path()).unwrap());
    plan["spec"]["host"] =
        json!({"name":"codex","version":"0.114.0","program":executable,"mode":"configuration"});
    plan["spec"]["scopes"] = json!(["project", "user"]);
    plan["spec"]["decisions"]
        .as_array_mut()
        .unwrap()
        .push(json!({"operationIndex":1,"decision":"replace","protected":false}));
    plan["spec"]["operations"].as_array_mut().unwrap().push(json!({"root":"home","path":".codex/config.toml","beforeDigest":fixture::hash(before.as_bytes()),"afterDigest":fixture::hash(after.as_bytes()),"after":after,"disableSkills":[skill_paths[2]]}));
    let plan = fixture::seal(plan);
    let planfile = s.path().join("plan.json");
    std::fs::write(&planfile, serde_json::to_vec(&plan).unwrap()).unwrap();
    // #1208: the preview names the program `--apply` runs, and it is the `executable` this test
    // compiled and wrote into the plan above, in every face a pipe can ask for.
    let program = executable.to_str().unwrap();
    for face in [None, Some("--json"), Some("--pretty")] {
        let mut run = command(p.path(), h.path(), s.path());
        run.arg("--plan").arg(&planfile);
        if let Some(flag) = face {
            run.arg(flag);
        }
        let preview = output(run.output().unwrap());
        assert_eq!(
            preview["data"]["plan"]["spec"]["host"]["program"], program,
            "{face:?}"
        );
        assert_eq!(
            preview["data"]["plan"]["spec"]["operations"][1]["disableSkills"],
            json!([skill_paths[2]]),
            "{face:?}"
        );
        assert!(
            !preview["data"]["plan"]["spec"]["operations"][1]
                .as_object()
                .unwrap()
                .contains_key("after"),
            "{face:?}"
        );
    }
    let apply = || {
        output(
            command(p.path(), h.path(), s.path())
                .arg("--apply")
                .arg(&planfile)
                .arg("--accept")
                .arg(plan["digest"].as_str().unwrap())
                .output()
                .unwrap(),
        )
    };
    let installed = apply();
    let baseline = std::fs::read(s.path().join("original.json")).unwrap();
    let receipt = fixture::receipt(
        &plan,
        installed["data"]["receipt"]["installedAtUnixMs"]
            .as_u64()
            .unwrap(),
    );
    let receiptfile = s.path().join("activation.json");
    std::fs::write(&receiptfile, serde_json::to_vec(&receipt).unwrap()).unwrap();
    let checked = output(
        command(p.path(), h.path(), s.path())
            .arg("--plan")
            .arg(&planfile)
            .arg("--verify")
            .arg(&receiptfile)
            .output()
            .unwrap(),
    );
    assert_eq!(
        checked["data"]["receipt"]["verification"]["status"],
        "observer_missing"
    );
    apply();
    assert_eq!(
        std::fs::read(s.path().join("original.json")).unwrap(),
        baseline
    );
    let installed: toml::Value =
        toml::from_str(&std::fs::read_to_string(&configfile).unwrap()).unwrap();
    for i in 0..3 {
        assert_eq!(
            installed["skills"]["config"][i]["enabled"].as_bool(),
            Some(i < 2)
        );
    }
    let mut edited = installed;
    edited
        .as_table_mut()
        .unwrap()
        .insert("user_added".into(), toml::Value::String("keep me".into()));
    std::fs::write(&configfile, toml::to_string(&edited).unwrap()).unwrap();
    let restore = output(
        assert_cmd::Command::new(assert_cmd::cargo::cargo_bin!("graphhelm"))
            .arg("restore")
            .arg("--state-root")
            .arg(s.path())
            .output()
            .unwrap(),
    );
    let restoreplan = &restore["data"]["plan"];
    let restorefile = s.path().join("restore.json");
    std::fs::write(&restorefile, serde_json::to_vec(restoreplan).unwrap()).unwrap();
    let restored = output(
        assert_cmd::Command::new(assert_cmd::cargo::cargo_bin!("graphhelm"))
            .arg("restore")
            .arg("--state-root")
            .arg(s.path())
            .arg("--apply")
            .arg(&restorefile)
            .arg("--accept")
            .arg(restoreplan["digest"].as_str().unwrap())
            .output()
            .unwrap(),
    );
    assert_eq!(restored["data"]["receipt"]["spec"]["state"], "restored");
    let restored: toml::Value =
        toml::from_str(&std::fs::read_to_string(&configfile).unwrap()).unwrap();
    let mut expected: toml::Value = toml::from_str(&before).unwrap();
    expected
        .as_table_mut()
        .unwrap()
        .insert("user_added".into(), toml::Value::String("keep me".into()));
    assert_eq!(restored, expected);
    assert_eq!(std::fs::read(p.path().join("AGENTS.md")).unwrap(), original);
    for (i, name) in ["compatible-a", "compatible-b", "factory"]
        .iter()
        .enumerate()
    {
        assert_eq!(
            std::fs::read_to_string(&skill_paths[i]).unwrap(),
            format!("# {name}\nFixture only\n")
        );
    }
}
