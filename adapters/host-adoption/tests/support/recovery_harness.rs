//! Process boundaries exercise durable files, never a production environment failpoint.
use super::*;
use serde_json::json;
use std::process::Command;
fn fixture_plan(root: &Path) -> Value {
    let (p, h, _) = dirs(root);
    let mut p = json!({"apiVersion":"p50.dev/adoption/v1","kind":"AdoptionPlan","id":"crash-plan","spec":{
 "coverage":"complete","rootBindings":root_bindings(&p,&h).unwrap(),"scopes":["project"],"packages":[],"hostBoundary":"quiescent",
 "decisions":[{"operationIndex":0,"decision":"replace","protected":false},{"operationIndex":1,"decision":"replace","protected":false}],
 "operations":[{"root":"project","path":"AGENTS.md","beforeDigest":digest(b"\xef\xbb\xbfOld A\r\n"),"afterDigest":digest(b"New A\n"),"after":"New A\n"},{"root":"project","path":"CLAUDE.md","beforeDigest":digest(b"Old B\r\n"),"afterDigest":digest(b"New B\n"),"after":"New B\n"}]}});
    p["digest"] = json!(format!(
        "sha256:{}",
        digest(&serde_json::to_vec(&p).unwrap())
    ));
    p
}
fn dirs(root: &Path) -> (std::path::PathBuf, std::path::PathBuf, std::path::PathBuf) {
    (root.join("project"), root.join("home"), root.join("state"))
}
fn fixture(root: &Path) {
    let (p, h, s) = dirs(root);
    for x in [&p, &h, &s] {
        std::fs::create_dir(x).unwrap();
    }
    std::fs::write(p.join("AGENTS.md"), b"\xef\xbb\xbfOld A\r\n").unwrap();
    std::fs::write(p.join("CLAUDE.md"), b"Old B\r\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&s, std::fs::Permissions::from_mode(0o700)).unwrap();
        std::fs::set_permissions(p.join("AGENTS.md"), std::fs::Permissions::from_mode(0o640))
            .unwrap();
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        use windows_sys::Win32::Storage::FileSystem::{READ_CONTROL, WRITE_DAC};
        let file = std::fs::OpenOptions::new()
            .access_mode(READ_CONTROL | WRITE_DAC)
            .open(p.join("AGENTS.md"))
            .unwrap();
        graphhelm_sealed_key_provider::protect_owner_only(&file).unwrap();
    }
}
fn child(root: &Path, mode: &str, point: usize) -> std::process::ExitStatus {
    Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "apply::tests::process_child", "--nocapture"])
        .env("GH_ADOPTION_TEST_ROOT", root)
        .env("GH_ADOPTION_TEST_MODE", mode)
        .env("GH_ADOPTION_TEST_POINT", point.to_string())
        .status()
        .unwrap()
}
#[test]
fn process_child() {
    let Ok(root) = std::env::var("GH_ADOPTION_TEST_ROOT") else {
        return;
    };
    let (p, h, s) = dirs(Path::new(&root));
    let plan = fixture_plan(Path::new(&root));
    let accepted = plan["digest"].as_str().unwrap();
    let mode = std::env::var("GH_ADOPTION_TEST_MODE").unwrap();
    if mode == "contend" {
        assert_eq!(
            apply(&p, &h, &s, &plan, accepted).unwrap_err().reason,
            AdoptionReason::Busy
        );
        return;
    }
    #[cfg(windows)]
    if mode == "detach-crash" || mode == "restore-detach-crash" {
        let mut stop = |b| {
            if matches!(
                b,
                Boundary::AfterApplyDetach | Boundary::AfterCompensationDetach
            ) {
                std::process::exit(71);
            }
            Ok(())
        };
        if mode == "detach-crash" {
            apply_with_hook(&p, &h, &s, &plan, accepted, &mut stop).unwrap();
        } else {
            recover_engine(&s, &digest(accepted.as_bytes()), &mut stop).unwrap();
        }
        return;
    }
    if mode == "recover-crash" {
        let stop: usize = std::env::var("GH_ADOPTION_TEST_POINT")
            .unwrap()
            .parse()
            .unwrap();
        let mut n = 0;
        recover_engine(&s, &digest(accepted.as_bytes()), &mut |_| {
            n += 1;
            if n == stop {
                std::process::exit(71);
            }
            Ok(())
        })
        .unwrap();
        return;
    }
    if mode == "capture-crash" {
        apply_with_hook(&p, &h, &s, &plan, accepted, &mut |b| {
            if b == Boundary::AfterApplyCapture {
                std::process::exit(71);
            }
            Ok(())
        })
        .unwrap();
        return;
    }
    if mode == "recover" {
        recover(&s, &digest(accepted.as_bytes())).unwrap();
        return;
    }
    let stop: usize = std::env::var("GH_ADOPTION_TEST_POINT")
        .unwrap()
        .parse()
        .unwrap();
    let mut n = 0;
    let result = apply_with_hook(&p, &h, &s, &plan, accepted, &mut |boundary| {
        if mode == "initial-gap" && boundary == Boundary::BetweenInitialWrites {
            std::process::exit(71)
        }
        if !matches!(
            boundary,
            Boundary::BeforeJournal
                | Boundary::AfterJournal
                | Boundary::BeforePublish
                | Boundary::AfterPublish
        ) {
            return Ok(());
        }
        n += 1;
        if n == stop {
            std::process::exit(71)
        }
        Ok(())
    });
    result.unwrap();
}
#[test]
fn every_journal_and_publication_boundary_recovers_in_a_fresh_process() {
    for point in 1..=apply_boundary_count() {
        let root = tempfile::tempdir().unwrap();
        fixture(root.path());
        let (p, _, s) = dirs(root.path());
        let access_before = ["AGENTS.md", "CLAUDE.md"]
            .map(|name| storage::access(&std::fs::File::open(p.join(name)).unwrap()).unwrap());
        assert_eq!(
            child(root.path(), "apply", point).code(),
            Some(71),
            "boundary {point} was not exercised"
        );
        let id = digest(
            fixture_plan(root.path())["digest"]
                .as_str()
                .unwrap()
                .as_bytes(),
        );
        if !s.join("journals").join(format!("{id}.json")).exists() {
            assert_eq!(point, 1);
        } else {
            assert!(
                child(root.path(), "recover", 0).success(),
                "recover boundary {point}"
            );
        }
        let committed = point == apply_boundary_count();
        assert_eq!(
            std::fs::read(p.join("AGENTS.md")).unwrap(),
            if committed {
                b"New A\n".as_slice()
            } else {
                b"\xef\xbb\xbfOld A\r\n".as_slice()
            },
            "boundary {point}"
        );
        assert_eq!(
            std::fs::read(p.join("CLAUDE.md")).unwrap(),
            if committed {
                b"New B\n".as_slice()
            } else {
                b"Old B\r\n".as_slice()
            },
            "boundary {point}"
        );
        for (name, before) in ["AGENTS.md", "CLAUDE.md"].into_iter().zip(access_before) {
            assert_eq!(
                storage::access(&std::fs::File::open(p.join(name)).unwrap()).unwrap(),
                before,
                "access metadata for {name} at boundary {point}"
            );
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(p.join("AGENTS.md"))
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o640
            );
        }
        #[cfg(windows)]
        {
            let file = std::fs::File::open(p.join("AGENTS.md")).unwrap();
            graphhelm_sealed_key_provider::verify_owner_only(&file).unwrap();
        }
    }
}
#[test]
fn failure_after_backup_compensates_without_replacing_original_baseline() {
    let root = tempfile::tempdir().unwrap();
    fixture(root.path());
    let (p, h, s) = dirs(root.path());
    let plan = fixture_plan(root.path());
    let mut publications = 0;
    let result = apply_with_hook(
        &p,
        &h,
        &s,
        &plan,
        plan["digest"].as_str().unwrap(),
        &mut |b| {
            if b == Boundary::AfterPublish {
                publications += 1;
                if publications == 1 {
                    return Err(AdoptionError {
                        reason: AdoptionReason::RecoveryRequired,
                    });
                }
            }
            Ok(())
        },
    );
    assert!(result.is_err());
    assert_eq!(
        std::fs::read(p.join("AGENTS.md")).unwrap(),
        b"\xef\xbb\xbfOld A\r\n"
    );
    assert_eq!(std::fs::read(p.join("CLAUDE.md")).unwrap(), b"Old B\r\n");
    assert!(s.join("original.json").exists());
}
#[test]
fn unrelated_writer_is_kept_and_recovery_refuses() {
    let root = tempfile::tempdir().unwrap();
    fixture(root.path());
    let (p, h, s) = dirs(root.path());
    let plan = fixture_plan(root.path());
    let result = apply_with_hook(
        &p,
        &h,
        &s,
        &plan,
        plan["digest"].as_str().unwrap(),
        &mut |b| {
            if b == Boundary::AfterPublish {
                std::fs::write(p.join("AGENTS.md"), b"other writer").unwrap();
                return Err(AdoptionError {
                    reason: AdoptionReason::RecoveryRequired,
                });
            }
            Ok(())
        },
    );
    assert_eq!(result.unwrap_err().reason, AdoptionReason::RecoveryRequired);
    assert_eq!(std::fs::read(p.join("AGENTS.md")).unwrap(), b"other writer");
    assert!(recover(&s, &digest(plan["digest"].as_str().unwrap().as_bytes())).is_err());
}

#[test]
fn second_process_gets_busy_before_any_source_write() {
    let root = tempfile::tempdir().unwrap();
    fixture(root.path());
    let (p, h, s) = dirs(root.path());
    let plan = fixture_plan(root.path());
    let mut checked = false;
    let result = apply_with_hook(
        &p,
        &h,
        &s,
        &plan,
        plan["digest"].as_str().unwrap(),
        &mut |boundary| {
            if boundary == Boundary::BeforePublish && !checked {
                assert!(child(root.path(), "contend", 0).success());
                checked = true;
            }
            Ok(())
        },
    );
    result.unwrap();
    assert!(checked);
}
#[test]
fn valid_json_corrupt_intent_refuses_and_retains_published_evidence() {
    let root = tempfile::tempdir().unwrap();
    fixture(root.path());
    let (p, _, s) = dirs(root.path());
    assert_eq!(
        child(root.path(), "apply", first_published_boundary()).code(),
        Some(71)
    );
    let id = digest(
        fixture_plan(root.path())["digest"]
            .as_str()
            .unwrap()
            .as_bytes(),
    );
    let path = s.join("journals").join(format!("{id}.json"));
    let mut value: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    value["record"]["entries"][0]["path"] = json!("CLAUDE.md");
    std::fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
    assert!(recover(&s, &id).is_err());
    assert_eq!(std::fs::read(p.join("AGENTS.md")).unwrap(), b"New A\n");
    assert!(path.exists());
}
#[test]
fn failure_before_publication_after_backup_leaves_all_originals() {
    let root = tempfile::tempdir().unwrap();
    fixture(root.path());
    let (p, h, s) = dirs(root.path());
    let plan = fixture_plan(root.path());
    let mut hit = false;
    let result = apply_with_hook(
        &p,
        &h,
        &s,
        &plan,
        plan["digest"].as_str().unwrap(),
        &mut |b| {
            if b == Boundary::BeforePublish {
                hit = true;
                return Err(storage::failed());
            }
            Ok(())
        },
    );
    assert!(hit);
    assert!(result.is_err());
    assert_eq!(
        std::fs::read(p.join("AGENTS.md")).unwrap(),
        b"\xef\xbb\xbfOld A\r\n"
    );
    assert_eq!(std::fs::read(p.join("CLAUDE.md")).unwrap(), b"Old B\r\n");
}
#[test]
fn incomplete_transaction_blocks_a_different_plan_before_backup_or_publication() {
    let root = tempfile::tempdir().unwrap();
    fixture(root.path());
    let (p, h, s) = dirs(root.path());
    assert_eq!(
        child(root.path(), "apply", first_published_boundary()).code(),
        Some(71)
    );
    let original = std::fs::read(s.join("original.json")).unwrap();
    let mut next = fixture_plan(root.path());
    next["spec"]["operations"][0]["beforeDigest"] = json!(digest(b"New A\n"));
    next["spec"]["operations"][0]["after"] = json!("third method\n");
    next["spec"]["operations"][0]["afterDigest"] = json!(digest(b"third method\n"));
    next.as_object_mut().unwrap().remove("digest");
    next["digest"] = json!(format!(
        "sha256:{}",
        digest(&serde_json::to_vec(&next).unwrap())
    ));
    assert_eq!(
        apply(&p, &h, &s, &next, next["digest"].as_str().unwrap())
            .unwrap_err()
            .reason,
        AdoptionReason::RecoveryRequired
    );
    assert_eq!(std::fs::read(p.join("AGENTS.md")).unwrap(), b"New A\n");
    assert_eq!(std::fs::read(s.join("original.json")).unwrap(), original);
}

#[test]
fn identical_byte_source_swap_after_preflight_is_refused() {
    let root = tempfile::tempdir().unwrap();
    fixture(root.path());
    let (p, h, s) = dirs(root.path());
    let plan = fixture_plan(root.path());
    let mut hit = false;
    let result = apply_with_hook(
        &p,
        &h,
        &s,
        &plan,
        plan["digest"].as_str().unwrap(),
        &mut |b| {
            if b == Boundary::BeforePublish && !hit {
                hit = true;
                let path = p.join("CLAUDE.md");
                std::fs::rename(&path, p.join("moved.md")).unwrap();
                std::fs::write(&path, b"Old B\r\n").unwrap();
            }
            Ok(())
        },
    );
    assert!(result.is_err());
    assert_eq!(
        std::fs::read(p.join("AGENTS.md")).unwrap(),
        b"\xef\xbb\xbfOld A\r\n"
    );
    assert_eq!(std::fs::read(p.join("CLAUDE.md")).unwrap(), b"Old B\r\n");
}
#[test]
fn swapped_journal_directory_refuses_before_source_publication() {
    let root = tempfile::tempdir().unwrap();
    fixture(root.path());
    let (p, h, s) = dirs(root.path());
    let plan = fixture_plan(root.path());
    let mut syncs = 0;
    let result = apply_with_hook(
        &p,
        &h,
        &s,
        &plan,
        plan["digest"].as_str().unwrap(),
        &mut |b| {
            if b == Boundary::BeforeJournal {
                syncs += 1;
                if syncs == 2 {
                    std::fs::rename(s.join("journals"), s.join("moved-journals")).unwrap();
                    std::fs::create_dir(s.join("journals")).unwrap();
                }
            }
            Ok(())
        },
    );
    assert!(result.is_err());
    assert_eq!(
        std::fs::read(p.join("AGENTS.md")).unwrap(),
        b"\xef\xbb\xbfOld A\r\n"
    );
}

#[test]
fn interrupted_initial_registration_keeps_a_recoverable_journal_before_active() {
    let root = tempfile::tempdir().unwrap();
    fixture(root.path());
    let (_, _, state) = dirs(root.path());
    assert_eq!(
        child(root.path(), "initial-gap", usize::MAX).code(),
        Some(71)
    );
    let plan = fixture_plan(root.path());
    let id = digest(plan["digest"].as_str().unwrap().as_bytes());
    assert!(state.join("journals").join(format!("{id}.json")).exists());
    assert!(!state.join("active.json").exists());
    assert!(child(root.path(), "recover", 0).success());
}
#[test]
fn initial_registration_io_failure_does_not_leave_a_dangling_active_pointer() {
    let root = tempfile::tempdir().unwrap();
    fixture(root.path());
    let (p, h, s) = dirs(root.path());
    let plan = fixture_plan(root.path());
    let result = apply_with_hook(
        &p,
        &h,
        &s,
        &plan,
        plan["digest"].as_str().unwrap(),
        &mut |boundary| {
            if boundary == Boundary::BetweenInitialWrites {
                return Err(storage::failed());
            }
            Ok(())
        },
    );
    assert!(result.is_err());
    let id = digest(plan["digest"].as_str().unwrap().as_bytes());
    assert!(s.join("journals").join(format!("{id}.json")).exists());
    assert!(!s.join("active.json").exists());
    assert!(child(root.path(), "recover", 0).success());
}
fn contains_writer_bytes(path: &Path) -> bool {
    std::fs::read_dir(path).unwrap().any(|entry| {
        let path = entry.unwrap().path();
        if path.is_dir() {
            contains_writer_bytes(&path)
        } else {
            std::fs::read(path).is_ok_and(|bytes| bytes == b"other writer")
        }
    })
}
#[test]
fn unrelated_writers_in_final_apply_and_compensation_windows_are_preserved() {
    for compensating in [false, true] {
        for replacing in [false, true] {
            let root = tempfile::tempdir().unwrap();
            fixture(root.path());
            let (p, h, s) = dirs(root.path());
            let plan = fixture_plan(root.path());
            let mut hit = false;
            let mut wrote = false;
            let result = apply_with_hook(
                &p,
                &h,
                &s,
                &plan,
                plan["digest"].as_str().unwrap(),
                &mut |boundary| {
                    if compensating && boundary == Boundary::AfterPublish {
                        return Err(storage::failed());
                    }
                    let selected = if compensating {
                        Boundary::FinalCompensationPublication
                    } else {
                        Boundary::FinalApplyPublication
                    };
                    if boundary == selected && !hit {
                        hit = true;
                        let destination = p.join("AGENTS.md");
                        let may_write = !replacing
                            || std::fs::rename(&destination, p.join("writer-moved.md")).is_ok();
                        if may_write {
                            wrote = std::fs::write(destination, b"other writer").is_ok();
                        }
                    }
                    Ok(())
                },
            );
            assert!(
                hit,
                "window was missed compensation={compensating} replacing={replacing} result={result:?}"
            );
            if !wrote && !compensating {
                assert!(
                    result.is_ok(),
                    "exclusion refused the writer but publication did not complete: {result:?}"
                );
            }
            if wrote {
                assert!(
                    result.is_err(),
                    "writer was accepted over in compensation={compensating} replacing={replacing}"
                );
                assert!(
                    contains_writer_bytes(&p),
                    "writer bytes were destroyed in compensation={compensating} replacing={replacing}"
                );
            }
        }
    }
}

#[test]
fn every_compensation_boundary_and_unreceipted_capture_recovers_in_a_fresh_process() {
    for point in 0..=compensation_boundary_count() {
        let root = tempfile::tempdir().unwrap();
        fixture(root.path());
        let (p, _, _) = dirs(root.path());
        let before = ["AGENTS.md", "CLAUDE.md"]
            .map(|name| storage::access(&std::fs::File::open(p.join(name)).unwrap()).unwrap());
        if point == 0 {
            assert_eq!(child(root.path(), "capture-crash", 0).code(), Some(71));
        } else {
            assert_eq!(
                child(root.path(), "apply", all_published_boundary()).code(),
                Some(71)
            );
            assert_eq!(
                child(root.path(), "recover-crash", point).code(),
                Some(71),
                "compensation boundary {point} was not reached"
            );
        }
        assert!(
            child(root.path(), "recover", 0).success(),
            "compensation boundary {point} did not recover"
        );
        assert_eq!(
            std::fs::read(p.join("AGENTS.md")).unwrap(),
            b"\xef\xbb\xbfOld A\r\n"
        );
        assert_eq!(std::fs::read(p.join("CLAUDE.md")).unwrap(), b"Old B\r\n");
        for (name, expected) in ["AGENTS.md", "CLAUDE.md"].into_iter().zip(before) {
            assert_eq!(
                storage::access(&std::fs::File::open(p.join(name)).unwrap()).unwrap(),
                expected
            );
        }
    }
}
#[test]
fn orphan_initial_journal_refuses_apply_retry_until_reconciled_and_does_not_block_a_new_plan() {
    let root = tempfile::tempdir().unwrap();
    fixture(root.path());
    let (p, h, s) = dirs(root.path());
    assert_eq!(child(root.path(), "initial-gap", 0).code(), Some(71));
    let mut plan = fixture_plan(root.path());
    let accepted = plan["digest"].as_str().unwrap().to_owned();
    assert_eq!(
        apply(&p, &h, &s, &plan, &accepted).unwrap_err().reason,
        AdoptionReason::RecoveryRequired
    );
    assert!(recover(&s, &digest(accepted.as_bytes())).is_ok());
    plan["id"] = json!("fresh-owner-review");
    plan.as_object_mut().unwrap().remove("digest");
    plan["digest"] = json!(format!(
        "sha256:{}",
        digest(&serde_json::to_vec(&plan).unwrap())
    ));
    let result = apply(&p, &h, &s, &plan, plan["digest"].as_str().unwrap());
    assert!(
        result.is_ok(),
        "fresh reviewed plan after orphan recovery: {result:?}"
    );
}

#[cfg(target_os = "linux")]
#[test]
fn releasing_authority_does_not_wait_for_an_unrelated_fork_to_exec() {
    // A concurrent spawn inherits CLOEXEC descriptors until its exec. Hold that exact window
    // with a pipe; no timing or retries are needed to observe whether authority was released.
    use std::os::fd::{AsRawFd, FromRawFd};
    let temp = tempfile::tempdir().unwrap();
    let root = Root::open(temp.path(), false).unwrap();
    let lock = root.lock(".authority.lock").unwrap();
    let mut fds = [0; 2];
    assert_eq!(unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC) }, 0);
    let reader = unsafe { std::fs::File::from_raw_fd(fds[0]) };
    let writer = unsafe { std::fs::File::from_raw_fd(fds[1]) };
    let pid = unsafe { libc::fork() };
    assert!(pid >= 0);
    if pid == 0 {
        // Only async-signal-safe syscalls between fork and _exit.
        unsafe {
            libc::close(writer.as_raw_fd());
            let mut byte = 0_u8;
            libc::read(reader.as_raw_fd(), (&raw mut byte).cast(), 1);
            libc::_exit(0);
        }
    }
    drop(lock);
    let acquired = root.lock(".authority.lock");
    drop(writer);
    let mut status = 0;
    assert_eq!(unsafe { libc::waitpid(pid, &mut status, 0) }, pid);
    assert!(
        acquired.is_ok(),
        "released authority remains owned by an unrelated child"
    );
}

#[test]
fn final_destination_link_swap_never_changes_the_outside_target() {
    let root = tempfile::tempdir().unwrap();
    fixture(root.path());
    let (p, h, s) = dirs(root.path());
    let outside = root.path().join("outside.md");
    std::fs::write(&outside, b"outside data").unwrap();
    let plan = fixture_plan(root.path());
    let mut hit = false;
    let mut swapped = false;
    let result = apply_with_hook(
        &p,
        &h,
        &s,
        &plan,
        plan["digest"].as_str().unwrap(),
        &mut |boundary| {
            if boundary == Boundary::FinalApplyPublication && !hit {
                hit = true;
                let target = p.join("AGENTS.md");
                if std::fs::rename(&target, p.join("moved.md")).is_err() {
                    return Ok(());
                }
                swapped = true;
                #[cfg(unix)]
                std::os::unix::fs::symlink(&outside, target).unwrap();
                #[cfg(windows)]
                std::os::windows::fs::symlink_file(&outside, target).unwrap();
            }
            Ok(())
        },
    );
    assert!(hit);
    if swapped {
        assert!(result.is_err());
    } else {
        assert!(result.is_ok());
    }
    assert_eq!(std::fs::read(outside).unwrap(), b"outside data");
}

#[cfg(windows)]
#[test]
fn writer_creation_in_the_detached_gap_is_never_overwritten() {
    for compensating in [false, true] {
        let root = tempfile::tempdir().unwrap();
        fixture(root.path());
        let (p, h, s) = dirs(root.path());
        let plan = fixture_plan(root.path());
        let mut hit = false;
        let result = apply_with_hook(
            &p,
            &h,
            &s,
            &plan,
            plan["digest"].as_str().unwrap(),
            &mut |b| {
                if compensating && b == Boundary::AfterPublish {
                    return Err(storage::failed());
                }
                let target = if compensating {
                    Boundary::AfterCompensationDetach
                } else {
                    Boundary::AfterApplyDetach
                };
                if b == target && !hit {
                    hit = true;
                    assert!(!p.join("AGENTS.md").exists());
                    std::fs::write(p.join("AGENTS.md"), b"other writer").unwrap();
                }
                Ok(())
            },
        );
        assert!(hit, "detached publication boundary was not exercised");
        assert_eq!(result.unwrap_err().reason, AdoptionReason::RecoveryRequired);
        assert_eq!(std::fs::read(p.join("AGENTS.md")).unwrap(), b"other writer");
        assert!(recover(&s, &digest(plan["digest"].as_str().unwrap().as_bytes())).is_err());
    }
}

fn apply_boundary_count() -> usize {
    if cfg!(windows) { 24 } else { 20 }
}
fn first_published_boundary() -> usize {
    if cfg!(windows) { 12 } else { 10 }
}
fn all_published_boundary() -> usize {
    if cfg!(windows) { 22 } else { 18 }
}
fn compensation_boundary_count() -> usize {
    if cfg!(windows) { 22 } else { 16 }
}

#[cfg(windows)]
#[test]
fn unjournaled_detachment_is_reconciled_before_fresh_process_compensation() {
    for restoring in [false, true] {
        let root = tempfile::tempdir().unwrap();
        fixture(root.path());
        let (p, _, _) = dirs(root.path());
        if restoring {
            assert_eq!(
                child(root.path(), "apply", all_published_boundary()).code(),
                Some(71)
            );
        }
        assert_eq!(
            child(
                root.path(),
                if restoring {
                    "restore-detach-crash"
                } else {
                    "detach-crash"
                },
                0
            )
            .code(),
            Some(71)
        );
        assert!(
            !p.join(if restoring { "CLAUDE.md" } else { "AGENTS.md" })
                .exists()
        );
        assert!(child(root.path(), "recover", 0).success());
        assert_eq!(
            std::fs::read(p.join("AGENTS.md")).unwrap(),
            b"\xef\xbb\xbfOld A\r\n"
        );
        assert_eq!(std::fs::read(p.join("CLAUDE.md")).unwrap(), b"Old B\r\n");
    }
}
