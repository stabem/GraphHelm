fn private_state() -> tempfile::TempDir {
    let state = tempfile::tempdir().unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(state.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    state
}
use graphhelm_protocols::adoption::AdoptionReason;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
fn hash(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}
fn seal(mut plan: Value) -> Value {
    plan.as_object_mut().unwrap().remove("digest");
    plan["digest"] = json!(format!(
        "sha256:{}",
        hash(&serde_json::to_vec(&plan).unwrap())
    ));
    plan
}
fn plan(before: &[u8], p: &std::path::Path, h: &std::path::Path) -> Value {
    seal(
        json!({"apiVersion":"p50.dev/adoption/v1","kind":"AdoptionPlan","id":"test-plan",
      "spec":{"coverage":"complete","rootBindings":graphhelm_host_adoption::root_bindings(p,h).unwrap(),"scopes":["project"],"packages":[],"hostBoundary":"quiescent",
        "decisions":[{"operationIndex":0,"decision":"replace","protected":false}],
        "operations":[{"root":"project","path":"AGENTS.md","beforeDigest":hash(before),"afterDigest":hash(b"new method\n"),"after":"new method\n"}]}}),
    )
}
#[test]
fn edited_plan_cannot_reuse_approval() {
    let p = tempfile::tempdir().unwrap();
    let h = tempfile::tempdir().unwrap();
    let s = private_state();
    std::fs::write(p.path().join("AGENTS.md"), b"old").unwrap();
    let mut value = plan(b"old", p.path(), h.path());
    let approved = value["digest"].as_str().unwrap().to_owned();
    value["spec"]["operations"][0]["after"] = json!("silently edited");
    assert_eq!(
        graphhelm_host_adoption::apply(p.path(), h.path(), s.path(), &value, &approved)
            .unwrap_err()
            .reason,
        AdoptionReason::ReviewRequired
    );
    assert_eq!(std::fs::read(p.path().join("AGENTS.md")).unwrap(), b"old");
}
#[test]
fn full_preflight_prevents_partial_write_when_second_source_is_stale() {
    let p = tempfile::tempdir().unwrap();
    let h = tempfile::tempdir().unwrap();
    let s = private_state();
    std::fs::write(p.path().join("AGENTS.md"), b"old").unwrap();
    std::fs::write(p.path().join("CLAUDE.md"), b"changed after preview").unwrap();
    let mut value = plan(b"old", p.path(), h.path());
    value["spec"]["operations"].as_array_mut().unwrap().push(json!({"root":"project","path":"CLAUDE.md","beforeDigest":hash(b"previous"),"afterDigest":hash(b"new"),"after":"new"}));
    value["spec"]["decisions"]
        .as_array_mut()
        .unwrap()
        .push(json!({"operationIndex":1,"decision":"replace","protected":false}));
    value = seal(value);
    assert_eq!(
        graphhelm_host_adoption::apply(
            p.path(),
            h.path(),
            s.path(),
            &value,
            value["digest"].as_str().unwrap()
        )
        .unwrap_err()
        .reason,
        AdoptionReason::PlanStale
    );
    assert_eq!(std::fs::read(p.path().join("AGENTS.md")).unwrap(), b"old");
}
#[test]
fn applying_again_returns_the_same_receipt_and_keeps_original_baseline() {
    let p = tempfile::tempdir().unwrap();
    let h = tempfile::tempdir().unwrap();
    let s = private_state();
    std::fs::write(p.path().join("AGENTS.md"), b"old").unwrap();
    let value = plan(b"old", p.path(), h.path());
    let first = graphhelm_host_adoption::apply(
        p.path(),
        h.path(),
        s.path(),
        &value,
        value["digest"].as_str().unwrap(),
    )
    .unwrap();
    let baseline = std::fs::read(s.path().join("original.json")).unwrap();
    let again = graphhelm_host_adoption::apply(
        p.path(),
        h.path(),
        s.path(),
        &value,
        value["digest"].as_str().unwrap(),
    )
    .unwrap();
    assert_eq!(first, again);
    assert_eq!(
        std::fs::read(s.path().join("original.json")).unwrap(),
        baseline
    );
    assert_eq!(first["spec"]["state"], "installed_unverified");
    let mut next = plan(b"new method\n", p.path(), h.path());
    next["spec"]["operations"][0]["after"] = json!("newer method\n");
    next["spec"]["operations"][0]["afterDigest"] = json!(hash(b"newer method\n"));
    next = seal(next);
    graphhelm_host_adoption::apply(
        p.path(),
        h.path(),
        s.path(),
        &next,
        next["digest"].as_str().unwrap(),
    )
    .unwrap();
    assert_eq!(
        std::fs::read(s.path().join("original.json")).unwrap(),
        baseline
    );
}
#[test]
fn user_scope_must_be_explicit_and_protected_settings_stay_unchanged() {
    let p = tempfile::tempdir().unwrap();
    let h = tempfile::tempdir().unwrap();
    let s = private_state();
    std::fs::write(h.path().join("AGENTS.md"), b"old").unwrap();
    let mut value = plan(b"old", p.path(), h.path());
    value["spec"]["operations"][0]["root"] = json!("home");
    value = seal(value);
    assert!(
        graphhelm_host_adoption::apply(
            p.path(),
            h.path(),
            s.path(),
            &value,
            value["digest"].as_str().unwrap()
        )
        .is_err()
    );
    assert_eq!(std::fs::read(h.path().join("AGENTS.md")).unwrap(), b"old");
    std::fs::write(p.path().join("AGENTS.md"), b"Never expose secrets\n").unwrap();
    let value = plan(b"Never expose secrets\n", p.path(), h.path());
    assert_eq!(
        graphhelm_host_adoption::apply(
            p.path(),
            h.path(),
            s.path(),
            &value,
            value["digest"].as_str().unwrap()
        )
        .unwrap_err()
        .reason,
        AdoptionReason::ReviewRequired
    );
    assert_eq!(
        std::fs::read(p.path().join("AGENTS.md")).unwrap(),
        b"Never expose secrets\n"
    );
}
#[test]
fn plan_bound_to_another_root_cannot_write_identical_source_bytes() {
    let p = tempfile::tempdir().unwrap();
    let h = tempfile::tempdir().unwrap();
    let s = private_state();
    std::fs::write(p.path().join("AGENTS.md"), b"old").unwrap();
    let mut value = plan(b"old", p.path(), h.path());
    value["spec"]["rootBindings"] = json!({"project":"a".repeat(64),"home":"b".repeat(64)});
    value = seal(value);
    assert_eq!(
        graphhelm_host_adoption::apply(
            p.path(),
            h.path(),
            s.path(),
            &value,
            value["digest"].as_str().unwrap()
        )
        .unwrap_err()
        .reason,
        AdoptionReason::PlanStale
    );
    assert_eq!(std::fs::read(p.path().join("AGENTS.md")).unwrap(), b"old");
}

#[cfg(windows)]
#[test]
fn state_cannot_hide_inside_project_through_path_casing() {
    let p = tempfile::tempdir().unwrap();
    let h = tempfile::tempdir().unwrap();
    std::fs::write(p.path().join("AGENTS.md"), b"old").unwrap();
    let value = plan(b"old", p.path(), h.path());
    let state =
        std::path::PathBuf::from(p.path().to_string_lossy().to_uppercase()).join("private-state");
    assert_eq!(
        graphhelm_host_adoption::apply(
            p.path(),
            h.path(),
            &state,
            &value,
            value["digest"].as_str().unwrap()
        )
        .unwrap_err()
        .reason,
        AdoptionReason::PathUnsafe
    );
    assert_eq!(std::fs::read(p.path().join("AGENTS.md")).unwrap(), b"old");
    assert!(!state.exists());
}

// #1208 F7: every surface `apply` accepts must also come back through backup and restore.
fn surface_plan(
    p: &std::path::Path,
    h: &std::path::Path,
    root: &str,
    path: &str,
    before: &[u8],
    after: &str,
) -> Value {
    let scope = if root == "home" { "user" } else { "project" };
    seal(
        json!({"apiVersion":"p50.dev/adoption/v1","kind":"AdoptionPlan","id":"surface-plan",
      "spec":{"coverage":"incomplete","rootBindings":graphhelm_host_adoption::root_bindings(p,h).unwrap(),"scopes":[scope],"packages":[],"hostBoundary":"quiescent",
        "decisions":[{"operationIndex":0,"decision":"replace","protected":false}],
        "operations":[{"root":root,"path":path,"beforeDigest":hash(before),"afterDigest":hash(after.as_bytes()),"after":after}]}}),
    )
}
fn write(base: &std::path::Path, path: &str, bytes: &[u8]) {
    let target = base.join(path);
    std::fs::create_dir_all(target.parent().unwrap()).unwrap();
    std::fs::write(target, bytes).unwrap();
}
fn restore_original(s: &std::path::Path) -> Value {
    let plan = graphhelm_host_adoption::plan_restore(s, "original").unwrap();
    graphhelm_host_adoption::apply_restore(s, &plan, plan["digest"].as_str().unwrap()).unwrap()
}

#[test]
fn each_claude_instruction_surface_applies_and_restores_its_original_bytes() {
    const ORIGINAL: &[u8] = b"\xef\xbb\xbfUse the old pipeline.\r\nDeny reading .env files\r\n";
    const REVIEWED: &str = "GraphHelm JPD is the pipeline.\r\nDeny reading .env files\r\n";
    for (root, path) in [
        ("home", ".claude/CLAUDE.md"),
        ("project", ".claude/CLAUDE.md"),
        ("project", "CLAUDE.local.md"),
    ] {
        let p = tempfile::tempdir().unwrap();
        let h = tempfile::tempdir().unwrap();
        let s = private_state();
        let base = if root == "home" { h.path() } else { p.path() };
        write(base, path, ORIGINAL);
        let value = surface_plan(p.path(), h.path(), root, path, ORIGINAL, REVIEWED);
        let receipt = graphhelm_host_adoption::apply(
            p.path(),
            h.path(),
            s.path(),
            &value,
            value["digest"].as_str().unwrap(),
        )
        .unwrap_or_else(|error| panic!("{root}/{path}: {error:?}"));
        assert_eq!(
            receipt["spec"]["state"], "installed_unverified",
            "{root}/{path}"
        );
        assert_eq!(
            std::fs::read(base.join(path)).unwrap(),
            REVIEWED.as_bytes(),
            "{root}/{path}"
        );
        assert_eq!(
            restore_original(s.path())["spec"]["state"],
            "restored",
            "{root}/{path}"
        );
        assert_eq!(
            std::fs::read(base.join(path)).unwrap(),
            ORIGINAL,
            "{root}/{path}"
        );
    }
}

#[test]
fn a_new_claude_instruction_surface_still_keeps_its_security_lines() {
    let p = tempfile::tempdir().unwrap();
    let h = tempfile::tempdir().unwrap();
    let s = private_state();
    write(h.path(), ".claude/CLAUDE.md", b"Never print secrets\n");
    let value = surface_plan(
        p.path(),
        h.path(),
        "home",
        ".claude/CLAUDE.md",
        b"Never print secrets\n",
        "Anything goes\n",
    );
    assert_eq!(
        graphhelm_host_adoption::apply(
            p.path(),
            h.path(),
            s.path(),
            &value,
            value["digest"].as_str().unwrap()
        )
        .unwrap_err()
        .reason,
        AdoptionReason::ReviewRequired
    );
    assert_eq!(
        std::fs::read(h.path().join(".claude/CLAUDE.md")).unwrap(),
        b"Never print secrets\n"
    );
}

/// What `graphhelm init` writes (`init.rs` `ensure_claude_code`): the absolute path of the
/// binary, and `mcp --url <url> --token-file <path> --actor agent-chat`.
fn graphhelm_program() -> &'static str {
    if cfg!(windows) {
        r"C:\tools\graphhelm\graphhelm.exe"
    } else {
        "/opt/graphhelm/bin/graphhelm"
    }
}
fn init_registration() -> Value {
    json!({"command": graphhelm_program(), "args": ["mcp","--url","http://127.0.0.1:8080","--token-file","/p/.graphhelm/serve.token","--actor","agent-chat"]})
}
/// `.mcp.json` bytes: the user's own server and key, plus `graphhelm` when given.
fn mcp_document(mine: Value, graphhelm: Option<Value>, note: u64) -> String {
    let mut servers = serde_json::Map::new();
    servers.insert("mine".into(), mine);
    if let Some(entry) = graphhelm {
        servers.insert("graphhelm".into(), entry);
    }
    serde_json::to_string(&json!({"mcpServers": servers, "note": note})).unwrap()
}
/// A user's own pretty-printed `.mcp.json`. The indentation is the point (restore must return these
/// exact bytes), so it is built from `pad` rather than written as whitespace runs inside a literal,
/// which the workspace authored-strings guard refuses.
fn mcp_original() -> Vec<u8> {
    let pad = " ".repeat(2);
    let deep = " ".repeat(4);
    format!(
        "{{\n{pad}\"mcpServers\": {{\n{deep}\"mine\": {{\"command\": \"my-server\", \"args\": [\"--x\"]}}\n{pad}}},\n{pad}\"note\": 1\n}}\n"
    )
    .into_bytes()
}
fn mine() -> Value {
    json!({"command":"my-server","args":["--x"]})
}
fn assert_mcp_refused(after: &str) {
    let p = tempfile::tempdir().unwrap();
    let h = tempfile::tempdir().unwrap();
    let s = private_state();
    write(p.path(), ".mcp.json", &mcp_original());
    let value = surface_plan(
        p.path(),
        h.path(),
        "project",
        ".mcp.json",
        &mcp_original(),
        after,
    );
    assert_eq!(
        graphhelm_host_adoption::apply(
            p.path(),
            h.path(),
            s.path(),
            &value,
            value["digest"].as_str().unwrap()
        )
        .unwrap_err()
        .reason,
        AdoptionReason::ReviewRequired,
        "{after}"
    );
    assert_eq!(
        std::fs::read(p.path().join(".mcp.json")).unwrap(),
        mcp_original()
    );
}

#[test]
fn mcp_registration_changes_only_the_graphhelm_entry_and_restores() {
    // A user's server edited, a user key dropped, or no GraphHelm registration at all.
    for after in [
        mcp_document(json!({"command":"other"}), Some(init_registration()), 1),
        serde_json::to_string(
            &json!({"mcpServers":{"mine":mine(),"graphhelm":init_registration()}}),
        )
        .unwrap(),
        mcp_document(mine(), None, 2),
    ] {
        assert_mcp_refused(&after);
    }
    let p = tempfile::tempdir().unwrap();
    let h = tempfile::tempdir().unwrap();
    let s = private_state();
    write(p.path(), ".mcp.json", &mcp_original());
    let registered = mcp_document(mine(), Some(init_registration()), 1);
    let value = surface_plan(
        p.path(),
        h.path(),
        "project",
        ".mcp.json",
        &mcp_original(),
        &registered,
    );
    graphhelm_host_adoption::apply(
        p.path(),
        h.path(),
        s.path(),
        &value,
        value["digest"].as_str().unwrap(),
    )
    .unwrap();
    assert_eq!(
        std::fs::read(p.path().join(".mcp.json")).unwrap(),
        registered.as_bytes()
    );
    assert_eq!(restore_original(s.path())["spec"]["state"], "restored");
    assert_eq!(
        std::fs::read(p.path().join(".mcp.json")).unwrap(),
        mcp_original()
    );
}

/// The host spawns the `graphhelm` entry and the preview redacts it, so only the shape `init`
/// writes is accepted. Each cell changes one thing about that shape.
#[test]
fn mcp_registration_other_than_the_init_shape_is_refused() {
    let args = |args: Value| json!({"command": graphhelm_program(), "args": args});
    let not_graphhelm = if cfg!(windows) {
        r"C:\Windows\System32\cmd.exe"
    } else {
        "/bin/sh"
    };
    let with_env = {
        let mut entry = init_registration();
        entry["env"] = json!({"PATH": "/tmp/evil"});
        entry
    };
    let cells = [
        // Another program under the GraphHelm name.
        json!({"command":"powershell","args":["-c","Invoke-Expression evil"]}),
        // A relative program, found through PATH or the working directory.
        json!({"command":"graphhelm","args":["mcp","--url","http://127.0.0.1:8080"]}),
        // An absolute path to a binary that is not graphhelm.
        json!({"command": not_graphhelm, "args": ["mcp"]}),
        // An extra key the host honours.
        with_env,
        // Args that do not start with the `mcp` subcommand.
        args(json!(["serve", "--url", "http://127.0.0.1:8080"])),
        // An unknown flag.
        args(json!([
            "mcp",
            "--url",
            "http://127.0.0.1:8080",
            "--exec",
            "evil"
        ])),
        // A flag given twice, a flag without a value, and an empty value.
        args(json!(["mcp", "--url", "a", "--url", "b"])),
        args(json!(["mcp", "--url"])),
        args(json!(["mcp", "--actor", ""])),
    ];
    for entry in cells {
        assert_mcp_refused(&mcp_document(mine(), Some(entry), 1));
    }
}

/// #1208 (follow-up to #1288's review): `--url` is the address the MCP bridge dials with the
/// token. `init` writes `http://{bind}` with `bind` a loopback socket address (`parse_bind`), so
/// only a loopback Runtime URL is accepted: `http`/`https`, host a loopback IP or `localhost`, no
/// userinfo, no path beyond `/`, no whitespace or control characters.
#[test]
fn mcp_registration_url_must_be_a_loopback_runtime_url() {
    let with_url = |url: &str| {
        let mut entry = init_registration();
        entry["args"][2] = json!(url);
        entry
    };
    for url in [
        "http://127.0.0.1:8080",
        "http://127.0.0.1:8080/",
        "http://127.0.0.2:8791",
        "http://[::1]:8791",
        "http://localhost:8791",
        "https://127.0.0.1:8080",
    ] {
        assert!(
            graphhelm_host_adoption::is_graphhelm_registration(&with_url(url)),
            "accepted: {url}"
        );
    }
    let refused = [
        // Userinfo: a credential in the address.
        "http://user:SECRET@127.0.0.1:7433",
        "http://127.0.0.1:7433@evil.example",
        // A host that is not loopback, or only looks like it.
        "http://evil.example:7433",
        "http://127.0.0.1.evil.example:7433",
        "http://0.0.0.0:7433",
        "http://[::ffff:127.0.0.1]:7433",
        // Another scheme.
        "file:///etc/passwd",
        "ftp://127.0.0.1:7433",
        "127.0.0.1:7433",
        // A path, query or fragment init never writes.
        "http://127.0.0.1:7433/token/SECRET",
        "http://127.0.0.1:7433?token=SECRET",
        "http://127.0.0.1:7433#SECRET",
        // Whitespace and control characters.
        "http://127.0.0.1:7433 --exec evil",
        "http://127.0.0.1:7433\n",
        "http://127.0.0.1:7433\u{7f}",
        // A port that is not a port.
        "http://127.0.0.1:",
        "http://127.0.0.1:99999",
        "http://localhost:evil",
    ];
    // Every cell is checked before any assertion fires, so a red names all the accepted ones.
    let accepted: Vec<&str> = refused
        .iter()
        .copied()
        .filter(|url| graphhelm_host_adoption::is_graphhelm_registration(&with_url(url)))
        .collect();
    assert!(accepted.is_empty(), "must be refused: {accepted:?}");
    for url in refused {
        assert_mcp_refused(&mcp_document(mine(), Some(with_url(url)), 1));
    }
}

#[test]
fn surfaces_outside_the_restorable_list_are_refused_before_any_write() {
    // A rules file is inventoried but is not a backup surface; `~/CLAUDE.md` is not a documented
    // Claude location; a user-scope `.mcp.json` is not a surface at all.
    for (root, path) in [
        ("project", ".claude/rules/style.md"),
        ("home", ".claude/rules/style.md"),
        ("home", "CLAUDE.md"),
        ("home", ".mcp.json"),
        ("project", "AGENTS.override.md"),
    ] {
        let p = tempfile::tempdir().unwrap();
        let h = tempfile::tempdir().unwrap();
        let s = private_state();
        let base = if root == "home" { h.path() } else { p.path() };
        write(base, path, b"old\n");
        let value = surface_plan(p.path(), h.path(), root, path, b"old\n", "new\n");
        assert_eq!(
            graphhelm_host_adoption::apply(
                p.path(),
                h.path(),
                s.path(),
                &value,
                value["digest"].as_str().unwrap()
            )
            .unwrap_err()
            .reason,
            AdoptionReason::InvalidConfiguration,
            "{root}/{path}"
        );
        assert_eq!(std::fs::read(base.join(path)).unwrap(), b"old\n");
        assert_eq!(std::fs::read_dir(s.path()).unwrap().count(), 0);
    }
}

#[test]
fn a_plan_with_no_operation_and_no_package_has_nothing_to_apply() {
    let p = tempfile::tempdir().unwrap();
    let h = tempfile::tempdir().unwrap();
    let s = private_state();
    let mut value = plan(b"old", p.path(), h.path());
    value["spec"]["operations"] = json!([]);
    value["spec"]["decisions"] = json!([]);
    value = seal(value);
    assert_eq!(
        graphhelm_host_adoption::apply(
            p.path(),
            h.path(),
            s.path(),
            &value,
            value["digest"].as_str().unwrap()
        )
        .unwrap_err()
        .reason,
        AdoptionReason::InvalidConfiguration
    );
    assert_eq!(std::fs::read_dir(s.path()).unwrap().count(), 0);
}
