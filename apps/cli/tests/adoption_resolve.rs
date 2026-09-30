//! From a preview to an applied plan without hand-written JSON (#1208 slice 2).
use serde_json::Value;
use std::path::Path;

fn setup(p: &Path, h: &Path) -> assert_cmd::Command {
    let mut command = assert_cmd::Command::new(assert_cmd::cargo::cargo_bin!("graphhelm"));
    command
        .arg("setup")
        .arg("--project")
        .arg(p)
        .arg("--home")
        .arg(h);
    command
}

fn json(output: std::process::Output) -> (bool, Value) {
    let value: Value = serde_json::from_slice(&output.stdout).unwrap_or_else(|_| {
        panic!(
            "not json: {} {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    });
    (output.status.success(), value)
}

fn private_dir() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    dir
}

const ORIGINAL: &[u8] = b"Use the old factory pipeline.\nPrefer concise replies\nDeny secrets\n";
const REVIEWED: &[u8] = b"GraphHelm JPD is the pipeline.\nPrefer concise replies\nDeny secrets\n";

fn seeded() -> (tempfile::TempDir, tempfile::TempDir) {
    let p = tempfile::tempdir().unwrap();
    let h = tempfile::tempdir().unwrap();
    std::fs::write(p.path().join("AGENTS.md"), ORIGINAL).unwrap();
    std::fs::create_dir_all(h.path().join(".claude/skills/compatible")).unwrap();
    std::fs::write(
        h.path().join(".claude/skills/compatible/SKILL.md"),
        b"a compatible skill",
    )
    .unwrap();
    std::fs::create_dir_all(h.path().join(".claude")).unwrap();
    std::fs::write(
        h.path().join(".claude/settings.json"),
        br#"{"permissions":{"deny":["Read(.env)"]}}"#,
    )
    .unwrap();
    (p, h)
}

#[test]
fn preview_decides_only_what_exists_keeps_tools_and_leaves_instructions_to_the_owner() {
    let (p, h) = seeded();
    let (ok, value) = json(setup(p.path(), h.path()).arg("--dry-run").output().unwrap());
    assert!(ok, "{value}");
    let decisions = value["data"]["plan"]["spec"]["decisions"]
        .as_array()
        .unwrap();
    let find = |id: &str| {
        decisions
            .iter()
            .find(|d| d["item"] == id)
            .unwrap_or_else(|| panic!("{id} missing from {decisions:?}"))
    };
    assert_eq!(find("project/AGENTS.md")["decision"], "unresolved");
    assert_eq!(find("project/AGENTS.md")["operable"], true);
    assert_eq!(
        find("home/skill/.claude/skills/compatible/SKILL.md/compatible")["decision"],
        "keep"
    );
    assert_eq!(find("home/.claude/settings.json")["decision"], "keep");
    assert!(
        !decisions.iter().any(|d| d["item"] == "project/CLAUDE.md"),
        "an absent surface got a decision: {decisions:?}"
    );
    let summary = &value["data"]["plan"]["spec"]["summary"];
    assert_eq!(summary["unresolved"], 1);
    assert_eq!(summary["operableUnresolved"], 1);
    assert_eq!(value["data"]["plan"]["spec"]["applyAllowed"], false);
}

#[test]
fn a_resolved_plan_round_trips_from_out_through_plan_to_apply() {
    let (p, h) = seeded();
    let s = private_dir();
    let reviewed = s.path().join("AGENTS.reviewed.md");
    std::fs::write(&reviewed, REVIEWED).unwrap();
    let out = s.path().join("plan.json");

    // --resolve without --out is refused before anything is read or written.
    let (ok, refused) = json(
        setup(p.path(), h.path())
            .arg("--resolve")
            .arg(format!("project/AGENTS.md=replace:{}", reviewed.display()))
            .output()
            .unwrap(),
    );
    assert!(!ok);
    assert_eq!(
        refused["diagnostics"][0]["path"],
        "/adoption/invalid_configuration"
    );
    assert!(!out.exists());

    let (ok, value) = json(
        setup(p.path(), h.path())
            .arg("--resolve")
            .arg(format!("project/AGENTS.md=replace:{}", reviewed.display()))
            .arg("--out")
            .arg(&out)
            .output()
            .unwrap(),
    );
    assert!(ok, "{value}");
    let digest = value["data"]["acceptance"]["digest"]
        .as_str()
        .unwrap()
        .to_owned();
    assert!(digest.starts_with("sha256:"));
    // The public envelope never carries the after-bytes; the private file does.
    let public = serde_json::to_string(&value).unwrap();
    assert!(
        !public.contains("GraphHelm JPD is the pipeline"),
        "{public}"
    );
    assert_eq!(
        value["data"]["plan"]["spec"]["operations"][0]["after"],
        "<redacted: in the private plan file>"
    );
    let private: Value = serde_json::from_slice(&std::fs::read(&out).unwrap()).unwrap();
    assert_eq!(private["digest"], digest);
    assert_eq!(
        private["spec"]["operations"][0]["after"],
        std::str::from_utf8(REVIEWED).unwrap()
    );
    assert_eq!(private["spec"]["decisions"][0]["decision"], "replace");
    assert_eq!(private["spec"]["coverage"], "incomplete");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&out).unwrap().permissions().mode() & 0o077,
            0
        );
    }

    // The written file is exactly what --plan accepts and what --apply consumes.
    let (ok, preview) = json(
        setup(p.path(), h.path())
            .arg("--plan")
            .arg(&out)
            .output()
            .unwrap(),
    );
    assert!(ok, "{preview}");
    assert_eq!(preview["data"]["plan"]["digest"], digest);

    let (ok, applied) = json(
        setup(p.path(), h.path())
            .arg("--state-root")
            .arg(s.path())
            .arg("--apply")
            .arg(&out)
            .arg("--accept")
            .arg(&digest)
            .output()
            .unwrap(),
    );
    assert!(ok, "{applied}");
    assert_eq!(
        applied["data"]["receipt"]["spec"]["state"],
        "installed_unverified"
    );
    assert_eq!(std::fs::read(p.path().join("AGENTS.md")).unwrap(), REVIEWED);
    assert_eq!(
        std::fs::read(h.path().join(".claude/skills/compatible/SKILL.md")).unwrap(),
        b"a compatible skill"
    );
    assert!(s.path().join("original.json").exists());
}

#[test]
fn unresolved_items_block_the_plan_and_keep_alone_has_nothing_to_apply() {
    let (p, h) = seeded();
    std::fs::write(p.path().join("CLAUDE.md"), b"second instruction file\n").unwrap();
    let s = private_dir();
    let reviewed = s.path().join("AGENTS.reviewed.md");
    std::fs::write(&reviewed, REVIEWED).unwrap();
    let out = s.path().join("plan.json");

    // One of two instruction files answered: review still required, nothing written.
    let (ok, refused) = json(
        setup(p.path(), h.path())
            .arg("--resolve")
            .arg(format!("project/AGENTS.md=replace:{}", reviewed.display()))
            .arg("--out")
            .arg(&out)
            .output()
            .unwrap(),
    );
    assert!(!ok);
    assert_eq!(
        refused["diagnostics"][0]["path"],
        "/adoption/review_required"
    );
    assert!(!out.exists());

    // Both kept: honest, but `resolve` adds no packages, so the plan would change nothing and
    // install nothing — refused, not padded.
    let (ok, refused) = json(
        setup(p.path(), h.path())
            .arg("--resolve")
            .arg("project/AGENTS.md=keep")
            .arg("--resolve")
            .arg("project/CLAUDE.md=keep")
            .arg("--out")
            .arg(&out)
            .output()
            .unwrap(),
    );
    assert!(!ok);
    assert_eq!(
        refused["diagnostics"][0]["path"],
        "/adoption/invalid_configuration"
    );
    assert!(!out.exists());

    // An unknown item, a second answer for the same item, and a replace that changes nothing.
    for resolves in [
        vec!["project/nowhere.md=keep".to_owned()],
        vec![
            "project/AGENTS.md=keep".to_owned(),
            "project/AGENTS.md=keep".to_owned(),
        ],
        vec![format!(
            "project/AGENTS.md=replace:{}",
            p.path().join("AGENTS.md").display()
        )],
    ] {
        let mut command = setup(p.path(), h.path());
        for resolve in &resolves {
            command.arg("--resolve").arg(resolve);
        }
        let (ok, refused) = json(command.arg("--out").arg(&out).output().unwrap());
        assert!(!ok, "{resolves:?}");
        assert_eq!(
            refused["diagnostics"][0]["path"], "/adoption/review_required",
            "{resolves:?}"
        );
    }
    assert!(!out.exists());
    assert_eq!(std::fs::read(p.path().join("AGENTS.md")).unwrap(), ORIGINAL);
}

#[test]
fn a_reviewed_plan_goes_stale_when_the_source_moves_after_out() {
    let (p, h) = seeded();
    let s = private_dir();
    let reviewed = s.path().join("AGENTS.reviewed.md");
    std::fs::write(&reviewed, REVIEWED).unwrap();
    let out = s.path().join("plan.json");
    let (ok, value) = json(
        setup(p.path(), h.path())
            .arg("--resolve")
            .arg(format!("project/AGENTS.md=replace:{}", reviewed.display()))
            .arg("--out")
            .arg(&out)
            .output()
            .unwrap(),
    );
    assert!(ok, "{value}");
    let digest = value["data"]["acceptance"]["digest"]
        .as_str()
        .unwrap()
        .to_owned();
    std::fs::write(p.path().join("AGENTS.md"), b"edited after review\n").unwrap();

    let (ok, refused) = json(
        setup(p.path(), h.path())
            .arg("--state-root")
            .arg(s.path())
            .arg("--apply")
            .arg(&out)
            .arg("--accept")
            .arg(&digest)
            .output()
            .unwrap(),
    );
    assert!(!ok);
    assert_eq!(refused["diagnostics"][0]["path"], "/adoption/plan_stale");
    assert_eq!(
        std::fs::read(p.path().join("AGENTS.md")).unwrap(),
        b"edited after review\n"
    );
}

/// #1208 F7: the user's real Claude instruction file is operable end to end — the resolved plan
/// applies, and `restore` brings the original bytes back.
#[test]
fn the_user_claude_instruction_file_resolves_applies_and_restores() {
    const USER_ORIGINAL: &[u8] = b"Global: use the old pipeline\r\nDeny secrets\r\n";
    const USER_REVIEWED: &[u8] = b"Global: GraphHelm JPD\r\nDeny secrets\r\n";
    let (p, h) = seeded();
    std::fs::write(h.path().join(".claude/CLAUDE.md"), USER_ORIGINAL).unwrap();
    let s = private_dir();
    let reviewed = s.path().join("CLAUDE.reviewed.md");
    std::fs::write(&reviewed, USER_REVIEWED).unwrap();
    let out = s.path().join("plan.json");

    let (ok, preview) = json(setup(p.path(), h.path()).arg("--dry-run").output().unwrap());
    assert!(ok, "{preview}");
    let user = preview["data"]["plan"]["spec"]["decisions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|d| d["item"] == "home/.claude/CLAUDE.md")
        .cloned()
        .unwrap_or_else(|| panic!("home/.claude/CLAUDE.md missing: {preview}"));
    assert_eq!(user["operable"], true);

    let (ok, value) = json(
        setup(p.path(), h.path())
            .arg("--resolve")
            .arg("project/AGENTS.md=keep")
            .arg("--resolve")
            .arg(format!(
                "home/.claude/CLAUDE.md=replace:{}",
                reviewed.display()
            ))
            .arg("--out")
            .arg(&out)
            .output()
            .unwrap(),
    );
    assert!(ok, "{value}");
    let digest = value["data"]["acceptance"]["digest"]
        .as_str()
        .unwrap()
        .to_owned();
    let (ok, applied) = json(
        setup(p.path(), h.path())
            .arg("--state-root")
            .arg(s.path())
            .arg("--apply")
            .arg(&out)
            .arg("--accept")
            .arg(&digest)
            .output()
            .unwrap(),
    );
    assert!(ok, "{applied}");
    assert_eq!(
        std::fs::read(h.path().join(".claude/CLAUDE.md")).unwrap(),
        USER_REVIEWED
    );
    assert_eq!(std::fs::read(p.path().join("AGENTS.md")).unwrap(), ORIGINAL);

    let restore = |extra: &[&std::ffi::OsStr]| {
        let mut command = assert_cmd::Command::new(assert_cmd::cargo::cargo_bin!("graphhelm"));
        command.arg("restore").arg("--state-root").arg(s.path());
        for arg in extra {
            command.arg(arg);
        }
        json(command.output().unwrap())
    };
    let (ok, preview) = restore(&[]);
    assert!(ok, "{preview}");
    let plan = &preview["data"]["plan"];
    let planfile = s.path().join("restore.json");
    std::fs::write(&planfile, serde_json::to_vec(plan).unwrap()).unwrap();
    let (ok, restored) = restore(&[
        "--apply".as_ref(),
        planfile.as_os_str(),
        "--accept".as_ref(),
        plan["digest"].as_str().unwrap().as_ref(),
    ]);
    assert!(ok, "{restored}");
    assert_eq!(restored["data"]["receipt"]["spec"]["state"], "restored");
    assert_eq!(
        std::fs::read(h.path().join(".claude/CLAUDE.md")).unwrap(),
        USER_ORIGINAL
    );
}

/// #1323: `setup` registers the `graphhelm` MCP server at Claude Code's user scope
/// (`~/.claude.json`) and upserts the marked GraphHelm + Keel block into the user instruction file,
/// both through the reviewed plan -> digest -> journaled apply path. A second run finds nothing
/// left to do, the user's other servers and lines survive, and restore returns the original bytes.
#[test]
fn setup_registers_the_user_mcp_and_upserts_the_user_block_idempotently() {
    const USER_CLAUDE: &[u8] = b"# Mine\r\nDeny secrets\r\n";
    let user_json = br#"{"numStartups":3,"mcpServers":{"mine":{"command":"my-server","args":[]}}}"#;
    let (p, h) = seeded();
    std::fs::write(h.path().join(".claude/CLAUDE.md"), USER_CLAUDE).unwrap();
    std::fs::write(h.path().join(".claude.json"), user_json).unwrap();
    let s = private_dir();
    let out = s.path().join("plan.json");

    let (ok, preview) = json(setup(p.path(), h.path()).arg("--dry-run").output().unwrap());
    assert!(ok, "{preview}");
    let suggested = preview["data"]["suggestedResolutions"].as_array().unwrap();
    assert!(
        suggested.contains(&"home/.claude.json=register-mcp".into()),
        "{preview}"
    );
    assert!(
        suggested.contains(&"home/.claude/CLAUDE.md=graphhelm-block".into()),
        "{preview}"
    );

    let resolve = |out: &Path| {
        json(
            setup(p.path(), h.path())
                .arg("--resolve")
                .arg("project/AGENTS.md=keep")
                .arg("--resolve")
                .arg("home/.claude/CLAUDE.md=graphhelm-block")
                .arg("--resolve")
                .arg("home/.claude.json=register-mcp")
                .arg("--out")
                .arg(out)
                .output()
                .unwrap(),
        )
    };
    let (ok, value) = resolve(&out);
    assert!(ok, "{value}");
    let digest = value["data"]["acceptance"]["digest"]
        .as_str()
        .unwrap()
        .to_owned();

    // The redacted preview names the program the host will start, and nothing else of the file.
    let (ok, reviewed) = json(
        setup(p.path(), h.path())
            .arg("--plan")
            .arg(&out)
            .output()
            .unwrap(),
    );
    assert!(ok, "{reviewed}");
    let operation = reviewed["data"]["plan"]["spec"]["operations"]
        .as_array()
        .unwrap()
        .iter()
        .find(|o| o["path"] == ".claude.json")
        .cloned()
        .unwrap_or_else(|| panic!("no .claude.json operation: {reviewed}"));
    assert_eq!(operation["registration"]["args"][0], "mcp");
    // User-scope discovery follows the host's active project directory, not a fixed port/token.
    assert_eq!(operation["registration"]["args"][1], "--discover");
    assert_eq!(operation["registration"]["args"][2], "--actor");
    assert_eq!(operation["registration"]["args"][3], "agent-chat");
    assert!(
        !operation["registration"]["args"]
            .as_array()
            .unwrap()
            .contains(&"--token-file".into()),
        "{operation}"
    );

    let (ok, applied) = json(
        setup(p.path(), h.path())
            .arg("--state-root")
            .arg(s.path())
            .arg("--apply")
            .arg(&out)
            .arg("--accept")
            .arg(&digest)
            .output()
            .unwrap(),
    );
    assert!(ok, "{applied}");

    let registered: Value =
        serde_json::from_slice(&std::fs::read(h.path().join(".claude.json")).unwrap()).unwrap();
    assert_eq!(registered["numStartups"], 3);
    assert_eq!(registered["mcpServers"]["mine"]["command"], "my-server");
    let entry = &registered["mcpServers"]["graphhelm"];
    assert!(
        graphhelm_host_adoption::is_graphhelm_registration(entry),
        "{entry}"
    );
    // One user-scope entry discovers the active project by identity, never a fixed port/token.
    let args = entry["args"].as_array().unwrap();
    assert!(args.contains(&"--discover".into()), "{entry}");
    assert!(!args.contains(&"--url".into()), "{entry}");
    assert!(!args.contains(&"--token-file".into()), "{entry}");

    let user =
        String::from_utf8(std::fs::read(h.path().join(".claude/CLAUDE.md")).unwrap()).unwrap();
    assert!(user.starts_with("# Mine\r\nDeny secrets\r\n"), "{user:?}");
    assert_eq!(user.matches("<!-- graphhelm:begin -->").count(), 1);
    assert!(user.contains("mcp__graphhelm__"));
    assert!(
        !user.replace("\r\n", "").contains('\n'),
        "line endings follow the file"
    );

    // Second run: the block is current (keep) and the registration is present (nothing), so the
    // plan would change nothing and is refused rather than padded; no file moves.
    let again = s.path().join("again.json");
    let (ok, refused) = resolve(&again);
    assert!(!ok, "{refused}");
    assert_eq!(
        refused["diagnostics"][0]["path"],
        "/adoption/invalid_configuration"
    );
    assert!(!again.exists());

    let mut command = assert_cmd::Command::new(assert_cmd::cargo::cargo_bin!("graphhelm"));
    let (ok, restore) = json(
        command
            .arg("restore")
            .arg("--state-root")
            .arg(s.path())
            .output()
            .unwrap(),
    );
    assert!(ok, "{restore}");
    let plan = &restore["data"]["plan"];
    let planfile = s.path().join("restore.json");
    std::fs::write(&planfile, serde_json::to_vec(plan).unwrap()).unwrap();
    let mut command = assert_cmd::Command::new(assert_cmd::cargo::cargo_bin!("graphhelm"));
    let (ok, restored) = json(
        command
            .arg("restore")
            .arg("--state-root")
            .arg(s.path())
            .arg("--apply")
            .arg(&planfile)
            .arg("--accept")
            .arg(plan["digest"].as_str().unwrap())
            .output()
            .unwrap(),
    );
    assert!(ok, "{restored}");
    assert_eq!(
        std::fs::read(h.path().join(".claude.json")).unwrap(),
        user_json
    );
    assert_eq!(
        std::fs::read(h.path().join(".claude/CLAUDE.md")).unwrap(),
        USER_CLAUDE
    );
}

/// #1323: the generated decisions are bounded to their surfaces. A block cannot target a settings
/// file, a registration cannot target an instruction file, and an unknown decision is refused.
#[test]
fn generated_decisions_are_refused_outside_their_surfaces() {
    let (p, h) = seeded();
    let s = private_dir();
    let out = s.path().join("plan.json");
    for resolve in [
        "home/.claude/settings.json=graphhelm-block",
        "project/AGENTS.md=register-mcp",
        "project/AGENTS.md=graphhelm",
    ] {
        let (ok, refused) = json(
            setup(p.path(), h.path())
                .arg("--resolve")
                .arg(resolve)
                .arg("--out")
                .arg(&out)
                .output()
                .unwrap(),
        );
        assert!(!ok, "{resolve}");
        assert_eq!(
            refused["diagnostics"][0]["path"], "/adoption/invalid_configuration",
            "{resolve}"
        );
    }
    assert!(!out.exists());
}

/// The project registration produced by init must cross the same preview/apply guard as the
/// user-scope entry, while preserving unrelated settings and never printing their secrets.
#[test]
fn setup_applies_and_restores_the_init_project_registration() {
    let (p, h) = seeded();
    let s = private_dir();
    let original = br#"{"note":7,"mcpServers":{"mine":{"command":"my-server","env":{"API_KEY":"PRIVATE-OTHER-SERVER-SENTINEL"}}}}"#;
    let mcp = p.path().join(".mcp.json");
    let settings = h.path().join(".claude/settings.json");
    let original_settings = std::fs::read(&settings).unwrap();
    std::fs::write(&mcp, original).unwrap();
    let mut init = assert_cmd::Command::new(assert_cmd::cargo::cargo_bin!("graphhelm"));
    let (ok, initialized) = json(
        init.args(["init", "--project"])
            .arg(p.path())
            .args(["--harness", "claude-code"])
            .output()
            .unwrap(),
    );
    assert!(ok, "{initialized}");
    let initialized_mcp: Value = serde_json::from_slice(&std::fs::read(&mcp).unwrap()).unwrap();
    let entry = &initialized_mcp["mcpServers"]["graphhelm"];
    assert_eq!(
        entry["args"],
        serde_json::json!([
            "mcp",
            "--discover",
            "--project",
            std::path::absolute(p.path()).unwrap(),
            "--actor",
            "agent-chat"
        ])
    );
    // Rehearse setup from the pre-registration bytes in private temporary roots.
    std::fs::write(&mcp, original).unwrap();
    let out = s.path().join("plan.json");
    let resolve = |out: &Path| {
        json(
            setup(p.path(), h.path())
                .args([
                    "--resolve",
                    "project/AGENTS.md=keep",
                    "--resolve",
                    "project/.mcp.json=register-mcp",
                ])
                .arg("--out")
                .arg(out)
                .output()
                .unwrap(),
        )
    };
    let (ok, resolved) = resolve(&out);
    assert!(ok, "{resolved}");
    let digest = resolved["data"]["acceptance"]["digest"].as_str().unwrap();
    let (ok, reviewed) = json(
        setup(p.path(), h.path())
            .arg("--plan")
            .arg(&out)
            .output()
            .unwrap(),
    );
    assert!(ok, "{reviewed}");
    let operations = reviewed["data"]["plan"]["spec"]["operations"]
        .as_array()
        .unwrap();
    assert_eq!(operations.len(), 1);
    assert_eq!(operations[0]["path"], ".mcp.json");
    assert_eq!(&operations[0]["registration"], entry);
    let apply = || {
        json(
            setup(p.path(), h.path())
                .arg("--state-root")
                .arg(s.path())
                .arg("--apply")
                .arg(&out)
                .arg("--accept")
                .arg(digest)
                .output()
                .unwrap(),
        )
    };
    let (ok, applied) = apply();
    assert!(ok, "{applied}");
    let (ok, repeated) = apply();
    assert!(ok, "{repeated}");
    assert_eq!(applied["data"]["receipt"], repeated["data"]["receipt"]);
    let registered: Value = serde_json::from_slice(&std::fs::read(&mcp).unwrap()).unwrap();
    let mut expected: Value = serde_json::from_slice(original).unwrap();
    expected["mcpServers"]["graphhelm"] = entry.clone();
    assert_eq!(registered, expected);
    assert_eq!(std::fs::read(&settings).unwrap(), original_settings);
    assert_eq!(std::fs::read(p.path().join("AGENTS.md")).unwrap(), ORIGINAL);
    let (ok, again) = resolve(&s.path().join("again.json"));
    assert!(!ok, "{again}");
    assert_eq!(
        again["diagnostics"][0]["path"],
        "/adoption/invalid_configuration"
    );
    assert!(!s.path().join("again.json").exists());

    let mut restore = assert_cmd::Command::new(assert_cmd::cargo::cargo_bin!("graphhelm"));
    let (ok, preview) = json(
        restore
            .arg("restore")
            .arg("--state-root")
            .arg(s.path())
            .output()
            .unwrap(),
    );
    assert!(ok, "{preview}");
    let plan = &preview["data"]["plan"];
    let restore_file = s.path().join("restore.json");
    std::fs::write(&restore_file, serde_json::to_vec(plan).unwrap()).unwrap();
    let mut restore = assert_cmd::Command::new(assert_cmd::cargo::cargo_bin!("graphhelm"));
    let (ok, restored) = json(
        restore
            .arg("restore")
            .arg("--state-root")
            .arg(s.path())
            .arg("--apply")
            .arg(&restore_file)
            .arg("--accept")
            .arg(plan["digest"].as_str().unwrap())
            .output()
            .unwrap(),
    );
    assert!(ok, "{restored}");
    assert_eq!(restored["data"]["receipt"]["spec"]["state"], "restored");
    assert_eq!(std::fs::read(&mcp).unwrap(), original);
    assert_eq!(std::fs::read(&settings).unwrap(), original_settings);
    assert_eq!(std::fs::read(p.path().join("AGENTS.md")).unwrap(), ORIGINAL);
    let token = std::fs::read_to_string(p.path().join(".graphhelm/events.token")).unwrap();
    let key = std::fs::read_to_string(p.path().join(".graphhelm/serve.key")).unwrap();
    for envelope in [
        &initialized,
        &resolved,
        &reviewed,
        &applied,
        &repeated,
        &again,
        &preview,
        &restored,
    ] {
        let public = envelope.to_string();
        for secret in ["PRIVATE-OTHER-SERVER-SENTINEL", token.trim(), key.trim()] {
            assert!(!secret.is_empty());
            assert!(!public.contains(secret), "public response leaked a secret");
        }
    }
}
