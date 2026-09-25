use super::*;

#[test]
fn disjoint_json_and_toml_edits_survive_but_same_key_and_arrays_conflict() {
    for (path, base, installed, current, expected) in [
        (
            ".claude/settings.json",
            r#"{"plugins":{"old":true},"other":1}"#,
            r#"{"plugins":{"old":false},"other":1}"#,
            r#"{"plugins":{"old":false},"other":2,"added":"mine"}"#,
            json!({"plugins":{"old":true},"other":2,"added":"mine"}),
        ),
        (
            ".codex/config.toml",
            "model='old'\nother=1\n",
            "model='graphhelm'\nother=1\n",
            "model='graphhelm'\nother=2\nadded='mine'\n",
            json!({"model":"old","other":2,"added":"mine"}),
        ),
    ] {
        let merged = restore_document(
            path,
            base.as_bytes(),
            installed.as_bytes(),
            current.as_bytes(),
        )
        .unwrap();
        assert_eq!(parse_document(path, &merged).unwrap(), expected);
    }
    for (base, installed, current) in [
        (
            json!({"method":"old"}),
            json!({"method":"graphhelm"}),
            json!({"method":"mine"}),
        ),
        (
            json!({"list":[1]}),
            json!({"list":[1,2]}),
            json!({"list":[1,2,3]}),
        ),
    ] {
        assert!(
            restore_document(
                "x.json",
                &serde_json::to_vec(&base).unwrap(),
                &serde_json::to_vec(&installed).unwrap(),
                &serde_json::to_vec(&current).unwrap()
            )
            .is_err()
        );
    }
    assert!(restore_document("AGENTS.md", b"old", b"installed", b"later").is_err());
}

#[test]
fn restore_original_reverses_each_adoption_and_keeps_intervening_json_and_toml_edits() {
    for (path, original, first, user, second, expected) in [
        (
            "settings.json",
            br#"{"method":"original","keep":true}"# as &[u8],
            br#"{"method":"a","keep":true}"# as &[u8],
            br#"{"method":"a","keep":true,"user":"mine"}"# as &[u8],
            br#"{"method":"a","keep":true,"user":"mine","second":"b"}"# as &[u8],
            json!({"method":"original","keep":true,"user":"mine"}),
        ),
        (
            "config.toml",
            b"method = 'original'\nkeep = true\n" as &[u8],
            b"method = 'a'\nkeep = true\n" as &[u8],
            b"method = 'a'\nkeep = true\nuser = 'mine'\n" as &[u8],
            b"method = 'a'\nkeep = true\nuser = 'mine'\nsecond = 'b'\n" as &[u8],
            json!({"method":"original","keep":true,"user":"mine"}),
        ),
    ] {
        let restored = restore_transitions(path, second, &[(user, second), (original, first)])
            .expect("each reviewed installation is reversed from newest to oldest");
        assert_eq!(parse_document(path, &restored).unwrap(), expected, "{path}");
    }
    assert!(
        restore_transitions(
            "AGENTS.md",
            b"installed-b\n",
            &[
                (b"later user prose\n" as &[u8], b"installed-b\n" as &[u8]),
                (b"original\n" as &[u8], b"installed-a\n" as &[u8]),
            ],
        )
        .is_err()
    );
}

#[test]
fn user_added_subkeys_survive_removal_of_an_adoption_created_object() {
    let merged = restore_document(
        "x.json",
        b"{}",
        br#"{"owned":{"key":"installed"}}"#,
        br#"{"owned":{"key":"installed","later":"mine"}}"#,
    )
    .unwrap();
    assert_eq!(
        parse_document("x.json", &merged).unwrap(),
        json!({"owned":{"later":"mine"}})
    );
}

fn fixture(root: &Path) -> Value {
    let p = root.join("project");
    let h = root.join("home");
    let s = root.join("state");
    for dir in [&p, &h, &s] {
        std::fs::create_dir(dir).unwrap();
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&s, std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    let ops=[("AGENTS.md","First original\r\n","First installed\n"),("CLAUDE.md","Second original\r\n","Second installed\n")].map(|(path,before,after)| {
        std::fs::write(p.join(path),before).unwrap();json!({"root":"project","path":path,"beforeDigest":apply::digest(before.as_bytes()),"afterDigest":apply::digest(after.as_bytes()),"after":after})
    });
    let setup=seal(json!({"apiVersion":"p50.dev/adoption/v1","kind":"AdoptionPlan","id":"restore-crash","spec":{"coverage":"complete","rootBindings":crate::root_bindings(&p,&h).unwrap(),"scopes":["project"],"packages":[],"hostBoundary":"quiescent","decisions":[{"operationIndex":0,"decision":"replace","protected":false},{"operationIndex":1,"decision":"replace","protected":false}],"operations":ops}})).unwrap();
    crate::apply(&p, &h, &s, &setup, setup["digest"].as_str().unwrap()).unwrap();
    let plan = plan_restore(&s, "original").unwrap();
    std::fs::write(
        s.join("restore-plan.json"),
        serde_json::to_vec(&plan).unwrap(),
    )
    .unwrap();
    plan
}
#[test]
fn restore_process_child() {
    let Ok(root) = std::env::var("GH_RESTORE_TEST_ROOT") else {
        return;
    };
    let state = Path::new(&root).join("state");
    let plan: Value =
        serde_json::from_slice(&std::fs::read(state.join("restore-plan.json")).unwrap()).unwrap();
    let point: usize = std::env::var("GH_RESTORE_TEST_POINT")
        .unwrap()
        .parse()
        .unwrap();
    let mut n = 0;
    apply_engine(&state, &plan, plan["digest"].as_str().unwrap(), &mut |_| {
        n += 1;
        if n == point {
            std::process::exit(71)
        }
        Ok(())
    })
    .unwrap();
}
#[test]
fn every_restore_boundary_recovers_in_a_fresh_process() {
    let _diagnostics = storage::diagnostic_scope();
    let counter = tempfile::tempdir().unwrap();
    let plan = fixture(counter.path());
    let mut count = 0;
    apply_engine(
        &counter.path().join("state"),
        &plan,
        plan["digest"].as_str().unwrap(),
        &mut |_| {
            count += 1;
            Ok(())
        },
    )
    .unwrap();
    assert!(count >= 15);
    for point in 1..=count {
        let temp = tempfile::tempdir().unwrap();
        let plan = fixture(temp.path());
        let state = temp.path().join("state");
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "restore::tests::restore_process_child",
                "--nocapture",
            ])
            .env("GH_RESTORE_TEST_ROOT", temp.path())
            .env("GH_RESTORE_TEST_POINT", point.to_string())
            .env("GRAPHHELM_ADOPTION_DIAGNOSTICS", "1")
            .status()
            .unwrap();
        assert_eq!(status.code(), Some(71), "point {point}");
        let id = apply::digest(plan["digest"].as_str().unwrap().as_bytes());
        let receipt = if state.join("journals").join(format!("{id}.json")).exists() {
            crate::recover(&state, &id).unwrap()
        } else {
            apply_restore(&state, &plan, plan["digest"].as_str().unwrap()).unwrap()
        };
        assert_eq!(receipt["spec"]["state"], "restored", "point {point}");
        assert_eq!(
            std::fs::read(temp.path().join("project/AGENTS.md")).unwrap(),
            b"First original\r\n",
            "point {point}"
        );
        assert_eq!(
            std::fs::read(temp.path().join("project/CLAUDE.md")).unwrap(),
            b"Second original\r\n",
            "point {point}"
        );
    }
}

#[test]
fn repeated_restore_never_reports_success_after_new_user_edits() {
    let _diagnostics = storage::diagnostic_scope();
    let temp = tempfile::tempdir().unwrap();
    let plan = fixture(temp.path());
    let state = temp.path().join("state");
    apply_restore(&state, &plan, plan["digest"].as_str().unwrap()).unwrap();
    std::fs::write(temp.path().join("project/AGENTS.md"), b"new user choice").unwrap();
    assert!(apply_restore(&state, &plan, plan["digest"].as_str().unwrap()).is_err());
    assert_eq!(
        std::fs::read(temp.path().join("project/AGENTS.md")).unwrap(),
        b"new user choice"
    );
}

#[test]
fn partial_restore_returns_recovery_receipt_and_preserves_interfering_writer() {
    let _diagnostics = storage::diagnostic_scope();
    let temp = tempfile::tempdir().unwrap();
    let plan = fixture(temp.path());
    let state = temp.path().join("state");
    let mut publications = 0;
    let receipt = apply_engine(
        &state,
        &plan,
        plan["digest"].as_str().unwrap(),
        &mut |boundary| {
            if boundary == Boundary::BeforePublish {
                publications += 1;
                if publications == 2 {
                    std::fs::write(temp.path().join("project/CLAUDE.md"), b"user choice").unwrap();
                }
            }
            Ok(())
        },
    )
    .expect("a durable partial restore returns its truthful recovery receipt");
    assert_eq!(receipt["spec"]["state"], "recovery_required");
    assert_eq!(
        std::fs::read(temp.path().join("project/AGENTS.md")).unwrap(),
        b"First original\r\n"
    );
    assert_eq!(
        std::fs::read(temp.path().join("project/CLAUDE.md")).unwrap(),
        b"user choice"
    );
    let id = apply::digest(plan["digest"].as_str().unwrap().as_bytes());
    assert_eq!(
        crate::recover(&state, &id).unwrap()["spec"]["state"],
        "recovery_required"
    );
}

/// The adoption state directory must be private on unix: `backup` refuses a state directory any
/// other user can read (`unix_check_private_dir`, mode & 0o077 == 0). `std::fs::create_dir`
/// honours the umask (typically 0o755), so without this the two cells below failed on Linux with
/// `PathUnsafe` before reaching the restore they test (#1295). `tempfile::tempdir` already creates
/// 0o700 directories, which is why the integration suites never hit it.
fn private_state_dir(state: &std::path::Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(state, std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    #[cfg(not(unix))]
    let _ = state;
}

#[test]
fn manual_restore_interruption_recovers_with_empty_source_chain() {
    let temp = tempfile::tempdir().unwrap();
    let project = temp.path().join("project");
    let home = temp.path().join("home");
    let state = temp.path().join("state");
    for root in [&project, &home, &state] {
        std::fs::create_dir(root).unwrap();
    }
    private_state_dir(&state);
    std::fs::write(project.join("AGENTS.md"), b"checkpoint\n").unwrap();
    let checkpoint = crate::backup(&project, &home, &state).unwrap();
    std::fs::write(project.join("AGENTS.md"), b"later\n").unwrap();
    let plan = plan_restore(&state, checkpoint["id"].as_str().unwrap()).unwrap();
    let receipt = apply_engine(
        &state,
        &plan,
        plan["digest"].as_str().unwrap(),
        &mut |boundary| {
            if boundary == Boundary::BeforePublish {
                return Err(graphhelm_protocols::adoption::AdoptionError {
                    reason: graphhelm_protocols::adoption::AdoptionReason::Busy,
                });
            }
            Ok(())
        },
    )
    .unwrap();
    assert_eq!(receipt["spec"]["state"], "recovery_required");
    let recovered = apply_restore(&state, &plan, plan["digest"].as_str().unwrap()).unwrap();
    assert_eq!(recovered["spec"]["state"], "restored");
    assert_eq!(
        std::fs::read(project.join("AGENTS.md")).unwrap(),
        b"checkpoint\n"
    );
}

#[cfg(unix)]
#[test]
fn manual_restore_recovery_accepts_published_metadata_only_change() {
    use std::os::unix::fs::PermissionsExt;

    let temp = tempfile::tempdir().unwrap();
    let project = temp.path().join("project");
    let home = temp.path().join("home");
    let state = temp.path().join("state");
    for root in [&project, &home, &state] {
        std::fs::create_dir(root).unwrap();
    }
    private_state_dir(&state);
    let path = project.join("AGENTS.md");
    std::fs::write(&path, b"checkpoint\n").unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o640)).unwrap();
    let checkpoint = crate::backup(&project, &home, &state).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
    let plan = plan_restore(&state, checkpoint["id"].as_str().unwrap()).unwrap();
    let receipt = apply_engine(
        &state,
        &plan,
        plan["digest"].as_str().unwrap(),
        &mut |boundary| {
            if boundary == Boundary::AfterCompensationCapture {
                return Err(graphhelm_protocols::adoption::AdoptionError {
                    reason: graphhelm_protocols::adoption::AdoptionReason::Busy,
                });
            }
            Ok(())
        },
    )
    .unwrap();
    assert_eq!(receipt["spec"]["state"], "recovery_required");
    let recovered = apply_restore(&state, &plan, plan["digest"].as_str().unwrap()).unwrap();
    assert_eq!(recovered["spec"]["state"], "restored");
    assert_eq!(std::fs::read(&path).unwrap(), b"checkpoint\n");
    assert_eq!(
        std::fs::metadata(path).unwrap().permissions().mode() & 0o777,
        0o640
    );
}
