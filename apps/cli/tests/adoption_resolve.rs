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
