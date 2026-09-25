use std::path::Path;

use serde_json::Value;

fn graphhelm() -> assert_cmd::Command {
    assert_cmd::Command::new(assert_cmd::cargo::cargo_bin!("graphhelm"))
}

fn output_json(output: std::process::Output) -> Value {
    assert!(
        output.status.success(),
        "setup failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

fn setup(project: &Path, home: &Path) -> Value {
    output_json(
        graphhelm()
            .args(["setup", "--dry-run", "--project"])
            .arg(project)
            .args(["--home"])
            .arg(home)
            .output()
            .unwrap(),
    )
}

fn setup_default(project: &Path, home: &Path) -> std::process::Output {
    graphhelm()
        .args(["setup", "--project"])
        .arg(project)
        .args(["--home"])
        .arg(home)
        .output()
        .unwrap()
}

#[test]
fn setup_defaults_to_safe_preview_without_dry_run() {
    let project = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();

    let output = setup_default(project.path(), home.path());

    assert!(
        output.status.success(),
        "setup failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["data"]["plan"]["kind"], "AdoptionPlan");
}

#[test]
fn setup_rejects_a_missing_explicit_root() {
    let project = tempfile::tempdir().unwrap();
    let home = project.path().join("missing-home");

    let output = setup_default(project.path(), &home);

    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("GHCLI029_ADOPTION_REFUSED"));
}

/// Since #1210 a link on a host surface no longer aborts the whole inventory: it is recorded as a
/// coverage gap and never followed. A followed broken link would read as `absent`; the surface
/// must instead be `inaccessible`, with no digest, and the gap named by its surface id.
#[cfg(unix)]
#[test]
fn setup_records_a_broken_symlink_surface_as_a_coverage_gap() {
    use std::os::unix::fs::symlink;

    let project = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    symlink(
        home.path().join("does-not-exist"),
        home.path().join(".claude.json"),
    )
    .unwrap();

    let value = output_json(setup_default(project.path(), home.path()));

    let claude = claude_host(&value);
    let surface = claude_items(&value)
        .into_iter()
        .find(|item| item["id"] == "home/.claude.json")
        .expect("the linked surface is still reported");
    assert_eq!(surface["status"], "inaccessible", "{surface}");
    assert_eq!(surface["digest"], Value::Null, "{surface}");
    assert_eq!(claude["coverage"], "inaccessible");
    assert!(
        claude["coverageDetails"]
            .as_array()
            .unwrap()
            .iter()
            .any(
                |detail| detail["scope"] == "home/.claude.json" && detail["reason"] == "PathUnsafe"
            ),
        "{}",
        claude["coverageDetails"]
    );
    assert_eq!(value["data"]["inventory"]["spec"]["coverage"], "incomplete");
}

#[cfg(unix)]
#[test]
fn setup_rejects_a_symlink_in_the_root_ancestry() {
    use std::os::unix::fs::symlink;

    let outside = tempfile::tempdir().unwrap();
    let holder = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    let link = holder.path().join("linked");
    symlink(outside.path(), &link).unwrap();

    let output = setup_default(&link, home.path());

    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("GHCLI029_ADOPTION_REFUSED"));
}

#[test]
fn preview_keeps_existing_settings_byte_identical() {
    let project = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    std::fs::create_dir(home.path().join(".claude")).unwrap();
    let path = home.path().join(".claude/settings.json");
    let original = br#"{"permissions":{"deny":["Read(.env)"]},"unknown":7}"#;
    std::fs::write(&path, original).unwrap();

    let value = setup(project.path(), home.path());

    assert_eq!(value["data"]["inventory"]["kind"], "Inventory");
    assert_eq!(std::fs::read(path).unwrap(), original);
}

#[test]
fn preview_reports_each_supported_host_in_stable_order() {
    let project = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    std::fs::create_dir(home.path().join(".claude")).unwrap();
    std::fs::create_dir(home.path().join(".codex")).unwrap();
    std::fs::write(home.path().join(".claude/settings.json"), b"{}").unwrap();
    std::fs::write(home.path().join(".codex/config.toml"), b"").unwrap();

    let value = setup(project.path(), home.path());
    let hosts = value["data"]["inventory"]["spec"]["hosts"]
        .as_array()
        .unwrap();

    assert_eq!(hosts[0]["host"], "claude");
    assert_eq!(hosts[1]["host"], "codex");
}

#[test]
fn preview_inventories_project_local_claude_settings_without_reading_home_contents() {
    let project = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    std::fs::create_dir(project.path().join(".claude")).unwrap();
    std::fs::write(
        project.path().join(".claude/settings.local.json"),
        br#"{"permissions":{"deny":["Read(.env)"]}}"#,
    )
    .unwrap();

    let value = setup(project.path(), home.path());
    let items = value["data"]["inventory"]["spec"]["hosts"][0]["items"]
        .as_array()
        .unwrap();

    assert!(
        items
            .iter()
            .any(|item| item["kind"] == ".claude/settings.local.json")
    );
}

#[test]
fn preview_leaves_unclassified_methodology_unresolved() {
    let project = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    std::fs::write(
        project.path().join("AGENTS.md"),
        b"Use a custom software-factory methodology.",
    )
    .unwrap();

    let value = setup(project.path(), home.path());

    assert_eq!(value["data"]["plan"]["kind"], "AdoptionPlan");
    assert_eq!(
        value["data"]["plan"]["spec"]["decisions"][0]["decision"],
        "unresolved"
    );
}

fn link_directory(target: &Path, link: &Path) {
    #[cfg(unix)]
    std::os::unix::fs::symlink(target, link).unwrap();
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let status = std::process::Command::new("cmd")
            .raw_arg(format!(
                "/C mklink /J \"{}\" \"{}\"",
                link.display(),
                target.display()
            ))
            .stdout(std::process::Stdio::null())
            .status()
            .unwrap();
        assert!(status.success(), "mklink /J failed");
    }
}

fn claude_host(value: &Value) -> Value {
    value["data"]["inventory"]["spec"]["hosts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|host| host["host"] == "claude")
        .unwrap()
        .clone()
}

fn claude_items(value: &Value) -> Vec<Value> {
    claude_host(value)["items"].as_array().unwrap().clone()
}

#[test]
fn preview_records_a_linked_skill_and_keeps_scanning_its_siblings() {
    let project = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let skills = home.path().join(".claude/skills");
    for name in ["aaa-real", "zzz-real"] {
        std::fs::create_dir_all(skills.join(name)).unwrap();
        std::fs::write(skills.join(name).join("SKILL.md"), name).unwrap();
    }
    std::fs::create_dir_all(outside.path().join("linked")).unwrap();
    std::fs::write(outside.path().join("linked/SKILL.md"), b"linked").unwrap();
    link_directory(&outside.path().join("linked"), &skills.join("mmm-link"));

    let value = setup(project.path(), home.path());
    let items = claude_items(&value);
    let skills = items
        .iter()
        .filter(|item| item["kind"] == "skill")
        .map(|item| {
            (
                item["name"].as_str().unwrap().to_owned(),
                item["status"].clone(),
            )
        })
        .collect::<Vec<_>>();

    assert!(
        skills.contains(&("aaa-real".into(), serde_json::json!("observed"))),
        "{skills:?}"
    );
    assert!(
        skills.contains(&("zzz-real".into(), serde_json::json!("observed"))),
        "{skills:?}"
    );
    assert!(
        skills.contains(&("mmm-link".into(), serde_json::json!("linked"))),
        "{skills:?}"
    );
    let claude = claude_host(&value);
    assert_eq!(claude["coverage"], "incomplete");
    assert!(
        claude["coverageDetails"]
            .as_array()
            .unwrap()
            .iter()
            .any(|detail| detail["scope"] == "claude" && detail["reason"] == "linked")
    );
    assert!(
        !items.iter().any(|item| item["path"]
            .as_str()
            .is_some_and(|path| path.contains("mmm-link/SKILL.md"))),
        "the link was followed"
    );
}

#[test]
fn preview_reads_the_installed_plugin_record_instead_of_walking_the_cache() {
    let project = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    let plugins = home.path().join(".claude/plugins");
    let cached = plugins.join("cache/market/demo/abc123");
    std::fs::create_dir_all(cached.join(".claude-plugin")).unwrap();
    std::fs::create_dir_all(cached.join(".cursor/skills/demo")).unwrap();
    std::fs::write(
        cached.join(".claude-plugin/plugin.json"),
        br#"{"name":"demo"}"#,
    )
    .unwrap();
    std::fs::write(cached.join(".cursor/skills/demo/SKILL.md"), b"vendored").unwrap();
    std::fs::write(
        plugins.join("installed_plugins.json"),
        br#"{"version":2,"plugins":{"demo@market":[{"scope":"user","installPath":"x","version":"abc123"}]}}"#,
    )
    .unwrap();

    let value = setup(project.path(), home.path());
    let plugin_items = claude_items(&value)
        .into_iter()
        .filter(|item| item["kind"] == "plugin")
        .collect::<Vec<_>>();

    assert_eq!(plugin_items.len(), 1, "{plugin_items:?}");
    assert_eq!(plugin_items[0]["name"], "demo@market");
    assert_eq!(plugin_items[0]["origin"], "installed");
    assert_eq!(plugin_items[0]["status"], "recorded");
    assert_eq!(plugin_items[0]["version"], "abc123");
    assert_eq!(plugin_items[0]["installed"], true);
}

/// Review of #1210 ([757c], confirmed by [3f90d6] and [b9deb2]): a record small on disk must not
/// expand without bound. One name with more installs than the entry budget, and one name longer
/// than a host writes, are both cut, and the cut is RECORDED as a coverage gap, never silent.
#[test]
fn preview_bounds_the_plugin_record_and_records_the_cut() {
    let project = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    let plugins = home.path().join(".claude/plugins");
    std::fs::create_dir_all(&plugins).unwrap();
    let installs = vec!["{}"; 10_001].join(",");
    let long_name = "x".repeat(600);
    let record = format!(
        r#"{{"version":2,"plugins":{{"{long_name}@m":[{{}}],"many@market":[{installs}]}}}}"#
    );
    std::fs::write(plugins.join("installed_plugins.json"), record).unwrap();

    let value = setup(project.path(), home.path());
    let plugin_items = claude_items(&value)
        .into_iter()
        .filter(|item| item["kind"] == "plugin")
        .collect::<Vec<_>>();

    assert_eq!(
        plugin_items.len(),
        10_000,
        "the entry budget spans installs, not names"
    );
    assert!(
        plugin_items
            .iter()
            .all(|item| item["name"] == "many@market"),
        "the over-long name is not emitted"
    );
    let host = claude_host(&value);
    assert_eq!(host["coverage"], "incomplete");
    assert!(
        host["coverageDetails"]
            .as_array()
            .unwrap()
            .iter()
            .any(|detail| detail["scope"] == "plugin-record" && detail["reason"] == "truncated"),
        "the cut is recorded: {}",
        host["coverageDetails"]
    );
}

/// Review of #1210 by [5bdc38] (LOW): the cell above cannot pin the name cap, because keys sort
/// and the entry budget fires before the long name is ever read. This one has no budget pressure:
/// only the 512-byte cap can keep the long name out, and the cut must still be recorded.
#[test]
fn preview_refuses_an_over_long_plugin_name_and_records_the_cut() {
    let project = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    let plugins = home.path().join(".claude/plugins");
    std::fs::create_dir_all(&plugins).unwrap();
    let long_name = "a".repeat(600);
    let record = format!(
        r#"{{"version":2,"plugins":{{"{long_name}@m":[{{"scope":"user"}}],"ok@market":[{{"scope":"user"}}]}}}}"#
    );
    std::fs::write(plugins.join("installed_plugins.json"), record).unwrap();

    let value = setup(project.path(), home.path());
    let names = claude_items(&value)
        .into_iter()
        .filter(|item| item["kind"] == "plugin")
        .map(|item| item["name"].as_str().unwrap_or_default().to_owned())
        .collect::<Vec<_>>();

    assert_eq!(
        names,
        vec!["ok@market".to_owned()],
        "only the valid name is emitted"
    );
    let host = claude_host(&value);
    assert!(
        host["coverageDetails"]
            .as_array()
            .unwrap()
            .iter()
            .any(|detail| detail["scope"] == "plugin-record" && detail["reason"] == "truncated"),
        "the cut is recorded: {}",
        host["coverageDetails"]
    );
}

#[test]
fn preview_inventories_user_instructions_rules_agents_and_commands() {
    let project = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    for (root, relative, bytes) in [
        (home.path(), ".claude/CLAUDE.md", "user memory"),
        (home.path(), ".claude/rules/style.md", "user rule"),
        (project.path(), ".claude/CLAUDE.md", "project memory"),
        (project.path(), "CLAUDE.local.md", "local memory"),
        (
            project.path(),
            ".claude/rules/nested/api.md",
            "project rule",
        ),
        (project.path(), ".claude/agents/reviewer.md", "agent"),
        (home.path(), ".claude/commands/deploy.md", "command"),
    ] {
        let path = root.join(relative);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, bytes).unwrap();
    }
    std::fs::write(home.path().join("CLAUDE.md"), b"not a claude code location").unwrap();

    let value = setup(project.path(), home.path());
    let items = claude_items(&value);
    let status_of = |id: &str| {
        items
            .iter()
            .find(|item| item["id"] == id)
            .unwrap_or_else(|| panic!("{id} missing"))["status"]
            .clone()
    };

    assert_eq!(status_of("home/.claude/CLAUDE.md"), "observed");
    assert_eq!(status_of("project/.claude/CLAUDE.md"), "observed");
    assert_eq!(status_of("project/CLAUDE.local.md"), "observed");
    assert_eq!(
        status_of("home/rule/.claude/rules/style.md/style"),
        "observed"
    );
    assert_eq!(
        status_of("project/rule/.claude/rules/nested/api.md/api"),
        "observed"
    );
    assert_eq!(
        status_of("project/agent/.claude/agents/reviewer.md/reviewer"),
        "observed"
    );
    assert_eq!(
        status_of("home/command/.claude/commands/deploy.md/deploy"),
        "observed"
    );
    let rule = items
        .iter()
        .find(|item| item["id"] == "home/rule/.claude/rules/style.md/style")
        .unwrap();
    assert_eq!(rule["protected"], true);
    assert!(!items.iter().any(|item| item["id"] == "home/CLAUDE.md"));
}

#[test]
fn preview_names_the_platform_managed_settings_location() {
    let project = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();

    let value = setup(project.path(), home.path());
    let items = claude_items(&value);
    let managed = items
        .iter()
        .find(|item| item["id"] == "managed/managed-settings.json")
        .unwrap();

    assert_eq!(managed["managed"], true);
    assert_eq!(managed["root"], "managed");
    assert_eq!(managed["origin"], "platform");
    let path = managed["path"].as_str().unwrap();
    if cfg!(windows) {
        assert!(
            path.ends_with("/ClaudeCode/managed-settings.json"),
            "{path}"
        );
        assert!(!path.contains("ProgramData"), "{path}");
    } else if cfg!(target_os = "macos") {
        assert_eq!(
            path,
            "/Library/Application Support/ClaudeCode/managed-settings.json"
        );
    } else {
        assert_eq!(path, "/etc/claude-code/managed-settings.json");
    }
    assert!(
        ["absent", "parsed", "invalid", "inaccessible", "truncated"]
            .contains(&managed["status"].as_str().unwrap()),
        "{managed}"
    );
    assert!(
        claude_host(&value)["coverageDetails"]
            .as_array()
            .unwrap()
            .iter()
            .any(|detail| detail["scope"] == "managed-policy" && detail["state"] == "incomplete")
    );
}
