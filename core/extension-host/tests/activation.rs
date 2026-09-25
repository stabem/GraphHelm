//! Activation state and its exclusive claim (#212), G5.

use std::fs;
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::process::{Command, ExitStatus, Stdio};

use graphhelm_extension_host::{ActivationClaim, ActivationRecord, ClaimRefusal, ClaimStatus};

const CHILD_MODE: &str = "GRAPHHELM_ACTIVATION_CLAIM_CHILD_MODE";
const CHILD_ROOT: &str = "GRAPHHELM_ACTIVATION_CLAIM_CHILD_ROOT";
const CLAIM_SLOT_BYTES: usize = 1024;

fn latest_persisted_frame(path: &std::path::Path) -> serde_json::Value {
    let bytes = fs::read(path).expect("claim metadata");
    [0, CLAIM_SLOT_BYTES]
        .into_iter()
        .filter_map(|offset| {
            let length = u32::from_le_bytes(bytes[offset..offset + 4].try_into().ok()?) as usize;
            serde_json::from_slice::<serde_json::Value>(bytes.get(offset + 4..offset + 4 + length)?)
                .ok()
        })
        .max_by_key(|frame| frame["payload"]["generation"].as_u64())
        .expect("one valid metadata frame")
}

fn spawn_claim_child(root: &std::path::Path, mode: &str) -> std::process::Child {
    Command::new(std::env::current_exe().expect("current test executable"))
        .args(["--exact", "claim_holder_child", "--nocapture"])
        .env(CHILD_MODE, mode)
        .env(CHILD_ROOT, root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("spawn claim child")
}

#[test]
fn claim_holder_child() {
    let Ok(mode) = std::env::var(CHILD_MODE) else {
        return;
    };
    let root = std::path::PathBuf::from(std::env::var_os(CHILD_ROOT).expect("child root"));
    let _claim = ActivationClaim::acquire(&root).expect("child claim");
    println!("GRAPHHELM_CLAIM_READY");
    std::io::stdout().flush().expect("flush ready signal");
    if mode == "crash" {
        std::process::exit(0);
    }
    let mut byte = [0_u8; 1];
    let read = std::io::stdin()
        .read(&mut byte)
        .expect("wait for parent EOF");
    assert_eq!(
        read, 0,
        "the parent must release the child by closing stdin"
    );
}

fn wait_until_ready(child: &mut std::process::Child) -> BufReader<std::process::ChildStdout> {
    let stdout = child.stdout.take().expect("child stdout");
    let mut output = BufReader::new(stdout);
    loop {
        let mut line = String::new();
        assert_ne!(
            output.read_line(&mut line).expect("child output"),
            0,
            "child exited before ready"
        );
        if line.trim() == "GRAPHHELM_CLAIM_READY" {
            return output;
        }
    }
}

fn wait_for_child_exit(child: &mut std::process::Child) -> ExitStatus {
    child.wait().expect("wait for child")
}

fn release_and_wait(child: &mut std::process::Child) -> ExitStatus {
    drop(child.stdin.take());
    wait_for_child_exit(child)
}

#[test]
fn a_live_process_lock_refuses_a_second_claim() {
    let root = tempfile::tempdir().expect("a temp dir");
    let mut child = spawn_claim_child(root.path(), "hold");
    let _child_output = wait_until_ready(&mut child);

    assert_eq!(ActivationClaim::inspect(root.path()), Ok(ClaimStatus::Held));
    assert_eq!(
        ActivationClaim::acquire(root.path()).err(),
        Some(ClaimRefusal::AlreadyHeld)
    );

    assert!(release_and_wait(&mut child).success());
}

#[test]
fn an_os_released_lock_is_recovered_after_a_child_exits_without_drop() {
    let root = tempfile::tempdir().expect("a temp dir");
    let mut child = spawn_claim_child(root.path(), "crash");
    let _child_output = wait_until_ready(&mut child);
    assert!(wait_for_child_exit(&mut child).success());

    let persisted = latest_persisted_frame(&root.path().join("activation.claim"));
    let metadata = &persisted["payload"]["metadata"];
    assert_eq!(metadata["version"], 1);
    assert_eq!(metadata["state"], "held");
    assert!(
        metadata["claimId"]
            .as_str()
            .is_some_and(|id| !id.is_empty())
    );
    assert!(metadata["ownerPid"].as_u64().is_some_and(|pid| pid > 0));

    assert_eq!(
        ActivationClaim::inspect(root.path()),
        Ok(ClaimStatus::Stale)
    );
    ActivationClaim::acquire(root.path()).expect("the orphaned claim must be recoverable");
}

#[test]
fn persisted_metadata_distinguishes_held_stale_and_free_claims() {
    let root = tempfile::tempdir().expect("a temp dir");
    let claim = ActivationClaim::acquire(root.path()).expect("claim");
    let path = claim.path().to_owned();

    assert_eq!(ActivationClaim::inspect(root.path()), Ok(ClaimStatus::Held));

    drop(claim);
    let free = latest_persisted_frame(&path);
    let metadata = &free["payload"]["metadata"];
    assert_eq!(metadata["version"], 1);
    assert_eq!(metadata["state"], "free");
    assert_eq!(ActivationClaim::inspect(root.path()), Ok(ClaimStatus::Free));

    let mut child = spawn_claim_child(root.path(), "crash");
    let _child_output = wait_until_ready(&mut child);
    assert!(wait_for_child_exit(&mut child).success());
    assert_eq!(
        ActivationClaim::inspect(root.path()),
        Ok(ClaimStatus::Stale)
    );
}

#[test]
fn empty_and_malformed_legacy_claims_are_not_stolen() {
    for bytes in [b"".as_slice(), b"not-json".as_slice()] {
        let root = tempfile::tempdir().expect("a temp dir");
        let path = root.path().join("activation.claim");
        fs::write(&path, bytes).expect("legacy claim");

        assert_eq!(
            ActivationClaim::acquire(root.path()).err(),
            Some(ClaimRefusal::LegacyUnverifiable)
        );
        assert_eq!(fs::read(path).unwrap(), bytes);
    }
}

#[test]
fn an_interrupted_metadata_slot_write_preserves_the_last_valid_state() {
    let root = tempfile::tempdir().expect("a temp dir");
    let claim = ActivationClaim::acquire(root.path()).expect("claim");
    let path = claim.path().to_owned();
    drop(claim);

    let mut file = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)
        .expect("claim file");
    file.seek(SeekFrom::Start(CLAIM_SLOT_BYTES as u64))
        .expect("inactive metadata slot");
    file.write_all(&[0xff; 64])
        .expect("simulate a torn slot write");
    file.sync_all().expect("persist sabotage");

    assert_eq!(ActivationClaim::inspect(root.path()), Ok(ClaimStatus::Free));
}

#[cfg(unix)]
#[test]
fn replacing_the_named_claim_cannot_create_a_second_live_authority() {
    let root = tempfile::tempdir().expect("a temp dir");
    let first = ActivationClaim::acquire(root.path()).expect("first claim");
    let named = root.path().join("activation.claim");
    let displaced = root.path().join("displaced.claim");
    fs::rename(&named, &displaced).expect("replace named claim");
    fs::copy(&displaced, &named).expect("valid-looking replacement");

    assert_eq!(
        ActivationClaim::acquire(root.path()).err(),
        Some(ClaimRefusal::AlreadyHeld),
        "a replacement inode must not become a second claim authority"
    );

    drop(first);
}

#[cfg(unix)]
#[test]
fn replacing_the_install_root_cannot_create_a_second_live_authority() {
    let parent = tempfile::tempdir().expect("a temp parent");
    let install_root = parent.path().join("install");
    let displaced_root = parent.path().join("displaced-install");
    fs::create_dir(&install_root).expect("install root");
    let first = ActivationClaim::acquire(&install_root).expect("first claim");

    fs::rename(&install_root, &displaced_root).expect("displace the complete install root");
    fs::create_dir(&install_root).expect("replacement install root");
    fs::copy(
        displaced_root.join("activation.claim"),
        install_root.join("activation.claim"),
    )
    .expect("valid-looking replacement claim");

    assert_eq!(
        ActivationClaim::acquire(&install_root).err(),
        Some(ClaimRefusal::AlreadyHeld),
        "a replacement install-root inode must not become a second claim authority"
    );

    drop(first);
}

#[cfg(target_os = "linux")]
#[test]
fn a_public_dev_null_lock_cannot_squat_the_install_root_authority() {
    use sha2::{Digest, Sha256};
    use std::os::fd::AsRawFd;
    use std::os::unix::ffi::OsStrExt;

    let root = tempfile::tempdir().expect("install root");
    let digest = Sha256::digest(root.path().as_os_str().as_bytes());
    let offset = i64::from_le_bytes(digest[..8].try_into().unwrap()) & i64::MAX;
    let public = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open("/dev/null")
        .expect("public namespace file");
    let lock = libc::flock {
        l_type: libc::F_WRLCK as _,
        l_whence: libc::SEEK_SET as _,
        l_start: offset,
        l_len: 1,
        l_pid: 0,
    };
    assert_eq!(
        unsafe { libc::fcntl(public.as_raw_fd(), libc::F_OFD_SETLK, &lock) },
        0,
        "the regression setup must hold the old public authority"
    );

    ActivationClaim::acquire(root.path())
        .expect("a public /dev/null lock must not control GraphHelm activation");
}

#[cfg(all(unix, not(target_os = "linux")))]
#[test]
fn unproven_non_linux_unix_authority_is_refused_explicitly() {
    let root = tempfile::tempdir().expect("install root");

    assert_eq!(
        ActivationClaim::acquire(root.path()).err(),
        Some(ClaimRefusal::UnsupportedPlatform)
    );
    assert_eq!(
        ActivationClaim::inspect(root.path()).err(),
        Some(ClaimRefusal::UnsupportedPlatform)
    );
    assert!(!root.path().join("activation.claim").exists());
}

#[cfg(windows)]
#[test]
fn retained_windows_ancestor_handles_deny_tree_replacement() {
    let parent = tempfile::tempdir().expect("parent");
    let ancestor = parent.path().join("ancestor");
    let install = ancestor.join("install");
    let displaced = parent.path().join("displaced-ancestor");
    fs::create_dir_all(&install).expect("install root");
    let claim = ActivationClaim::acquire(&install).expect("claim");

    assert!(
        fs::rename(&ancestor, &displaced).is_err(),
        "retained no-share-delete ancestor handles must deny path replacement"
    );
    drop(claim);
    fs::rename(&ancestor, &displaced).expect("release restores ordinary rename behavior");
}

#[cfg(any(target_os = "linux", windows))]
#[test]
fn a_multiply_linked_claim_is_refused_without_touching_the_other_name() {
    let root = tempfile::tempdir().expect("install root");
    let sentinel = root.path().join("sentinel");
    let claim = root.path().join("activation.claim");
    fs::write(&sentinel, b"sentinel").expect("sentinel");
    fs::hard_link(&sentinel, &claim).expect("attacker hard link");

    assert_eq!(
        ActivationClaim::inspect(root.path()).err(),
        Some(ClaimRefusal::UnsafeClaimPath)
    );
    assert_eq!(
        ActivationClaim::acquire(root.path()).err(),
        Some(ClaimRefusal::UnsafeClaimPath)
    );
    assert_eq!(fs::read(sentinel).unwrap(), b"sentinel");
}

#[cfg(unix)]
fn symlink_file(target: &std::path::Path, link: &std::path::Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(target, link)
}

#[cfg(unix)]
#[test]
fn a_symlink_claim_is_refused_without_touching_its_target() {
    let root = tempfile::tempdir().expect("a temp dir");
    let target = root.path().join("target");
    let claim_path = root.path().join("activation.claim");
    fs::write(&target, b"sentinel").expect("target");
    symlink_file(&target, &claim_path).expect("create claim link");

    assert_eq!(
        ActivationClaim::inspect(root.path()).err(),
        Some(ClaimRefusal::UnsafeClaimPath)
    );
    assert_eq!(
        ActivationClaim::acquire(root.path()).err(),
        Some(ClaimRefusal::UnsafeClaimPath)
    );
    assert_eq!(fs::read(target).unwrap(), b"sentinel");
}

#[cfg(unix)]
#[test]
fn a_symlinked_ancestor_is_refused_before_the_claim_is_opened() {
    let root = tempfile::tempdir().expect("a temp dir");
    let real_parent = root.path().join("real-parent");
    let real_install = real_parent.join("install");
    let alias = root.path().join("alias-parent");
    fs::create_dir_all(&real_install).expect("real install root");
    std::os::unix::fs::symlink(&real_parent, &alias).expect("ancestor link");

    assert_eq!(
        ActivationClaim::acquire(&alias.join("install")).err(),
        Some(ClaimRefusal::UnsafeClaimPath)
    );
    assert!(!real_install.join("activation.claim").exists());
}

#[cfg(windows)]
#[test]
fn a_reparse_point_claim_is_refused_without_touching_its_target() {
    let root = tempfile::tempdir().expect("a temp dir");
    let target = root.path().join("target");
    let sentinel = target.join("sentinel");
    let claim_path = root.path().join("activation.claim");
    fs::create_dir(&target).expect("target directory");
    fs::write(&sentinel, b"sentinel").expect("target");
    let status = Command::new("cmd")
        .args(["/C", "mklink", "/J"])
        .arg(&claim_path)
        .arg(&target)
        .status()
        .expect("create claim junction");
    assert!(status.success(), "claim junction creation failed");

    assert_eq!(
        ActivationClaim::inspect(root.path()).err(),
        Some(ClaimRefusal::UnsafeClaimPath)
    );
    assert_eq!(
        ActivationClaim::acquire(root.path()).err(),
        Some(ClaimRefusal::UnsafeClaimPath)
    );
    assert_eq!(fs::read(sentinel).unwrap(), b"sentinel");
}

#[cfg(windows)]
#[test]
fn a_reparse_point_ancestor_is_refused_before_the_claim_is_opened() {
    let root = tempfile::tempdir().expect("a temp dir");
    let real_parent = root.path().join("real-parent");
    let real_install = real_parent.join("install");
    let alias = root.path().join("alias-parent");
    fs::create_dir_all(&real_install).expect("real install root");
    let status = Command::new("cmd")
        .args(["/C", "mklink", "/J"])
        .arg(&alias)
        .arg(&real_parent)
        .status()
        .expect("create ancestor junction");
    assert!(status.success(), "ancestor junction creation failed");

    assert_eq!(
        ActivationClaim::acquire(&alias.join("install")).err(),
        Some(ClaimRefusal::UnsafeClaimPath)
    );
    assert!(!real_install.join("activation.claim").exists());
}

/// The production change this catches: `File::create` where `create_new` was meant.
///
/// Both are "make the file". Only one of them FAILS when the file is already there, and a
/// create-if-absent that succeeds twice is not a claim -- it is two writers each believing they
/// hold the switch. The house rule (ED-22) names the same distinction on the PowerShell side:
/// `CreateNew`, never `New-Item`.
#[test]
fn a_second_claim_is_refused_while_the_first_is_held_and_granted_after_it_is_released() {
    let root = tempfile::tempdir().expect("a temp dir");

    let first = ActivationClaim::acquire(root.path()).expect("the first claim must be granted");

    // Landmark: the claim is really held. Without this, the refusal below could be a claim path
    // that fails for everyone, and "the second one failed" would prove nothing.
    assert!(
        first.path().exists(),
        "HARNESS-BROKE: the first claim left nothing on disk, so a second refusal would say \
         nothing about exclusivity"
    );

    let second = ActivationClaim::acquire(root.path());
    assert_eq!(
        second.err(),
        Some(ClaimRefusal::AlreadyHeld),
        "a second claim was granted while the first was held"
    );

    // The other half of the pair: a refusal that never lifts is a broken claim, not a safe one.
    drop(first);
    let third = ActivationClaim::acquire(root.path());
    assert!(
        third.is_ok(),
        "HARNESS-BROKE: the claim was never released, so the refusal above may be permanent \
         breakage rather than exclusivity"
    );
}

/// G3 of the blueprint: a failed activation leaves the PREVIOUS version active.
///
/// Every prefix of the durable write sequence is checked, not one hand-picked crash point. A crash
/// test that chooses its own boundary tests the boundary the author thought of.
///
/// WHAT THIS DOES NOT PROVE, stated here because the test's NAME claims more than its reach: this
/// asserts a property of the ORDER declared by `activation_steps()`, which is a Vec. It does not
/// observe a single write to disk, and it cannot: no filesystem is touched anywhere in this test.
/// A caller that performs the steps in a different order than the one declared here would violate
/// the invariant with this guard still green. What the guard holds is that the DECLARED order is a
/// safe one; what it does not hold is that anyone follows it. The site that performs the writes
/// inherits that obligation, and no comment substitutes for the guard that will live there.
///
/// The production change this catches: retiring the previous version before recording the new one.
/// Both orders complete identically; they differ only in what a crash leaves behind, and one of
/// them leaves a machine with NO active version -- which is not a rollback, it is an outage.
#[test]
fn no_crash_point_leaves_the_machine_without_an_active_version() {
    let steps = graphhelm_extension_host::activation_steps();

    // Landmarks: the sequence must actually do both things, or the invariant below holds
    // vacuously over a sequence that never retires anything.
    assert!(
        steps.contains(&graphhelm_extension_host::ActivationStep::NewVersionRecorded),
        "HARNESS-BROKE: the sequence never records a new version"
    );
    assert!(
        steps.contains(&graphhelm_extension_host::ActivationStep::PreviousVersionRetired),
        "HARNESS-BROKE: the sequence never retires the previous version"
    );

    for crash_after in 0..=steps.len() {
        let survived = &steps[..crash_after];
        let retired =
            survived.contains(&graphhelm_extension_host::ActivationStep::PreviousVersionRetired);
        let recorded =
            survived.contains(&graphhelm_extension_host::ActivationStep::NewVersionRecorded);

        assert!(
            recorded || !retired,
            "a crash after {crash_after} step(s) left no active version at all: the previous one \
             was retired before the new one was recorded. The survivor reads as {survived:?}"
        );
    }
}

/// Built as a literal because `ValidatedExtensionPackage` has no constructor. That couples this
/// test to the struct's field list -- #325 added `contributions` upstream and this helper stopped
/// compiling, which is the coupling announcing itself. A compile error is the right failure mode
/// for it: it names the field and the line, and it cannot be mistaken for a behaviour change.
fn validated(id: &str, version: &str, digest: &str) -> graphhelm_schema::ValidatedExtensionPackage {
    graphhelm_schema::ValidatedExtensionPackage {
        id: id.to_owned(),
        version: version.to_owned(),
        contribution_count: 1,
        package_digest: digest.to_owned(),
        contributions: Vec::new(),
    }
}

/// G2 of the blueprint: validation alone grants no runtime authority.
///
/// The record is minted from ONE validated package and authorizes that one. A second package that
/// also validates is still not the one that was activated.
///
/// The production change this catches: authorizing by IDENTITY. Comparing the extension id is the
/// natural shortcut -- it reads as "is this the same extension?" -- and every version of an
/// extension shares its id, so a record minted for v1 would authorize v2. Validation says
/// well-formed; identity says same family; neither says "this is the artifact that was activated".
#[test]
fn a_record_authorizes_the_package_it_was_activated_from_and_no_other() {
    let root = tempfile::tempdir().expect("a temp dir");
    let claim = ActivationClaim::acquire(root.path()).expect("the claim must be granted");

    let activated = validated("graphhelm-example", "1.0.0", "sha256:aaaa");
    let record = ActivationRecord::activate(&claim, &activated, root.path().join("graphhelm"));

    // Landmark: the record authorizes its own package. Without this the refusal below could be a
    // record that authorizes nothing at all, which would be safe and useless.
    assert!(
        record.authorizes(&activated),
        "HARNESS-BROKE: the record does not authorize the package it was activated from"
    );

    // Same extension, same id, different artifact. It validates; it was not activated.
    let other_version = validated("graphhelm-example", "1.1.0", "sha256:bbbb");
    assert!(
        !record.authorizes(&other_version),
        "a record minted for one package authorized a different one that merely validates"
    );
}

/// A record that was never activated authorizes NOTHING, including a package whose digest is also
/// empty.
///
/// The constructors that build a record from a path alone -- the ones discovery uses -- leave the
/// digest empty and are reachable without holding a claim. The emptiness check in `authorizes` was
/// written for exactly that and never exercised, so deleting it left every test green while an
/// unactivated record began authorizing any package that also carried an empty digest.
///
/// The production change this catches: dropping the `is_empty` arm, or comparing with `==` alone.
#[test]
fn a_record_that_was_never_activated_authorizes_nothing() {
    let root = tempfile::tempdir().expect("a temp dir");

    // EVERY constructor that does not take a claim, not just the one the first version reached.
    // These are exactly the records that were never activated, and the list is hand-written because
    // Rust cannot enumerate constructors -- so a constructor added later is invisible here until
    // someone adds it, and that is the known cost of this shape rather than an oversight.
    let unactivated: [(&str, ActivationRecord); 2] = [
        (
            "for_package_without_recorded_executable",
            ActivationRecord::for_package_without_recorded_executable(root.path()),
        ),
        (
            "for_package_with_recorded_executables",
            ActivationRecord::for_package_with_recorded_executables(
                root.path(),
                vec![root.path().join("graphhelm")],
            ),
        ),
    ];

    // Landmark: an activated record DOES authorize, so the refusals below are about this record
    // never having been activated rather than about `authorizes` being broken for everyone.
    let claim = ActivationClaim::acquire(root.path()).expect("the claim must be granted");
    let package = validated("graphhelm-example", "1.0.0", "sha256:aaaa");
    let activated = ActivationRecord::activate(&claim, &package, root.path().join("graphhelm"));
    assert!(
        activated.authorizes(&package),
        "HARNESS-BROKE: an activated record does not authorize its own package"
    );

    // The case the emptiness check exists for: both digests empty. Equality alone says yes here,
    // and the first assertion below cannot reach it -- "" != "sha256:aaaa" holds with or without
    // the guard, so only the empty-against-empty case tests the arm at all.
    let also_empty = validated("graphhelm-example", "1.0.0", "");

    for (constructor, record) in &unactivated {
        assert!(
            !record.authorizes(&package),
            "{constructor} produced a record that authorized a real package"
        );
        assert!(
            !record.authorizes(&also_empty),
            "{constructor} produced a record that authorized a package by matching one empty \
             digest against another"
        );
    }
}

/// Two paths that are ONE directory must contend for one lock.
///
/// Symlink aliases are already refused upstream: the root walk opens every component with
/// `O_NOFOLLOW`, so a link anywhere in the path never reaches the lock at all. The alias channel
/// that survives the walk is a bind mount -- both names are real directories, the walk succeeds
/// under each, and the per-root lock is derived from the root's identity. Before the fix it was
/// derived from a SHA-256 of the PATH BYTES, so each alias computed its own lock file and both
/// acquired: two live activations against one installation, which is the property this mechanism
/// exists to prevent. (#212 blocker, fourth bullet; read against the code by N on #473.)
///
/// The bind mount needs `CAP_SYS_ADMIN`, so the cell is ignored by default and run deliberately
/// in the privileged toolchain container. An `#[ignore]` with a reason is the honest gate here:
/// a test that silently self-skips on EPERM reports the same green as one that measured.
#[cfg(target_os = "linux")]
#[test]
#[ignore = "needs mount privileges (CAP_SYS_ADMIN); run deliberately: cargo test -- --ignored"]
fn a_bind_mount_alias_of_a_held_root_is_refused() {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::MetadataExt;

    let base = tempfile::tempdir().expect("a temp dir");
    let real = base.path().join("real-root");
    let alias = base.path().join("alias-root");
    std::fs::create_dir(&real).expect("the real root creates");
    std::fs::create_dir(&alias).expect("the alias mount point creates");

    let source = CString::new(real.as_os_str().as_bytes()).expect("source path");
    let target = CString::new(alias.as_os_str().as_bytes()).expect("target path");
    // SAFETY: both strings are live NUL-terminated paths; MS_BIND copies no data.
    let mounted = unsafe {
        libc::mount(
            source.as_ptr(),
            target.as_ptr(),
            std::ptr::null(),
            libc::MS_BIND,
            std::ptr::null(),
        )
    };
    assert_eq!(
        mounted,
        0,
        "ARRANGEMENT: the bind mount failed ({:?}) -- this cell needs CAP_SYS_ADMIN and is meant \
         to run in the privileged toolchain container, not on a developer default",
        std::io::Error::last_os_error()
    );

    // CONTROL: the two names really are one directory. Without this, a refusal below could be
    // any refusal at all, and a green would prove nothing about aliasing.
    let real_metadata = std::fs::metadata(&real).expect("the real root stats");
    let alias_metadata = std::fs::metadata(&alias).expect("the alias stats");
    assert_eq!(
        (real_metadata.dev(), real_metadata.ino()),
        (alias_metadata.dev(), alias_metadata.ino()),
        "CONTROL: the bind mount did not produce an alias of the same directory"
    );

    let held = ActivationClaim::acquire(&real).expect("the real path acquires first");

    // FIRST CELL, and it is a control rather than the subject: with the claim file intact, the
    // alias is refused even when the per-root lock diverges, because the claim file is one inode
    // under both names and its flock catches the second acquire. Measured GREEN against the
    // unfixed code -- which is what narrowed the blocker's fourth bullet from "both acquire" to
    // the replacement case below.
    assert_eq!(
        ActivationClaim::acquire(&alias).err(),
        Some(ClaimRefusal::AlreadyHeld),
        "CONTROL: with the claim file intact the inode-anchored claim flock must refuse the \
         alias on its own, with or without the per-root lock"
    );

    // THE SUBJECT: the claim file is removed while the first authority is live. An unlink is an
    // ordinary accident -- an operator cleanup, a sync tool -- and it kills the flock backstop,
    // because the next acquire creates a NEW claim file whose flock domain is a new inode. The
    // only defence left is the per-root lock in /run/user, which exists precisely because it
    // lives OUTSIDE the replaceable subtree. Named by path bytes it diverges across the alias
    // and defends nothing; named by root identity it contends.
    //
    // The same-path variant of this replacement is already pinned by
    // `replacing_the_named_claim_cannot_create_a_second_live_authority`; this cell is the alias
    // variant, which only the lock's NAME decides.
    std::fs::remove_file(real.join("activation.claim"))
        .expect("ARRANGEMENT: the live claim file removes");
    let second = ActivationClaim::acquire(&alias).err();
    drop(held);

    // The mount is unwound before the verdict so a red does not leak a mount into the tempdir
    // teardown; the verdict is asserted from the captured value.
    // SAFETY: target is the same live NUL-terminated path that was mounted above.
    let unmounted = unsafe { libc::umount2(target.as_ptr(), 0) };
    assert_eq!(
        second,
        Some(ClaimRefusal::AlreadyHeld),
        "with the claim file replaced out from under the first authority, a bind-mount alias \
         acquired a SECOND live claim: the per-root lock's identity is the path that was typed, \
         not the directory that was opened"
    );
    assert_eq!(unmounted, 0, "cleanup: the bind mount did not unmount");
}
