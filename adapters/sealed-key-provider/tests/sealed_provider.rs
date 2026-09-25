use std::{
    fs,
    future::Future,
    path::Path,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll, Wake, Waker},
    thread,
};

use graphhelm_events::{
    AuthenticateRequest, KeyError, KeyProvider, RevokeKeyRequest, SecretBytes,
    VerifyAuthenticationRequest, WrapKeyRequest, WrappedKey,
};
use graphhelm_sealed_key_provider::SealedKeyProvider;
use sha2::Sha256;
use static_assertions::{assert_impl_all, assert_not_impl_any};
use tempfile::TempDir;
use zeroize::ZeroizeOnDrop;

const KEYRING_FILE: &str = "keyring.v1.json";
const JOURNAL_FILE: &str = "revocations.v1.jsonl";
const KEYRING_TEMP_FILE: &str = ".keyring.v1.pending";
const LOCK_FILE: &str = ".sealed-key-provider.lock";

assert_impl_all!(Sha256: ZeroizeOnDrop);

fn block_on<F: Future>(future: F) -> F::Output {
    struct ThreadWaker(thread::Thread);
    impl Wake for ThreadWaker {
        fn wake(self: Arc<Self>) {
            self.0.unpark();
        }

        fn wake_by_ref(self: &Arc<Self>) {
            self.0.unpark();
        }
    }

    let waker = Waker::from(Arc::new(ThreadWaker(thread::current())));
    let mut context = Context::from_waker(&waker);
    let mut future = Box::pin(future);
    loop {
        match Pin::as_mut(&mut future).poll(&mut context) {
            Poll::Ready(output) => return output,
            Poll::Pending => thread::park(),
        }
    }
}

fn key_material(seed: u8) -> Vec<u8> {
    (0..32).map(|offset| seed.wrapping_add(offset)).collect()
}

fn create(directory: &Path, seed: u8) -> SealedKeyProvider {
    SealedKeyProvider::create(
        directory,
        "local-key-v1",
        SecretBytes::new(key_material(seed)),
    )
    .unwrap()
}

fn reopen(directory: &Path, seed: u8) -> SealedKeyProvider {
    SealedKeyProvider::open(
        directory,
        "local-key-v1",
        SecretBytes::new(key_material(seed)),
    )
    .unwrap()
}

fn wrap(provider: &SealedKeyProvider, handle: &str, seed: u8) -> WrappedKey {
    block_on(
        provider.wrap(
            WrapKeyRequest::new(
                handle,
                SecretBytes::new(key_material(seed)),
                b"evidence-aad-v1".to_vec(),
            )
            .unwrap(),
        ),
    )
    .unwrap()
}

fn key_error<T>(result: Result<T, KeyError>) -> KeyError {
    match result {
        Ok(_) => panic!("expected key operation to fail"),
        Err(error) => error,
    }
}

fn tamper_json_tag(path: &Path, line_index: usize, field: &str) {
    let text = fs::read_to_string(path).unwrap();
    let mut lines = text.lines().map(str::to_owned).collect::<Vec<_>>();
    let mut value: serde_json::Value = serde_json::from_str(&lines[line_index]).unwrap();
    let tag = value[field].as_str().unwrap();
    let replacement = if tag.starts_with('a') { 'b' } else { 'a' };
    value[field] = serde_json::Value::String(format!("{replacement}{}", &tag[1..]));
    lines[line_index] = serde_json::to_string(&value).unwrap();
    fs::write(path, format!("{}\n", lines.join("\n"))).unwrap();
}

#[test]
fn provider_wraps_opens_authenticates_and_rejects_tamper() {
    assert_not_impl_any!(SealedKeyProvider: Clone, std::fmt::Debug, serde::Serialize);

    let directory = TempDir::new().unwrap();
    let provider = create(directory.path(), 17);
    let wrapped = wrap(&provider, "handle-1", 91);

    let opened = block_on(provider.unwrap(wrapped.clone())).unwrap();
    assert_eq!(opened.expose(|bytes| bytes.to_vec()), key_material(91));
    assert!(!format!("{wrapped:?}").contains(&hex::encode(key_material(91))));

    let tag = block_on(provider.authenticate(
        AuthenticateRequest::new("checkpoint", b"bounded manifest".to_vec()).unwrap(),
    ))
    .unwrap();
    assert_eq!(tag.algorithm(), "hmac-sha256");
    assert_eq!(tag.bytes().len(), 32);
    assert_eq!(
        hex::encode(tag.bytes()),
        "d3e22b45f0e21d498b4bd9c1a4657918f96ec94e922c24ca4ebf6c0e62976142"
    );
    block_on(
        provider.verify(
            VerifyAuthenticationRequest::new(
                "checkpoint",
                b"bounded manifest".to_vec(),
                tag.clone(),
            )
            .unwrap(),
        ),
    )
    .unwrap();

    let wrong_bytes = key_error(block_on(provider.verify(
        VerifyAuthenticationRequest::new("checkpoint", b"changed".to_vec(), tag).unwrap(),
    )));
    assert_eq!(wrong_bytes, KeyError::Integrity);

    let mut tampered_ciphertext = wrapped.ciphertext().to_vec();
    tampered_ciphertext[0] ^= 1;
    let tampered = WrappedKey::new(
        wrapped.key_id(),
        wrapped.handle(),
        wrapped.algorithm(),
        wrapped.nonce().to_vec(),
        tampered_ciphertext,
        wrapped.aad_sha256().clone(),
    )
    .unwrap();
    assert_eq!(
        key_error(block_on(provider.unwrap(tampered))),
        KeyError::Integrity
    );

    assert_eq!(
        key_error(SealedKeyProvider::open(
            directory.path(),
            "local-key-v1",
            SecretBytes::new(key_material(18)),
        )),
        KeyError::Integrity
    );
}

#[test]
fn provider_metadata_is_object_safe_current_and_contains_no_custody_bytes() {
    let directory = TempDir::new().unwrap();
    let provider = create(directory.path(), 19);
    let key_provider: &dyn KeyProvider = &provider;

    let initial = block_on(key_provider.metadata()).unwrap();
    assert_eq!(initial.key_id(), "local-key-v1");
    assert_eq!(initial.algorithm(), "xchacha20poly1305+hmac-sha256");
    assert_eq!(initial.version(), "1.0.0");
    assert_eq!(initial.current_revocation_epoch(), 0);

    block_on(provider.revoke(RevokeKeyRequest::new("handle-1", "operation-1").unwrap())).unwrap();
    let current = block_on(key_provider.metadata()).unwrap();
    assert_eq!(current.current_revocation_epoch(), 1);

    let serialized = serde_json::to_string(&current).unwrap();
    assert_eq!(
        serialized,
        r#"{"keyId":"local-key-v1","algorithm":"xchacha20poly1305+hmac-sha256","version":"1.0.0","currentRevocationEpoch":1}"#
    );
    let debug = format!("{current:?}");
    for forbidden in [
        hex::encode(key_material(19)),
        directory.path().display().to_string(),
        JOURNAL_FILE.to_owned(),
        KEYRING_FILE.to_owned(),
        "recordTag".to_owned(),
    ] {
        assert!(!serialized.contains(&forbidden));
        assert!(!debug.contains(&forbidden));
    }
}

#[test]
fn revocation_is_exactly_idempotent_monotonic_and_survives_reopen() {
    let directory = TempDir::new().unwrap();
    let provider = create(directory.path(), 33);
    let stale_wrapped = wrap(&provider, "handle-1", 61);

    let first =
        block_on(provider.revoke(RevokeKeyRequest::new("handle-1", "revoke-operation-1").unwrap()))
            .unwrap();
    let retry =
        block_on(provider.revoke(RevokeKeyRequest::new("handle-1", "revoke-operation-1").unwrap()))
            .unwrap();
    assert_eq!(retry, first);
    assert_eq!(first.epoch(), 1);

    let divergent = key_error(block_on(
        provider.revoke(RevokeKeyRequest::new("handle-2", "revoke-operation-1").unwrap()),
    ));
    assert_eq!(divergent, KeyError::Conflict);
    assert_eq!(
        key_error(block_on(provider.unwrap(stale_wrapped.clone()))),
        KeyError::Unavailable
    );

    drop(provider);
    let reopened = reopen(directory.path(), 33);
    assert_eq!(reopened.epoch().unwrap(), 1);
    assert_eq!(
        key_error(block_on(reopened.unwrap(stale_wrapped))),
        KeyError::Unavailable
    );
    assert_eq!(
        block_on(
            reopened.revoke(RevokeKeyRequest::new("handle-1", "revoke-operation-1").unwrap(),)
        )
        .unwrap(),
        first
    );

    let second =
        block_on(reopened.revoke(RevokeKeyRequest::new("handle-2", "revoke-operation-2").unwrap()))
            .unwrap();
    assert_eq!(second.epoch(), 2);
}

#[test]
fn corrupt_truncated_and_reordered_journal_fail_closed() {
    let corrupt = TempDir::new().unwrap();
    let provider = create(corrupt.path(), 44);
    block_on(provider.revoke(RevokeKeyRequest::new("handle-1", "operation-1").unwrap())).unwrap();
    drop(provider);
    tamper_json_tag(&corrupt.path().join(JOURNAL_FILE), 1, "recordTag");
    assert_eq!(
        key_error(SealedKeyProvider::open(
            corrupt.path(),
            "local-key-v1",
            SecretBytes::new(key_material(44)),
        )),
        KeyError::Integrity
    );

    let truncated = TempDir::new().unwrap();
    let provider = create(truncated.path(), 45);
    block_on(provider.revoke(RevokeKeyRequest::new("handle-1", "operation-1").unwrap())).unwrap();
    drop(provider);
    let journal = truncated.path().join(JOURNAL_FILE);
    let mut bytes = fs::read(&journal).unwrap();
    bytes.pop();
    fs::write(&journal, bytes).unwrap();
    assert_eq!(
        key_error(SealedKeyProvider::open(
            truncated.path(),
            "local-key-v1",
            SecretBytes::new(key_material(45)),
        )),
        KeyError::Integrity
    );

    let reordered = TempDir::new().unwrap();
    let provider = create(reordered.path(), 46);
    block_on(provider.revoke(RevokeKeyRequest::new("handle-1", "operation-1").unwrap())).unwrap();
    block_on(provider.revoke(RevokeKeyRequest::new("handle-2", "operation-2").unwrap())).unwrap();
    drop(provider);
    let journal = reordered.path().join(JOURNAL_FILE);
    let text = fs::read_to_string(&journal).unwrap();
    let mut lines = text.lines().collect::<Vec<_>>();
    assert_eq!(lines.len(), 3);
    lines.swap(1, 2);
    fs::write(&journal, format!("{}\n", lines.join("\n"))).unwrap();
    assert_eq!(
        key_error(SealedKeyProvider::open(
            reordered.path(),
            "local-key-v1",
            SecretBytes::new(key_material(46)),
        )),
        KeyError::Integrity
    );
}

#[test]
fn keyring_authentication_no_overwrite_and_temp_cleanup_are_fail_closed() {
    let directory = TempDir::new().unwrap();
    // A crash leftover can only exist in a directory `create` already made owner-only, and
    // `create` never tightens a directory that holds anything.
    set_owner_only_directory(directory.path());
    fs::write(
        directory.path().join(KEYRING_TEMP_FILE),
        b"partial crash bytes",
    )
    .unwrap();
    let provider = create(directory.path(), 55);
    assert!(!directory.path().join(KEYRING_TEMP_FILE).exists());

    assert_eq!(
        key_error(SealedKeyProvider::create(
            directory.path(),
            "other-key",
            SecretBytes::new(key_material(99)),
        )),
        KeyError::Conflict
    );
    drop(provider);
    assert!(reopen(directory.path(), 55).epoch().is_ok());

    tamper_json_tag(&directory.path().join(KEYRING_FILE), 0, "authenticationTag");
    assert_eq!(
        key_error(SealedKeyProvider::open(
            directory.path(),
            "local-key-v1",
            SecretBytes::new(key_material(55)),
        )),
        KeyError::Integrity
    );
}

#[test]
fn keyring_destination_symlink_is_never_followed() {
    let directory = TempDir::new().unwrap();
    // Owner-only first: `create` only tightens an EMPTY directory, and this one is about to hold
    // the target and the link, so without this it would stop at `KeyError::Storage` on Unix
    // before ever reaching the keyring name.
    set_owner_only_directory(directory.path());
    let target = directory.path().join("attacker-target");
    fs::write(&target, b"do-not-overwrite").unwrap();
    let link = directory.path().join(KEYRING_FILE);

    if let Err(error) = create_file_symlink(&target, &link) {
        if symlink_privilege_is_unavailable(&error) {
            eprintln!("symlink creation unavailable without elevation; no adapter action executed");
            return;
        }
        panic!("failed to create test symlink: {error}");
    }

    assert_eq!(
        key_error(SealedKeyProvider::create(
            directory.path(),
            "local-key-v1",
            SecretBytes::new(key_material(66)),
        )),
        KeyError::Integrity
    );
    assert_eq!(fs::read(target).unwrap(), b"do-not-overwrite");
}

#[test]
fn concurrent_writers_publish_unique_monotonic_epochs() {
    let directory = TempDir::new().unwrap();
    create(directory.path(), 77);
    let first = reopen(directory.path(), 77);
    let second = reopen(directory.path(), 77);

    let one = thread::spawn(move || {
        block_on(first.revoke(RevokeKeyRequest::new("handle-1", "operation-1").unwrap()))
            .unwrap()
            .epoch()
    });
    let two = thread::spawn(move || {
        block_on(second.revoke(RevokeKeyRequest::new("handle-2", "operation-2").unwrap()))
            .unwrap()
            .epoch()
    });
    let mut epochs = vec![one.join().unwrap(), two.join().unwrap()];
    epochs.sort_unstable();
    assert_eq!(epochs, vec![1, 2]);
    assert_eq!(reopen(directory.path(), 77).epoch().unwrap(), 2);
}

#[test]
fn an_open_provider_accepts_an_authenticated_extension_from_another_instance() {
    let directory = TempDir::new().unwrap();
    let first = create(directory.path(), 78);
    let second = reopen(directory.path(), 78);
    let second_handle = wrap(&first, "handle-second-instance", 79);

    block_on(first.revoke(RevokeKeyRequest::new("handle-first", "operation-first").unwrap()))
        .unwrap();
    block_on(
        second.revoke(RevokeKeyRequest::new("handle-second-instance", "operation-second").unwrap()),
    )
    .unwrap();

    assert_eq!(first.epoch().unwrap(), 2);
    assert_eq!(
        key_error(block_on(first.unwrap(second_handle))),
        KeyError::Unavailable
    );
}

#[test]
fn relative_root_is_anchored_across_working_directory_changes() {
    static CWD_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _serial = CWD_LOCK.lock().unwrap();
    let original = std::env::current_dir().unwrap();
    let directory = tempfile::Builder::new()
        .prefix("graphhelm-relative-root-")
        .tempdir_in(&original)
        .unwrap();
    let relative = directory.path().strip_prefix(&original).unwrap().to_owned();
    let elsewhere = TempDir::new().unwrap();

    let provider = create(&relative, 81);
    std::env::set_current_dir(elsewhere.path()).unwrap();
    let result = provider.epoch();
    std::env::set_current_dir(&original).unwrap();

    assert_eq!(result.unwrap(), 0);
}

#[test]
fn parent_symlink_or_reparse_component_is_rejected() {
    let directory = TempDir::new().unwrap();
    let real_parent = directory.path().join("real-parent");
    let root = real_parent.join("provider-root");
    fs::create_dir_all(&root).unwrap();
    set_owner_only_directory(&root);
    let alias = directory.path().join("alias-parent");

    if let Err(error) = create_directory_symlink(&real_parent, &alias) {
        if symlink_privilege_is_unavailable(&error) {
            return;
        }
        panic!("failed to create directory symlink: {error}");
    }

    assert_eq!(
        key_error(SealedKeyProvider::create(
            alias.join("provider-root"),
            "local-key-v1",
            SecretBytes::new(key_material(82)),
        )),
        KeyError::Storage
    );
}

#[test]
fn root_replacement_fails_without_touching_replacement_directory() {
    let parent = TempDir::new().unwrap();
    let root = parent.path().join("provider-root");
    fs::create_dir(&root).unwrap();
    set_owner_only_directory(&root);
    let provider = create(&root, 83);
    let displaced = parent.path().join("displaced-root");
    if let Err(error) = fs::rename(&root, &displaced) {
        if cfg!(windows) && matches!(error.raw_os_error(), Some(5) | Some(32)) {
            assert_eq!(provider.epoch().unwrap(), 0);
            return;
        }
        panic!("failed to exercise root replacement: {error}");
    }
    fs::create_dir(&root).unwrap();
    set_owner_only_directory(&root);

    assert_eq!(key_error(provider.epoch()), KeyError::Integrity);
    assert!(!root.join(LOCK_FILE).exists());
}

#[test]
fn lock_replacement_is_rejected_after_lock_acquisition() {
    let directory = TempDir::new().unwrap();
    let provider = create(directory.path(), 84);
    let lock_path = directory.path().join(LOCK_FILE);
    let displaced_lock = directory.path().join("displaced.lock");
    if let Err(error) = fs::rename(&lock_path, &displaced_lock) {
        if cfg!(windows) && matches!(error.raw_os_error(), Some(5) | Some(32)) {
            assert_eq!(provider.epoch().unwrap(), 0);
            return;
        }
        panic!("failed to exercise lock replacement: {error}");
    }
    fs::write(&lock_path, b"").unwrap();

    assert_eq!(key_error(provider.epoch()), KeyError::Integrity);
}

#[cfg(windows)]
#[test]
fn created_provider_state_has_a_protected_dacl() {
    let directory = TempDir::new().unwrap();
    let provider = create(directory.path(), 85);

    for path in [
        directory.path().to_path_buf(),
        directory.path().join(LOCK_FILE),
        directory.path().join(KEYRING_FILE),
        directory.path().join(JOURNAL_FILE),
    ] {
        assert!(windows_dacl_is_protected(&path));
    }
    drop(provider);
}

#[cfg(windows)]
#[test]
fn permissive_existing_state_is_rejected() {
    let directory = TempDir::new().unwrap();
    drop(create(directory.path(), 86));
    let keyring = directory.path().join(KEYRING_FILE);
    set_windows_everyone_full_control(&keyring);
    assert!(windows_dacl_is_protected(&keyring));

    assert_eq!(
        key_error(SealedKeyProvider::open(
            directory.path(),
            "local-key-v1",
            SecretBytes::new(key_material(86)),
        )),
        KeyError::Integrity
    );
}

#[cfg(windows)]
fn windows_dacl_is_protected(path: &Path) -> bool {
    use std::{iter, os::windows::ffi::OsStrExt, ptr};
    use windows_sys::Win32::{
        Foundation::LocalFree,
        Security::{
            Authorization::{GetNamedSecurityInfoW, SE_FILE_OBJECT},
            DACL_SECURITY_INFORMATION, GetSecurityDescriptorControl, PSECURITY_DESCRIPTOR,
            SE_DACL_PROTECTED,
        },
    };

    let wide = path
        .as_os_str()
        .encode_wide()
        .chain(iter::once(0))
        .collect::<Vec<_>>();
    let mut descriptor: PSECURITY_DESCRIPTOR = ptr::null_mut();
    // SAFETY: `wide` is NUL-terminated and all optional outputs except the allocated
    // descriptor are null because this probe only needs descriptor control flags.
    let status = unsafe {
        GetNamedSecurityInfoW(
            wide.as_ptr(),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION,
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
            &mut descriptor,
        )
    };
    if status != 0 || descriptor.is_null() {
        return false;
    }
    let mut control = 0_u16;
    let mut revision = 0_u32;
    // SAFETY: a successful `GetNamedSecurityInfoW` returned a live descriptor and
    // both output pointers reference initialized stack storage.
    let succeeded =
        unsafe { GetSecurityDescriptorControl(descriptor, &mut control, &mut revision) };
    // SAFETY: Windows allocated `descriptor` for this call and requires `LocalFree`.
    unsafe { LocalFree(descriptor.cast()) };
    succeeded != 0 && control & SE_DACL_PROTECTED != 0
}

#[cfg(windows)]
fn set_windows_everyone_full_control(path: &Path) {
    use std::{iter, os::windows::ffi::OsStrExt, ptr};
    use windows_sys::Win32::{
        Foundation::LocalFree,
        Security::{
            Authorization::{
                ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
                SE_FILE_OBJECT, SetNamedSecurityInfoW,
            },
            DACL_SECURITY_INFORMATION, GetSecurityDescriptorDacl,
            PROTECTED_DACL_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR,
        },
    };

    let sddl = "D:P(A;;FA;;;WD)"
        .encode_utf16()
        .chain(iter::once(0))
        .collect::<Vec<_>>();
    let mut descriptor: PSECURITY_DESCRIPTOR = ptr::null_mut();
    // SAFETY: `sddl` is a valid NUL-terminated descriptor string and the output
    // pointer is valid for the allocation returned by Windows.
    assert_ne!(
        unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                sddl.as_ptr(),
                SDDL_REVISION_1,
                &mut descriptor,
                ptr::null_mut(),
            )
        },
        0
    );
    let mut present = 0;
    let mut defaulted = 0;
    let mut dacl = ptr::null_mut();
    // SAFETY: `descriptor` is live and all DACL output pointers are writable.
    assert_ne!(
        unsafe { GetSecurityDescriptorDacl(descriptor, &mut present, &mut dacl, &mut defaulted) },
        0
    );
    assert_ne!(present, 0);
    assert!(!dacl.is_null());
    let mut wide = path
        .as_os_str()
        .encode_wide()
        .chain(iter::once(0))
        .collect::<Vec<_>>();
    // SAFETY: the path and descriptor-owned DACL remain live for the call; owner,
    // group and SACL are intentionally unchanged.
    let status = unsafe {
        SetNamedSecurityInfoW(
            wide.as_mut_ptr(),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
            ptr::null_mut(),
            ptr::null_mut(),
            dacl,
            ptr::null_mut(),
        )
    };
    // SAFETY: Windows allocated `descriptor` for this call and requires `LocalFree`.
    unsafe { LocalFree(descriptor.cast()) };
    assert_eq!(status, 0);
}

#[cfg(unix)]
fn create_file_symlink(target: &Path, link: &Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(target, link)
}

#[cfg(unix)]
fn create_directory_symlink(target: &Path, link: &Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(target, link)
}

#[cfg(windows)]
fn create_directory_symlink(target: &Path, link: &Path) -> std::io::Result<()> {
    std::os::windows::fs::symlink_dir(target, link)
}

fn symlink_privilege_is_unavailable(error: &std::io::Error) -> bool {
    cfg!(windows) && error.raw_os_error() == Some(1314)
}

#[cfg(unix)]
fn set_owner_only_directory(path: &Path) {
    use std::os::unix::fs::PermissionsExt;

    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
    assert_eq!(
        fs::metadata(path).unwrap().permissions().mode() & 0o777,
        0o700
    );
}

#[cfg(not(unix))]
fn set_owner_only_directory(_path: &Path) {}

#[cfg(windows)]
fn create_file_symlink(target: &Path, link: &Path) -> std::io::Result<()> {
    std::os::windows::fs::symlink_file(target, link)
}

#[cfg(unix)]
fn unix_mode(path: &Path) -> u32 {
    use std::os::unix::fs::PermissionsExt;
    fs::symlink_metadata(path).unwrap().permissions().mode() & 0o7777
}

#[cfg(unix)]
fn set_unix_mode(path: &Path, mode: u32) {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(mode)).unwrap();
    assert_eq!(unix_mode(path), mode);
}

/// #1305: `mkdir` under the default umask makes `0755`. `create` tightens a directory the caller
/// owns, as it already does on Windows, instead of refusing it with `KeyError::Storage`.
#[cfg(unix)]
#[test]
fn create_tightens_an_owned_0755_directory_to_0700() {
    let parent = TempDir::new().unwrap();
    let root = parent.path().join("keyring");
    fs::create_dir(&root).unwrap();
    set_unix_mode(&root, 0o755);

    drop(create(&root, 87));

    assert_eq!(unix_mode(&root), 0o700);
    assert_eq!(reopen(&root, 87).epoch().unwrap(), 0);
}

/// Only an EMPTY directory is tightened. One that already holds anything (here an unrelated file)
/// is somebody's directory, not a fresh keyring: `--keyring ~` must not chmod a home directory.
/// It is refused as before #1305, and its mode and contents are left exactly as they were.
#[cfg(unix)]
#[test]
fn a_non_empty_0755_directory_is_refused_and_left_untouched() {
    let parent = TempDir::new().unwrap();
    let root = parent.path().join("home");
    fs::create_dir(&root).unwrap();
    fs::write(root.join("notes.txt"), b"unrelated").unwrap();
    set_unix_mode(&root, 0o755);

    assert_eq!(
        key_error(SealedKeyProvider::create(
            &root,
            "local-key-v1",
            SecretBytes::new(key_material(91)),
        )),
        KeyError::Storage
    );
    assert_eq!(unix_mode(&root), 0o755);
    assert!(!root.join(LOCK_FILE).exists());
    assert!(!root.join(KEYRING_FILE).exists());
    assert_eq!(fs::read(root.join("notes.txt")).unwrap(), b"unrelated");
}

/// No permission bit is ever added. An empty `0500` directory lacks the owner write bit; the
/// narrowed mode (`0500 & 0700`) is not `0700`, so it is refused rather than widened.
#[cfg(unix)]
#[test]
fn a_0500_directory_is_refused_and_not_widened() {
    let parent = TempDir::new().unwrap();
    let root = parent.path().join("keyring");
    fs::create_dir(&root).unwrap();
    set_unix_mode(&root, 0o500);

    let result =
        SealedKeyProvider::create(&root, "local-key-v1", SecretBytes::new(key_material(92)));
    let mode = unix_mode(&root);
    // Restore write access before asserting so `TempDir` can always clean up.
    set_unix_mode(&root, 0o700);

    assert_eq!(key_error(result), KeyError::Storage);
    assert_eq!(mode, 0o500);
}

/// A keyring path that is itself a symlink stays refused, and the tightening never reaches the
/// directory the link points at.
#[test]
fn a_symlinked_keyring_directory_is_refused_and_its_target_is_untouched() {
    let parent = TempDir::new().unwrap();
    let target = parent.path().join("real-keyring");
    fs::create_dir(&target).unwrap();
    #[cfg(unix)]
    set_unix_mode(&target, 0o755);
    let link = parent.path().join("keyring-link");
    if let Err(error) = create_directory_symlink(&target, &link) {
        if symlink_privilege_is_unavailable(&error) {
            eprintln!("symlink creation unavailable without elevation; no adapter action executed");
            return;
        }
        panic!("failed to create directory symlink: {error}");
    }

    assert_eq!(
        key_error(SealedKeyProvider::create(
            &link,
            "local-key-v1",
            SecretBytes::new(key_material(88)),
        )),
        KeyError::Storage
    );
    assert!(!target.join(LOCK_FILE).exists());
    assert!(!target.join(KEYRING_FILE).exists());
    #[cfg(unix)]
    assert_eq!(unix_mode(&target), 0o755);
}

/// The open path keeps its strict check: a keyring whose directory was loosened after it was
/// created is refused, and neither `open` nor a repeated `create` repairs the directory.
#[cfg(unix)]
#[test]
fn an_existing_keyring_in_a_loosened_directory_stays_refused() {
    let parent = TempDir::new().unwrap();
    let root = parent.path().join("keyring");
    fs::create_dir(&root).unwrap();
    set_unix_mode(&root, 0o700);
    drop(create(&root, 89));
    set_unix_mode(&root, 0o755);

    assert_eq!(
        key_error(SealedKeyProvider::open(
            &root,
            "local-key-v1",
            SecretBytes::new(key_material(89)),
        )),
        KeyError::Storage
    );
    assert!(
        SealedKeyProvider::create(&root, "local-key-v1", SecretBytes::new(key_material(89)))
            .is_err()
    );
    assert_eq!(unix_mode(&root), 0o755);
}

/// A directory owned by another user is refused and never changed. Uses `/`, which is owned by
/// root: the cell is skipped when the test itself runs as the owner of `/` (as root, it would own
/// it and `create` would be allowed to tighten it).
#[cfg(unix)]
#[test]
fn a_directory_owned_by_another_user_is_refused_and_left_untouched() {
    use std::os::unix::fs::MetadataExt;

    let own = TempDir::new().unwrap();
    let own_uid = fs::metadata(own.path()).unwrap().uid();
    let foreign = Path::new("/");
    if own_uid == 0 || fs::metadata(foreign).unwrap().uid() == own_uid {
        eprintln!("running as the owner of `/`; the foreign-owner cell is not exercised");
        return;
    }
    let before = unix_mode(foreign);

    assert_eq!(
        key_error(SealedKeyProvider::create(
            foreign,
            "local-key-v1",
            SecretBytes::new(key_material(90)),
        )),
        KeyError::Storage
    );
    assert_eq!(unix_mode(foreign), before);
}
