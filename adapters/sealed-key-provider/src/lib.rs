//! Durable local key provider backed by authenticated sealed metadata.

mod journal;
mod keyring;

use std::{
    collections::HashMap,
    fs::{self, File, Metadata},
    path::{Component, Path, PathBuf},
    sync::{Arc, Mutex, OnceLock, Weak},
};

use chacha20poly1305::{
    XChaCha20Poly1305, XNonce,
    aead::{Aead, KeyInit, Payload},
};
use fs2::FileExt;
use graphhelm_events::{
    AuthenticateRequest, AuthenticationTag, KeyError, KeyProvider, KeyProviderMetadata,
    RepositoryFuture, RevocationReceipt, RevokeKeyRequest, SecretBytes,
    VerifyAuthenticationRequest, WrapKeyRequest, WrappedKey,
};
use graphhelm_protocols::{OpaqueId, RawSha256};
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

#[cfg(test)]
use std::cell::RefCell;

const LOCK_FILE: &str = ".sealed-key-provider.lock";
const AUTH_ALGORITHM: &str = "hmac-sha256";
const WRAP_ALGORITHM: &str = "xchacha20poly1305";
const PROVIDER_ALGORITHM: &str = "xchacha20poly1305+hmac-sha256";
const PROVIDER_VERSION: &str = "1.0.0";

#[cfg(test)]
#[derive(Clone, Copy, PartialEq, Eq)]
enum TestRacePoint {
    BeforePathIo,
    AfterPathIo,
}

#[cfg(test)]
type TestRaceHook = Box<dyn FnMut(TestRacePoint)>;

#[cfg(test)]
thread_local! {
    static TEST_RACE_HOOK: RefCell<Option<TestRaceHook>> = RefCell::new(None);
}

#[cfg(test)]
struct TestRaceHookGuard;

#[cfg(test)]
impl Drop for TestRaceHookGuard {
    fn drop(&mut self) {
        TEST_RACE_HOOK.with(|slot| *slot.borrow_mut() = None);
    }
}

#[cfg(test)]
fn install_test_race_hook(hook: impl FnMut(TestRacePoint) + 'static) -> TestRaceHookGuard {
    TEST_RACE_HOOK.with(|slot| *slot.borrow_mut() = Some(Box::new(hook)));
    TestRaceHookGuard
}

#[cfg(test)]
fn test_race_point(point: TestRacePoint) {
    TEST_RACE_HOOK.with(|slot| {
        if let Some(hook) = slot.borrow_mut().as_mut() {
            hook(point);
        }
    });
}

/// Explicitly keyed, durable local implementation of the adapter-neutral key boundary.
pub struct SealedKeyProvider {
    directory: AnchoredDirectory,
    journal: Mutex<LockedJournal>,
    shared_floor: SharedJournalFloor,
    _keyring: File,
    key_id: OpaqueId,
    key_material: SecretBytes,
}

struct LockedJournal {
    file: File,
}

struct SharedJournalFloor {
    identity: SharedFloorIdentity,
    floor: Arc<Mutex<journal::JournalFloor>>,
}

#[derive(Clone, PartialEq, Eq, Hash)]
struct SharedFloorIdentity {
    root: FileIdentity,
    key_id: String,
}

type SharedFloorRegistry = HashMap<SharedFloorIdentity, Weak<Mutex<journal::JournalFloor>>>;

// This mutex protects only weak-reference lookup and lifecycle bookkeeping. Filesystem access,
// journal authentication, repository locking, cryptography, and provider callbacks never execute
// while it is held. Each live storage/key identity has a separate floor mutex.
static SHARED_FLOORS: OnceLock<Mutex<SharedFloorRegistry>> = OnceLock::new();

struct AnchoredLock {
    file: File,
    identity: FileIdentity,
}

pub(crate) struct AnchoredDirectory {
    path: PathBuf,
    handle: File,
    identity: FileIdentity,
    lock: Mutex<AnchoredLock>,
}

#[derive(Clone, Copy)]
enum DirectoryAccess {
    Initialize,
    Existing,
}

#[derive(Clone, Copy)]
pub(crate) enum FileAccess {
    ReadOnly,
    ReadWrite,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct FileIdentity {
    first: u64,
    second: u64,
}

impl SealedKeyProvider {
    pub fn create(
        directory: impl AsRef<Path>,
        key_id: impl Into<String>,
        key_material: SecretBytes,
    ) -> Result<Self, KeyError> {
        validate_key_material(&key_material)?;
        let directory = AnchoredDirectory::open(directory.as_ref(), DirectoryAccess::Initialize)?;
        let key_id = OpaqueId::parse(key_id.into()).map_err(|_| KeyError::Invalid)?;
        let (keyring, journal, state) = directory.with_exclusive_lock(|| {
            if keyring::exists(&directory)? {
                return Err(KeyError::Conflict);
            }
            let journal =
                journal::create_or_recover_empty(&directory, key_id.as_str(), &key_material)?;
            let keyring = keyring::publish(&directory, key_id.as_str(), &key_material)?;
            directory.sync()?;
            let mut journal = journal;
            let state = journal::load(&mut journal, key_id.as_str(), &key_material)?;
            Ok((keyring, journal, state))
        })?;
        let floor_identity = SharedFloorIdentity::new(&directory, key_id.as_str());
        let shared_floor = SharedJournalFloor::join(floor_identity, &state)?;

        Ok(Self {
            directory,
            journal: Mutex::new(LockedJournal { file: journal }),
            shared_floor,
            _keyring: keyring,
            key_id,
            key_material,
        })
    }

    pub fn open(
        directory: impl AsRef<Path>,
        key_id: impl Into<String>,
        key_material: SecretBytes,
    ) -> Result<Self, KeyError> {
        validate_key_material(&key_material)?;
        let directory = AnchoredDirectory::open(directory.as_ref(), DirectoryAccess::Existing)?;
        let key_id = OpaqueId::parse(key_id.into()).map_err(|_| KeyError::Invalid)?;
        let (keyring, journal, state) = directory.with_exclusive_lock(|| {
            let keyring = keyring::open_and_verify(&directory, key_id.as_str(), &key_material)?;
            let mut journal = journal::open(&directory)?;
            let state = journal::load(&mut journal, key_id.as_str(), &key_material)?;
            Ok((keyring, journal, state))
        })?;
        let floor_identity = SharedFloorIdentity::new(&directory, key_id.as_str());
        let shared_floor = SharedJournalFloor::join(floor_identity, &state)?;
        Ok(Self {
            directory,
            journal: Mutex::new(LockedJournal { file: journal }),
            shared_floor,
            _keyring: keyring,
            key_id,
            key_material,
        })
    }

    pub fn epoch(&self) -> Result<u64, KeyError> {
        Ok(self.metadata_now()?.current_revocation_epoch())
    }

    fn metadata_now(&self) -> Result<KeyProviderMetadata, KeyError> {
        self.with_locked_state(|_, state| {
            let epoch = state.epoch();
            KeyProviderMetadata::new(
                self.key_id.as_str(),
                PROVIDER_ALGORITHM,
                PROVIDER_VERSION,
                epoch,
            )
        })
    }

    fn with_locked_state<R>(
        &self,
        operation: impl FnOnce(&mut File, journal::JournalState) -> Result<R, KeyError>,
    ) -> Result<R, KeyError> {
        self.directory.with_exclusive_lock(|| {
            let mut journal = self.journal.lock().map_err(|_| KeyError::Storage)?;
            self.directory
                .verify_child_identity(journal::JOURNAL_FILE, &journal.file)?;
            let state = journal::load(&mut journal.file, self.key_id.as_str(), &self.key_material)?;
            let observed_floor = self.shared_floor.observe(&state)?;
            let result = operation(&mut journal.file, state);
            let verification = self
                .directory
                .verify_child_identity(journal::JOURNAL_FILE, &journal.file);
            match (result, verification) {
                (_, Err(error)) => Err(error),
                (Err(error), Ok(())) => Err(error),
                (Ok(result), Ok(())) => {
                    let confirmed =
                        journal::load(&mut journal.file, self.key_id.as_str(), &self.key_material)?;
                    self.shared_floor.confirm(&confirmed, &observed_floor)?;
                    Ok(result)
                }
            }
        })
    }
}

impl SharedFloorIdentity {
    fn new(directory: &AnchoredDirectory, key_id: &str) -> Self {
        Self {
            root: directory.identity,
            key_id: key_id.to_owned(),
        }
    }
}

impl SharedJournalFloor {
    fn join(
        identity: SharedFloorIdentity,
        authenticated_state: &journal::JournalState,
    ) -> Result<Self, KeyError> {
        let authenticated_floor = authenticated_state.floor()?;
        let floor = {
            let mut registry = shared_floor_registry()
                .lock()
                .map_err(|_| KeyError::Integrity)?;
            if let Some(existing) = registry.get(&identity).and_then(Weak::upgrade) {
                existing
            } else {
                let floor = Arc::new(Mutex::new(authenticated_floor.clone()));
                registry.insert(identity.clone(), Arc::downgrade(&floor));
                floor
            }
        };

        {
            let mut live_floor = floor.lock().map_err(|_| KeyError::Integrity)?;
            authenticated_state.require_extension_of(&live_floor)?;
            *live_floor = authenticated_floor;
        }

        Ok(Self { identity, floor })
    }

    fn observe(
        &self,
        authenticated_state: &journal::JournalState,
    ) -> Result<journal::JournalFloor, KeyError> {
        let observed_floor = authenticated_state.floor()?;
        // The per-identity floor lock covers only comparison and replacement of two small values;
        // it is released before the caller invokes any operation callback or cryptographic work.
        let mut live_floor = self.floor.lock().map_err(|_| KeyError::Integrity)?;
        authenticated_state.require_extension_of(&live_floor)?;
        *live_floor = observed_floor.clone();
        Ok(observed_floor)
    }

    fn confirm(
        &self,
        authenticated_state: &journal::JournalState,
        operation_floor: &journal::JournalFloor,
    ) -> Result<(), KeyError> {
        authenticated_state.require_extension_of(operation_floor)?;
        let confirmed_floor = authenticated_state.floor()?;
        let mut live_floor = self.floor.lock().map_err(|_| KeyError::Integrity)?;
        authenticated_state.require_extension_of(&live_floor)?;
        *live_floor = confirmed_floor;
        Ok(())
    }
}

impl Drop for SharedJournalFloor {
    fn drop(&mut self) {
        let Some(registry) = SHARED_FLOORS.get() else {
            return;
        };
        let Ok(mut registry) = registry.lock() else {
            return;
        };
        let is_last_live_floor = Arc::strong_count(&self.floor) == 1
            && registry
                .get(&self.identity)
                .is_some_and(|registered| Weak::ptr_eq(registered, &Arc::downgrade(&self.floor)));
        if is_last_live_floor {
            registry.remove(&self.identity);
        }
    }
}

fn shared_floor_registry() -> &'static Mutex<SharedFloorRegistry> {
    SHARED_FLOORS.get_or_init(|| Mutex::new(HashMap::new()))
}

impl KeyProvider for SealedKeyProvider {
    fn metadata<'a>(&'a self) -> RepositoryFuture<'a, Result<KeyProviderMetadata, KeyError>> {
        Box::pin(async move { self.metadata_now() })
    }

    fn wrap<'a>(
        &'a self,
        request: WrapKeyRequest,
    ) -> RepositoryFuture<'a, Result<WrappedKey, KeyError>> {
        Box::pin(async move {
            self.with_locked_state(|_, state| {
                if state.is_revoked(request.handle()) {
                    return Err(KeyError::Unavailable);
                }
                let (handle, plaintext_key, aad) = request.into_parts();
                let aad_sha256 = raw_sha256(&aad);
                let wrapping_aad = wrapping_aad(&handle, &aad_sha256)?;
                let mut nonce = vec![0_u8; 24];
                getrandom::fill(&mut nonce).map_err(|_| KeyError::Storage)?;
                let ciphertext =
                    with_derived_key(&self.key_material, b"graphhelm-wrapping-key-v1", |key| {
                        let cipher = XChaCha20Poly1305::new_from_slice(key)
                            .map_err(|_| KeyError::Integrity)?;
                        let nonce_array =
                            XNonce::try_from(nonce.as_slice()).map_err(|_| KeyError::Invalid)?;
                        plaintext_key.expose(|plaintext| {
                            cipher
                                .encrypt(
                                    &nonce_array,
                                    Payload {
                                        msg: plaintext,
                                        aad: &wrapping_aad,
                                    },
                                )
                                .map_err(|_| KeyError::Integrity)
                        })
                    })?;
                WrappedKey::new(
                    self.key_id.as_str(),
                    handle,
                    WRAP_ALGORITHM,
                    nonce,
                    ciphertext,
                    aad_sha256,
                )
            })
        })
    }

    fn unwrap<'a>(
        &'a self,
        wrapped: WrappedKey,
    ) -> RepositoryFuture<'a, Result<SecretBytes, KeyError>> {
        Box::pin(async move {
            self.with_locked_state(|_, state| {
                if wrapped.key_id() != self.key_id.as_str() {
                    return Err(KeyError::Unavailable);
                }
                if state.is_revoked(wrapped.handle()) {
                    return Err(KeyError::Unavailable);
                }
                let wrapping_aad = wrapping_aad(wrapped.handle(), wrapped.aad_sha256())?;
                let plaintext =
                    with_derived_key(&self.key_material, b"graphhelm-wrapping-key-v1", |key| {
                        let cipher = XChaCha20Poly1305::new_from_slice(key)
                            .map_err(|_| KeyError::Integrity)?;
                        let nonce =
                            XNonce::try_from(wrapped.nonce()).map_err(|_| KeyError::Invalid)?;
                        cipher
                            .decrypt(
                                &nonce,
                                Payload {
                                    msg: wrapped.ciphertext(),
                                    aad: &wrapping_aad,
                                },
                            )
                            .map_err(|_| KeyError::Integrity)
                    })?;
                if plaintext.len() != 32 {
                    return Err(KeyError::Integrity);
                }
                Ok(SecretBytes::new(plaintext))
            })
        })
    }

    fn revoke<'a>(
        &'a self,
        request: RevokeKeyRequest,
    ) -> RepositoryFuture<'a, Result<RevocationReceipt, KeyError>> {
        Box::pin(async move {
            self.with_locked_state(|journal, state| {
                journal::append_revocation(
                    journal,
                    &self.directory,
                    self.key_id.as_str(),
                    &self.key_material,
                    state,
                    request,
                )
            })
        })
    }

    fn authenticate<'a>(
        &'a self,
        request: AuthenticateRequest,
    ) -> RepositoryFuture<'a, Result<AuthenticationTag, KeyError>> {
        Box::pin(async move {
            let (purpose, bytes) = request.into_parts();
            self.with_locked_state(|_, _| {
                let tag = authentication_bytes(&self.key_material, &purpose, &bytes)?;
                AuthenticationTag::new(self.key_id.as_str(), AUTH_ALGORITHM, tag)
            })
        })
    }

    fn verify<'a>(
        &'a self,
        request: VerifyAuthenticationRequest,
    ) -> RepositoryFuture<'a, Result<(), KeyError>> {
        Box::pin(async move {
            let (purpose, bytes, tag) = request.into_parts();
            self.with_locked_state(|_, _| {
                if tag.key_id() != self.key_id.as_str() || tag.algorithm() != AUTH_ALGORITHM {
                    return Err(KeyError::Integrity);
                }
                verify_authentication_bytes(&self.key_material, &purpose, &bytes, tag.bytes())
            })
        })
    }
}

fn validate_key_material(key_material: &SecretBytes) -> Result<(), KeyError> {
    if key_material.len() == 32 {
        Ok(())
    } else {
        Err(KeyError::Invalid)
    }
}

impl AnchoredDirectory {
    fn open(directory: &Path, access: DirectoryAccess) -> Result<Self, KeyError> {
        let (path, handle) = open_directory_handle(directory, access)?;
        let metadata = handle.metadata().map_err(|_| KeyError::Storage)?;
        #[cfg(unix)]
        let metadata = if matches!(access, DirectoryAccess::Initialize) {
            tighten_owned_directory(&handle, metadata)?
        } else {
            metadata
        };
        #[cfg(windows)]
        if matches!(access, DirectoryAccess::Initialize) {
            apply_windows_protected_dacl(&handle)?;
        }
        validate_secure_directory(&metadata)?;
        #[cfg(windows)]
        validate_windows_handle_dacl(&handle)?;
        let identity = file_identity(&handle)?;
        let lock_file = open_or_create_lock(&handle, &path)?;
        let lock_identity = file_identity(&lock_file)?;
        let anchored = Self {
            path,
            handle,
            identity,
            lock: Mutex::new(AnchoredLock {
                file: lock_file,
                identity: lock_identity,
            }),
        };
        anchored.verify_root_path()?;
        {
            let lock = anchored.lock.lock().map_err(|_| KeyError::Storage)?;
            anchored.verify_lock_identity(&lock)?;
        }
        Ok(anchored)
    }

    fn verify_root_path(&self) -> Result<(), KeyError> {
        let named = fs::symlink_metadata(&self.path).map_err(|_| KeyError::Integrity)?;
        let (_, named_handle) = open_directory_handle(&self.path, DirectoryAccess::Existing)
            .map_err(|_| KeyError::Integrity)?;
        if is_link_or_reparse(&named)
            || !named.is_dir()
            || file_identity(&named_handle)? != self.identity
            || file_identity(&self.handle)? != self.identity
        {
            return Err(KeyError::Integrity);
        }
        validate_secure_directory(&named).map_err(|_| KeyError::Integrity)?;
        #[cfg(windows)]
        validate_windows_handle_dacl(&named_handle).map_err(|_| KeyError::Integrity)?;
        Ok(())
    }

    pub(crate) fn sync(&self) -> Result<(), KeyError> {
        self.handle.sync_all().map_err(|_| KeyError::Storage)
    }

    fn with_exclusive_lock<R>(
        &self,
        operation: impl FnOnce() -> Result<R, KeyError>,
    ) -> Result<R, KeyError> {
        let lock = self.lock.lock().map_err(|_| KeyError::Storage)?;
        #[cfg(unix)]
        self.handle
            .lock_exclusive()
            .map_err(|_| KeyError::Storage)?;
        if lock.file.lock_exclusive().is_err() {
            #[cfg(unix)]
            let _ = FileExt::unlock(&self.handle);
            return Err(KeyError::Storage);
        }
        let before = self
            .verify_root_path()
            .and_then(|()| self.verify_lock_identity(&lock));
        #[cfg(test)]
        if before.is_ok() {
            test_race_point(TestRacePoint::BeforePathIo);
        }
        let result = match before {
            Ok(()) => operation(),
            Err(error) => Err(error),
        };
        #[cfg(test)]
        test_race_point(TestRacePoint::AfterPathIo);
        let after = self
            .verify_root_path()
            .and_then(|()| self.verify_lock_identity(&lock));
        let unlock = FileExt::unlock(&lock.file).map_err(|_| KeyError::Storage);
        #[cfg(unix)]
        let root_unlock = FileExt::unlock(&self.handle).map_err(|_| KeyError::Storage);
        #[cfg(not(unix))]
        let root_unlock: Result<(), KeyError> = Ok(());
        match (result, after, unlock, root_unlock) {
            (_, Err(error), _, _) | (_, _, Err(error), _) | (_, _, _, Err(error)) => Err(error),
            (result, Ok(()), Ok(()), Ok(())) => result,
        }
    }

    fn verify_lock_identity(&self, lock: &AnchoredLock) -> Result<(), KeyError> {
        let handle_identity = file_identity(&lock.file)?;
        if handle_identity != lock.identity {
            return Err(KeyError::Integrity);
        }
        self.verify_child_identity(LOCK_FILE, &lock.file)
    }

    pub(crate) fn verify_child_identity(&self, name: &str, file: &File) -> Result<(), KeyError> {
        validate_secure_regular(&file.metadata().map_err(|_| KeyError::Integrity)?)
            .map_err(|_| KeyError::Integrity)?;
        #[cfg(windows)]
        validate_windows_handle_dacl(file).map_err(|_| KeyError::Integrity)?;
        let expected = file_identity(file)?;
        let named = named_file_identity(&self.handle, &self.path, name)?;
        if named != expected {
            Err(KeyError::Integrity)
        } else {
            Ok(())
        }
    }

    pub(crate) fn try_open_regular(
        &self,
        name: &str,
        access: FileAccess,
    ) -> Result<Option<File>, KeyError> {
        match open_child_file(&self.handle, &self.path, name, access, false, false) {
            Ok(file) => {
                validate_secure_regular(&file.metadata().map_err(|_| KeyError::Integrity)?)
                    .map_err(|_| KeyError::Integrity)?;
                #[cfg(windows)]
                validate_windows_handle_dacl(&file).map_err(|_| KeyError::Integrity)?;
                Ok(Some(file))
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(_) => Err(KeyError::Integrity),
        }
    }

    pub(crate) fn open_regular(&self, name: &str, access: FileAccess) -> Result<File, KeyError> {
        self.try_open_regular(name, access)?
            .ok_or(KeyError::Integrity)
    }

    pub(crate) fn create_regular(&self, name: &str) -> Result<File, KeyError> {
        self.create_regular_with_sharing(name, false)
    }

    pub(crate) fn create_linkable_regular(&self, name: &str) -> Result<File, KeyError> {
        self.create_regular_with_sharing(name, true)
    }

    fn create_regular_with_sharing(
        &self,
        name: &str,
        allow_delete_share: bool,
    ) -> Result<File, KeyError> {
        let file = open_child_file(
            &self.handle,
            &self.path,
            name,
            FileAccess::ReadWrite,
            true,
            allow_delete_share,
        )
        .map_err(|error| {
            if error.kind() == std::io::ErrorKind::AlreadyExists {
                KeyError::Conflict
            } else {
                KeyError::Storage
            }
        })?;
        #[cfg(windows)]
        apply_windows_protected_dacl(&file)?;
        validate_secure_regular(&file.metadata().map_err(|_| KeyError::Storage)?)
            .map_err(|_| KeyError::Storage)?;
        #[cfg(windows)]
        validate_windows_handle_dacl(&file)?;
        Ok(file)
    }

    pub(crate) fn remove_file_if_exists(&self, name: &str) -> Result<(), KeyError> {
        remove_child_file(&self.handle, &self.path, name)
    }

    pub(crate) fn link_no_replace(&self, source: &str, destination: &str) -> Result<(), KeyError> {
        link_child_file(&self.handle, &self.path, source, destination)
    }
}

#[cfg(windows)]
fn validate_directory_path(directory: &Path) -> Result<PathBuf, KeyError> {
    let absolute = if directory.is_absolute() {
        directory.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|_| KeyError::Storage)?
            .join(directory)
    };
    let mut current = PathBuf::new();
    for component in absolute.components() {
        match component {
            Component::Prefix(prefix) => current.push(prefix.as_os_str()),
            Component::RootDir => current.push(Path::new(std::path::MAIN_SEPARATOR_STR)),
            Component::CurDir => {}
            Component::ParentDir => return Err(KeyError::Storage),
            Component::Normal(part) => {
                current.push(part);
                let metadata = fs::symlink_metadata(&current).map_err(|_| KeyError::Storage)?;
                if is_link_or_reparse(&metadata) {
                    return Err(KeyError::Storage);
                }
            }
        }
    }
    let canonical = fs::canonicalize(&absolute).map_err(|_| KeyError::Storage)?;
    if !canonical.is_absolute() {
        return Err(KeyError::Storage);
    }
    let metadata = fs::symlink_metadata(&canonical).map_err(|_| KeyError::Storage)?;
    if is_link_or_reparse(&metadata) || !metadata.is_dir() {
        return Err(KeyError::Storage);
    }
    Ok(canonical)
}

#[cfg(unix)]
fn open_directory_handle(
    directory: &Path,
    _access: DirectoryAccess,
) -> Result<(PathBuf, File), KeyError> {
    use std::{ffi::CString, os::fd::FromRawFd, os::unix::ffi::OsStrExt};

    let absolute = if directory.is_absolute() {
        directory.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|_| KeyError::Storage)?
            .join(directory)
    };
    let mut normalized = PathBuf::from("/");
    let root = CString::new("/").map_err(|_| KeyError::Storage)?;
    // SAFETY: `root` is NUL-terminated and `open` returns a uniquely owned descriptor
    // on success. The descriptor is immediately transferred into `File`.
    let descriptor = unsafe {
        libc::open(
            root.as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC | libc::O_NOFOLLOW,
        )
    };
    if descriptor < 0 {
        return Err(KeyError::Storage);
    }
    // SAFETY: `descriptor` is nonnegative and ownership has not been transferred yet.
    let mut current = unsafe { File::from_raw_fd(descriptor) };
    for component in absolute.components() {
        match component {
            Component::RootDir | Component::CurDir => {}
            Component::ParentDir | Component::Prefix(_) => return Err(KeyError::Storage),
            Component::Normal(part) => {
                let name = CString::new(part.as_bytes()).map_err(|_| KeyError::Storage)?;
                use std::os::fd::AsRawFd;
                // SAFETY: both descriptors and the NUL-terminated component are live;
                // `O_NOFOLLOW|O_DIRECTORY` prevents selecting a symlink or non-directory.
                let next = unsafe {
                    libc::openat(
                        current.as_raw_fd(),
                        name.as_ptr(),
                        libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC | libc::O_NOFOLLOW,
                    )
                };
                if next < 0 {
                    return Err(KeyError::Storage);
                }
                // SAFETY: `next` is a fresh owned descriptor from successful `openat`.
                current = unsafe { File::from_raw_fd(next) };
                normalized.push(part);
            }
        }
    }
    Ok((normalized, current))
}

#[cfg(windows)]
fn open_directory_handle(
    directory: &Path,
    access: DirectoryAccess,
) -> Result<(PathBuf, File), KeyError> {
    use std::os::windows::fs::OpenOptionsExt;
    use windows_sys::Win32::{
        Foundation::{GENERIC_READ, GENERIC_WRITE},
        Storage::FileSystem::{
            FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_READ,
            FILE_SHARE_WRITE, READ_CONTROL, WRITE_DAC,
        },
    };

    let path = validate_directory_path(directory)?;
    let mut desired = GENERIC_READ | GENERIC_WRITE | READ_CONTROL;
    if matches!(access, DirectoryAccess::Initialize) {
        desired |= WRITE_DAC;
    }
    let file = fs::OpenOptions::new()
        .access_mode(desired)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
        .open(&path)
        .map_err(|_| KeyError::Storage)?;
    Ok((path, file))
}

/// The Unix half of what `create` does on Windows with `apply_windows_protected_dacl`: an EMPTY
/// directory the caller owns is narrowed to `0700` before it is validated, so `mkdir keyring`
/// (umask `0755`) followed by `create` works instead of failing with `KeyError::Storage` (#1305).
///
/// - The mode is changed with `fchmod` on the descriptor `open_directory_handle` already opened
///   component by component with `O_NOFOLLOW`, never on a path: a path swapped after the open is
///   not the one changed, and `verify_root_path` then refuses the swap. Emptiness is read through
///   that same descriptor (see [`directory_is_empty`]), never by path.
/// - Only an empty directory is changed. A directory that already holds anything is somebody's
///   directory, not a fresh keyring: `--keyring ~` must not chmod a home directory. It is left
///   untouched and `validate_secure_directory` refuses it with `KeyError::Storage`, as before
///   #1305. That covers an existing keyring whose directory was loosened after the fact too: it is
///   refused rather than silently repaired, because the loosening may already have exposed it, and
///   `open` keeps the same strict check.
/// - No permission bit is ever added: the new mode is the current mode masked to the owner bits.
///   A directory whose owner bits are not `rwx` (a `0500` one, say) is left untouched and refused.
///   `create` needs to write there, so the only way to make it work would be to grant the owner a
///   bit the owner withheld.
/// - A directory owned by another user is left untouched; `validate_secure_directory` refuses it.
///
/// Returns the metadata read back from the descriptor after the change, which the caller
/// validates as before.
#[cfg(unix)]
fn tighten_owned_directory(handle: &File, metadata: Metadata) -> Result<Metadata, KeyError> {
    use std::os::{fd::AsRawFd, unix::fs::MetadataExt};

    // SAFETY: `geteuid` has no preconditions.
    let effective_uid = unsafe { libc::geteuid() };
    let mode = metadata.mode() & 0o7777;
    let narrowed = mode & 0o700;
    if !metadata.is_dir() || metadata.uid() != effective_uid || mode == 0o700 || narrowed != 0o700 {
        return Ok(metadata);
    }
    if !directory_is_empty(handle)? {
        return Ok(metadata);
    }
    // SAFETY: `handle` owns a live directory descriptor for the duration of the call.
    if unsafe { libc::fchmod(handle.as_raw_fd(), narrowed as libc::mode_t) } != 0 {
        return Err(KeyError::Storage);
    }
    handle.metadata().map_err(|_| KeyError::Storage)
}

/// Whether the directory `root` holds no entry besides `.` and `..`. Read through an independent
/// open file description of `root` itself (`openat(root, ".")`), so the answer is about the
/// directory the descriptor already holds, not about whatever a path names now. A read failure is
/// an error, never an empty directory: `errno` is cleared before every `readdir`, because POSIX
/// returns the same null for the end of the stream and for a failure.
#[cfg(unix)]
fn directory_is_empty(root: &File) -> Result<bool, KeyError> {
    use std::{ffi::CStr, os::fd::AsRawFd};

    // SAFETY: `root` is a live directory descriptor and the name is a NUL-terminated literal.
    let duplicate = unsafe {
        libc::openat(
            root.as_raw_fd(),
            c".".as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC | libc::O_NOFOLLOW,
        )
    };
    if duplicate < 0 {
        return Err(KeyError::Storage);
    }
    // SAFETY: `fdopendir` takes ownership of `duplicate` on success.
    let stream = unsafe { libc::fdopendir(duplicate) };
    if stream.is_null() {
        // SAFETY: `fdopendir` failed and did not take ownership of the descriptor.
        unsafe { libc::close(duplicate) };
        return Err(KeyError::Storage);
    }
    let result = (|| {
        loop {
            // SAFETY: the helper returns this thread's errno cell, or null on an unlisted target.
            let errno = unsafe { unix_errno_location() };
            if errno.is_null() {
                return Err(KeyError::Storage);
            }
            // SAFETY: `errno` is this thread's live errno cell.
            unsafe { *errno = 0 };
            // SAFETY: `stream` stays live until it is closed below.
            let entry = unsafe { libc::readdir(stream) };
            if entry.is_null() {
                // SAFETY: no call intervened between `readdir` and this read of errno.
                return if unsafe { *errno } == 0 {
                    Ok(true)
                } else {
                    Err(KeyError::Storage)
                };
            }
            // SAFETY: `d_name` is NUL-terminated for the live entry `readdir` returned.
            let name = unsafe { CStr::from_ptr((*entry).d_name.as_ptr()) }.to_bytes();
            if !matches!(name, b"." | b"..") {
                return Ok(false);
            }
        }
    })();
    // SAFETY: `stream` is live and closed exactly once; closing it closes `duplicate`.
    if unsafe { libc::closedir(stream) } != 0 {
        return Err(KeyError::Storage);
    }
    result
}

/// This thread's `errno` cell, or null on a target not listed here (the caller then fails
/// closed). A subset of `unix_errno_location` in `core/events/src/local.rs`.
#[cfg(unix)]
#[allow(unreachable_code)]
unsafe fn unix_errno_location() -> *mut libc::c_int {
    #[cfg(any(target_os = "linux", target_os = "dragonfly"))]
    {
        // SAFETY: libc exposes the current thread's errno cell on these targets.
        return unsafe { libc::__errno_location() };
    }
    #[cfg(any(target_os = "android", target_os = "netbsd", target_os = "openbsd"))]
    {
        // SAFETY: libc exposes the current thread's errno cell on these targets.
        return unsafe { libc::__errno() };
    }
    #[cfg(any(target_vendor = "apple", target_os = "freebsd"))]
    {
        // SAFETY: libc exposes the current thread's errno cell on these targets.
        return unsafe { libc::__error() };
    }
    std::ptr::null_mut()
}

fn validate_secure_directory(metadata: &Metadata) -> Result<(), KeyError> {
    if !metadata.is_dir() || is_link_or_reparse(metadata) {
        return Err(KeyError::Storage);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        // SAFETY: `geteuid` has no preconditions and returns the effective identity
        // used by the kernel for this process's filesystem access checks.
        let effective_uid = unsafe { libc::geteuid() };
        if !unix_owner_only_metadata_is_valid(true, metadata.mode(), metadata.uid(), effective_uid)
        {
            return Err(KeyError::Storage);
        }
    }
    Ok(())
}

#[cfg(unix)]
fn file_identity(file: &File) -> Result<FileIdentity, KeyError> {
    use std::os::unix::fs::MetadataExt;
    let metadata = file.metadata().map_err(|_| KeyError::Integrity)?;
    Ok(FileIdentity {
        first: metadata.dev(),
        second: metadata.ino(),
    })
}

#[cfg(windows)]
fn file_identity(file: &File) -> Result<FileIdentity, KeyError> {
    use std::{mem::MaybeUninit, os::windows::io::AsRawHandle};
    use windows_sys::Win32::Storage::FileSystem::{
        BY_HANDLE_FILE_INFORMATION, GetFileInformationByHandle,
    };
    let mut information = MaybeUninit::<BY_HANDLE_FILE_INFORMATION>::zeroed();
    // SAFETY: `file` owns a valid live handle and `information` points to writable storage
    // for the exact structure required by `GetFileInformationByHandle`.
    let succeeded =
        unsafe { GetFileInformationByHandle(file.as_raw_handle(), information.as_mut_ptr()) };
    if succeeded == 0 {
        return Err(KeyError::Integrity);
    }
    // SAFETY: a nonzero return guarantees that Windows initialized the structure.
    let information = unsafe { information.assume_init() };
    Ok(FileIdentity {
        first: u64::from(information.dwVolumeSerialNumber),
        second: (u64::from(information.nFileIndexHigh) << 32)
            | u64::from(information.nFileIndexLow),
    })
}

pub(crate) fn is_link_or_reparse(metadata: &Metadata) -> bool {
    if metadata.file_type().is_symlink() {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        metadata.file_attributes() & 0x0000_0400 != 0
    }
    #[cfg(not(windows))]
    false
}

fn validate_child_name(name: &str) -> Result<(), KeyError> {
    if name.is_empty()
        || name.len() > 128
        || !name.is_ascii()
        || name.contains('/')
        || name.contains('\\')
        || matches!(name, "." | "..")
    {
        Err(KeyError::Invalid)
    } else {
        Ok(())
    }
}

#[cfg(unix)]
fn open_child_file(
    root: &File,
    _path: &Path,
    name: &str,
    access: FileAccess,
    create_new: bool,
    _allow_delete_share: bool,
) -> Result<File, std::io::Error> {
    use std::{
        ffi::CString,
        os::fd::{AsRawFd, FromRawFd},
    };

    validate_child_name(name).map_err(|_| std::io::Error::other("invalid child"))?;
    let name = CString::new(name).map_err(|_| std::io::Error::other("invalid child"))?;
    let mut flags = match access {
        FileAccess::ReadOnly => libc::O_RDONLY,
        FileAccess::ReadWrite => libc::O_RDWR,
    } | libc::O_CLOEXEC
        | libc::O_NOFOLLOW;
    if create_new {
        flags |= libc::O_CREAT | libc::O_EXCL;
    }
    // SAFETY: `root` and `name` are live, the component contains no separator, and a
    // successful descriptor is uniquely owned and transferred immediately to `File`.
    let descriptor = unsafe { libc::openat(root.as_raw_fd(), name.as_ptr(), flags, 0o600) };
    if descriptor < 0 {
        return Err(std::io::Error::last_os_error());
    }
    // SAFETY: `descriptor` is a fresh nonnegative descriptor from `openat`.
    Ok(unsafe { File::from_raw_fd(descriptor) })
}

#[cfg(windows)]
fn open_child_file(
    _root: &File,
    path: &Path,
    name: &str,
    access: FileAccess,
    create_new: bool,
    allow_delete_share: bool,
) -> Result<File, std::io::Error> {
    use std::os::windows::fs::OpenOptionsExt;
    use windows_sys::Win32::{
        Foundation::{GENERIC_READ, GENERIC_WRITE},
        Storage::FileSystem::{
            FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_READ, FILE_SHARE_WRITE, READ_CONTROL,
            WRITE_DAC,
        },
    };

    validate_child_name(name).map_err(|_| std::io::Error::other("invalid child"))?;
    let mut desired = match access {
        FileAccess::ReadOnly => GENERIC_READ | READ_CONTROL,
        FileAccess::ReadWrite => GENERIC_READ | GENERIC_WRITE | READ_CONTROL,
    };
    if create_new {
        desired |= WRITE_DAC;
    }
    let mut options = fs::OpenOptions::new();
    let mut sharing = FILE_SHARE_READ | FILE_SHARE_WRITE;
    if allow_delete_share {
        use windows_sys::Win32::Storage::FileSystem::FILE_SHARE_DELETE;
        sharing |= FILE_SHARE_DELETE;
    }
    options
        .read(true)
        .write(matches!(access, FileAccess::ReadWrite))
        .access_mode(desired)
        .share_mode(sharing)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .create_new(create_new);
    options.open(path.join(name))
}

fn open_or_create_lock(root: &File, path: &Path) -> Result<File, KeyError> {
    match open_child_file(root, path, LOCK_FILE, FileAccess::ReadWrite, false, false) {
        Ok(file) => {
            validate_secure_regular(&file.metadata().map_err(|_| KeyError::Storage)?)
                .map_err(|_| KeyError::Storage)?;
            #[cfg(windows)]
            validate_windows_handle_dacl(&file)?;
            Ok(file)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            match open_child_file(root, path, LOCK_FILE, FileAccess::ReadWrite, true, false) {
                Ok(file) => {
                    #[cfg(windows)]
                    apply_windows_protected_dacl(&file)?;
                    validate_secure_regular(&file.metadata().map_err(|_| KeyError::Storage)?)
                        .map_err(|_| KeyError::Storage)?;
                    #[cfg(windows)]
                    validate_windows_handle_dacl(&file)?;
                    file.sync_all().map_err(|_| KeyError::Storage)?;
                    Ok(file)
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                    let file =
                        open_child_file(root, path, LOCK_FILE, FileAccess::ReadWrite, false, false)
                            .map_err(|_| KeyError::Storage)?;
                    validate_secure_regular(&file.metadata().map_err(|_| KeyError::Storage)?)
                        .map_err(|_| KeyError::Storage)?;
                    #[cfg(windows)]
                    validate_windows_handle_dacl(&file)?;
                    Ok(file)
                }
                Err(_) => Err(KeyError::Storage),
            }
        }
        Err(_) => Err(KeyError::Storage),
    }
}

#[cfg(unix)]
fn named_file_identity(root: &File, _path: &Path, name: &str) -> Result<FileIdentity, KeyError> {
    use std::{ffi::CString, mem::MaybeUninit, os::fd::AsRawFd};
    validate_child_name(name)?;
    let name = CString::new(name).map_err(|_| KeyError::Invalid)?;
    let mut stat = MaybeUninit::<libc::stat>::zeroed();
    // SAFETY: `root`, `name`, and output storage are live; `AT_SYMLINK_NOFOLLOW`
    // ensures identity is read for the directory entry itself.
    if unsafe {
        libc::fstatat(
            root.as_raw_fd(),
            name.as_ptr(),
            stat.as_mut_ptr(),
            libc::AT_SYMLINK_NOFOLLOW,
        )
    } != 0
    {
        return Err(KeyError::Integrity);
    }
    // SAFETY: successful `fstatat` initialized the complete structure.
    let stat = unsafe { stat.assume_init() };
    if stat.st_mode & libc::S_IFMT != libc::S_IFREG || stat.st_mode & 0o077 != 0 {
        return Err(KeyError::Integrity);
    }
    Ok(FileIdentity {
        first: stat.st_dev,
        second: stat.st_ino,
    })
}

#[cfg(windows)]
fn named_file_identity(root: &File, path: &Path, name: &str) -> Result<FileIdentity, KeyError> {
    let file = open_child_file(root, path, name, FileAccess::ReadOnly, false, false)
        .map_err(|_| KeyError::Integrity)?;
    validate_secure_regular(&file.metadata().map_err(|_| KeyError::Integrity)?)
        .map_err(|_| KeyError::Integrity)?;
    validate_windows_handle_dacl(&file).map_err(|_| KeyError::Integrity)?;
    file_identity(&file)
}

#[cfg(unix)]
fn remove_child_file(root: &File, _path: &Path, name: &str) -> Result<(), KeyError> {
    use std::{ffi::CString, os::fd::AsRawFd};

    validate_child_name(name)?;
    let name = CString::new(name).map_err(|_| KeyError::Invalid)?;
    // SAFETY: the root descriptor and fixed NUL-terminated child component are live;
    // `unlinkat` removes only that entry and never follows it.
    if unsafe { libc::unlinkat(root.as_raw_fd(), name.as_ptr(), 0) } == 0 {
        return Ok(());
    }
    let error = std::io::Error::last_os_error();
    if error.kind() == std::io::ErrorKind::NotFound {
        Ok(())
    } else {
        Err(KeyError::Storage)
    }
}

#[cfg(windows)]
fn remove_child_file(_root: &File, path: &Path, name: &str) -> Result<(), KeyError> {
    validate_child_name(name)?;
    let child = path.join(name);
    match fs::symlink_metadata(&child) {
        Ok(metadata) if metadata.is_file() || is_link_or_reparse(&metadata) => {
            fs::remove_file(child).map_err(|_| KeyError::Storage)
        }
        Ok(_) => Err(KeyError::Storage),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err(KeyError::Storage),
    }
}

#[cfg(unix)]
fn link_child_file(
    root: &File,
    _path: &Path,
    source: &str,
    destination: &str,
) -> Result<(), KeyError> {
    use std::{ffi::CString, os::fd::AsRawFd};

    validate_child_name(source)?;
    validate_child_name(destination)?;
    let source = CString::new(source).map_err(|_| KeyError::Invalid)?;
    let destination = CString::new(destination).map_err(|_| KeyError::Invalid)?;
    // SAFETY: both fixed child components and the retained root descriptor are live;
    // zero flags create a hard link without following symlinks or replacing a target.
    if unsafe {
        libc::linkat(
            root.as_raw_fd(),
            source.as_ptr(),
            root.as_raw_fd(),
            destination.as_ptr(),
            0,
        )
    } == 0
    {
        Ok(())
    } else {
        let error = std::io::Error::last_os_error();
        if matches!(
            error.kind(),
            std::io::ErrorKind::AlreadyExists | std::io::ErrorKind::PermissionDenied
        ) {
            Err(KeyError::Conflict)
        } else {
            Err(KeyError::Storage)
        }
    }
}

#[cfg(windows)]
fn link_child_file(
    _root: &File,
    path: &Path,
    source: &str,
    destination: &str,
) -> Result<(), KeyError> {
    validate_child_name(source)?;
    validate_child_name(destination)?;
    fs::hard_link(path.join(source), path.join(destination)).map_err(|error| {
        if matches!(
            error.kind(),
            std::io::ErrorKind::AlreadyExists | std::io::ErrorKind::PermissionDenied
        ) {
            KeyError::Conflict
        } else {
            KeyError::Storage
        }
    })
}

fn validate_secure_regular(metadata: &Metadata) -> Result<(), std::io::Error> {
    if !metadata.is_file() || is_link_or_reparse(metadata) {
        return Err(std::io::Error::other("invalid regular file"));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        // SAFETY: `geteuid` has no preconditions and returns the effective identity
        // used by the kernel for this process's filesystem access checks.
        let effective_uid = unsafe { libc::geteuid() };
        if !unix_owner_only_metadata_is_valid(false, metadata.mode(), metadata.uid(), effective_uid)
        {
            return Err(std::io::Error::other("insecure regular file permissions"));
        }
    }
    Ok(())
}

#[cfg(unix)]
fn unix_owner_only_metadata_is_valid(
    directory: bool,
    mode: u32,
    owner_uid: u32,
    effective_uid: u32,
) -> bool {
    let expected_mode = if directory { 0o700 } else { 0o600 };
    owner_uid == effective_uid && mode & 0o7777 == expected_mode
}

/// Owner, SYSTEM and Administrators only — applied and then read back. The same protection every
/// keyring file receives, exported so the CLI's own secret files (`events.token`, `serve.key`)
/// share this ONE implementation instead of inheriting the directory ACL that any local user can
/// read (PR #1070 review, measured with `icacls`). `file` must be open with `WRITE_DAC` and
/// `READ_CONTROL`.
#[cfg(windows)]
pub fn protect_owner_only(file: &File) -> Result<(), KeyError> {
    apply_windows_protected_dacl(file)?;
    validate_windows_handle_dacl(file)
}

/// The read-back alone: `Ok` when `file` carries the protected owner-only DACL and is owned by
/// the calling user. `file` needs `READ_CONTROL`, which a plain `File::open` grants.
#[cfg(windows)]
pub fn verify_owner_only(file: &File) -> Result<(), KeyError> {
    validate_windows_handle_dacl(file)
}

#[cfg(windows)]
fn apply_windows_protected_dacl(file: &File) -> Result<(), KeyError> {
    use std::{iter, os::windows::io::AsRawHandle, ptr};
    use windows_sys::Win32::Security::{
        Authorization::{
            ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1, SE_FILE_OBJECT,
            SetSecurityInfo,
        },
        DACL_SECURITY_INFORMATION, GetSecurityDescriptorDacl, PROTECTED_DACL_SECURITY_INFORMATION,
        PSECURITY_DESCRIPTOR,
    };

    let sddl = "D:P(A;;FA;;;OW)(A;;FA;;;SY)(A;;FA;;;BA)"
        .encode_utf16()
        .chain(iter::once(0))
        .collect::<Vec<_>>();
    let mut descriptor: PSECURITY_DESCRIPTOR = ptr::null_mut();
    // SAFETY: `sddl` is a valid NUL-terminated descriptor string and `descriptor`
    // points to writable storage for the Windows-owned allocation.
    let converted = unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            sddl.as_ptr(),
            SDDL_REVISION_1,
            &mut descriptor,
            ptr::null_mut(),
        )
    };
    if descriptor.is_null() {
        return Err(KeyError::Storage);
    }
    let descriptor = LocalSecurityDescriptor(descriptor);
    if converted == 0 {
        return Err(KeyError::Storage);
    }
    let mut present = 0;
    let mut defaulted = 0;
    let mut dacl = ptr::null_mut();
    // SAFETY: the descriptor remains owned by `descriptor`, and all output pointers
    // reference valid stack storage for the duration of the call.
    if unsafe { GetSecurityDescriptorDacl(descriptor.0, &mut present, &mut dacl, &mut defaulted) }
        == 0
        || present == 0
        || dacl.is_null()
    {
        return Err(KeyError::Storage);
    }
    // SAFETY: `file` owns a live file/directory handle opened with `WRITE_DAC`; the
    // DACL is owned by the live descriptor, and owner/group/SACL are unchanged.
    let status = unsafe {
        SetSecurityInfo(
            file.as_raw_handle(),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
            ptr::null_mut(),
            ptr::null_mut(),
            dacl,
            ptr::null_mut(),
        )
    };
    if status != 0 {
        return Err(KeyError::Storage);
    }
    validate_windows_handle_dacl(file)
}

#[cfg(windows)]
struct LocalSecurityDescriptor(windows_sys::Win32::Security::PSECURITY_DESCRIPTOR);

#[cfg(windows)]
impl Drop for LocalSecurityDescriptor {
    fn drop(&mut self) {
        use windows_sys::Win32::Foundation::LocalFree;
        // SAFETY: this pointer was allocated by a Windows security-descriptor API
        // documented to transfer ownership to the caller for `LocalFree` cleanup.
        unsafe { LocalFree(self.0.cast()) };
    }
}

#[cfg(windows)]
fn validate_windows_handle_dacl(file: &File) -> Result<(), KeyError> {
    use std::{
        mem::{MaybeUninit, offset_of, size_of},
        os::windows::io::AsRawHandle,
        ptr,
    };
    use windows_sys::Win32::{
        Security::{
            ACCESS_ALLOWED_ACE, ACE_HEADER, ACL, ACL_SIZE_INFORMATION, AclSizeInformation,
            Authorization::{GetSecurityInfo, SE_FILE_OBJECT},
            DACL_SECURITY_INFORMATION, EqualSid, GetAce, GetAclInformation, GetLengthSid,
            GetSecurityDescriptorControl, GetSecurityDescriptorLength, IsValidSid,
            OWNER_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR, PSID, SE_DACL_PROTECTED,
            SECURITY_MAX_SID_SIZE, WinBuiltinAdministratorsSid, WinCreatorOwnerRightsSid,
            WinLocalSystemSid,
        },
        Storage::FileSystem::FILE_ALL_ACCESS,
    };

    let mut dacl: *mut ACL = ptr::null_mut();
    let mut owner: PSID = ptr::null_mut();
    let mut descriptor: PSECURITY_DESCRIPTOR = ptr::null_mut();
    // SAFETY: `file` owns a valid handle; owner, DACL, and descriptor outputs point
    // to writable storage and the returned descriptor is released below.
    let status = unsafe {
        GetSecurityInfo(
            file.as_raw_handle(),
            SE_FILE_OBJECT,
            OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
            &mut owner,
            ptr::null_mut(),
            &mut dacl,
            ptr::null_mut(),
            &mut descriptor,
        )
    };
    if descriptor.is_null() {
        return Err(KeyError::Storage);
    }
    let descriptor = LocalSecurityDescriptor(descriptor);
    if status != 0 || owner.is_null() || dacl.is_null() {
        return Err(KeyError::Storage);
    }
    // SAFETY: `descriptor` is the live self-relative descriptor returned by
    // `GetSecurityInfo`; Windows reports its complete allocation length.
    let descriptor_length = usize::try_from(unsafe { GetSecurityDescriptorLength(descriptor.0) })
        .map_err(|_| KeyError::Storage)?;
    let owner_sid = copy_bounded_sid(owner, descriptor.0.cast_const().cast(), descriptor_length)?;
    let token_user_sid = current_process_token_user_sid()?;
    if !windows_owner_sid_matches(&owner_sid, &token_user_sid) {
        return Err(KeyError::Storage);
    }
    let mut control = 0_u16;
    let mut revision = 0_u32;
    // SAFETY: `descriptor` is live and both output pointers are writable.
    if unsafe { GetSecurityDescriptorControl(descriptor.0, &mut control, &mut revision) } == 0
        || control & SE_DACL_PROTECTED == 0
    {
        return Err(KeyError::Storage);
    }
    let mut information = MaybeUninit::<ACL_SIZE_INFORMATION>::zeroed();
    // SAFETY: `dacl` belongs to the live descriptor and the output buffer has the
    // exact size required for `AclSizeInformation`.
    if unsafe {
        GetAclInformation(
            dacl,
            information.as_mut_ptr().cast(),
            u32::try_from(size_of::<ACL_SIZE_INFORMATION>()).map_err(|_| KeyError::Storage)?,
            AclSizeInformation,
        )
    } == 0
    {
        return Err(KeyError::Storage);
    }
    // SAFETY: successful `GetAclInformation` initialized the structure.
    let information = unsafe { information.assume_init() };
    if information.AceCount != 3 {
        return Err(KeyError::Storage);
    }
    let expected = [
        well_known_sid(WinCreatorOwnerRightsSid)?,
        well_known_sid(WinLocalSystemSid)?,
        well_known_sid(WinBuiltinAdministratorsSid)?,
    ];
    let mut seen = [false; 3];
    for index in 0..information.AceCount {
        let mut ace = ptr::null_mut();
        // SAFETY: `dacl` is a validated OS-owned ACL and `index` is below AceCount.
        if unsafe { GetAce(dacl, index, &mut ace) } == 0 || ace.is_null() {
            return Err(KeyError::Storage);
        }
        // SAFETY: every ACE begins with `ACE_HEADER`, and the OS-owned DACL remains
        // live. The declared size is checked before casting to the larger shape.
        let header = unsafe { &*(ace.cast::<ACE_HEADER>()) };
        let ace_size = usize::from(header.AceSize);
        let sid_offset = offset_of!(ACCESS_ALLOWED_ACE, SidStart);
        let minimum_sid_header = 8_usize;
        if header.AceType != 0
            || ace_size < sid_offset.saturating_add(minimum_sid_header)
            || header.AceFlags != 0
        {
            return Err(KeyError::Storage);
        }
        // SAFETY: the header established the allowed ACE type and sufficient size.
        let allowed = unsafe { &*(ace.cast::<ACCESS_ALLOWED_ACE>()) };
        if allowed.Mask != FILE_ALL_ACCESS {
            return Err(KeyError::Storage);
        }
        let sid = ptr::addr_of!(allowed.SidStart).cast_mut().cast();
        // SAFETY: the ACE-size check above proves that the complete fixed SID header is
        // inside the ACE, so Windows may validate it and derive its bounded length.
        if unsafe { IsValidSid(sid) } == 0 {
            return Err(KeyError::Storage);
        }
        // SAFETY: `IsValidSid` accepted this SID while its descriptor-owned ACE is live.
        let sid_length =
            usize::try_from(unsafe { GetLengthSid(sid) }).map_err(|_| KeyError::Storage)?;
        if sid_length > usize::try_from(SECURITY_MAX_SID_SIZE).map_err(|_| KeyError::Storage)?
            || sid_offset
                .checked_add(sid_length)
                .is_none_or(|required| required > ace_size)
        {
            return Err(KeyError::Storage);
        }
        let match_index = expected.iter().position(|expected_sid| {
            // SAFETY: both SIDs come from validated Windows ACL/SID APIs and remain
            // live for the comparison.
            unsafe { EqualSid(sid, expected_sid.as_ptr().cast_mut().cast()) != 0 }
        });
        let match_index = match_index.ok_or(KeyError::Storage)?;
        if seen[match_index] {
            return Err(KeyError::Storage);
        }
        seen[match_index] = true;
    }
    if seen.into_iter().all(|entry| entry) {
        Ok(())
    } else {
        Err(KeyError::Storage)
    }
}

#[cfg(windows)]
fn windows_owner_sid_matches(owner_sid: &[u8], token_user_sid: &[u8]) -> bool {
    owner_sid == token_user_sid
}

#[cfg(windows)]
fn current_process_token_user_sid() -> Result<Vec<u8>, KeyError> {
    use std::{
        mem::size_of,
        os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle},
        ptr,
    };
    use windows_sys::Win32::{
        Security::{GetTokenInformation, TOKEN_QUERY, TOKEN_USER, TokenUser},
        System::Threading::{GetCurrentProcess, OpenProcessToken},
    };

    const MAX_TOKEN_INFORMATION_BYTES: u32 = 64 * 1024;
    let mut token = ptr::null_mut();
    // SAFETY: `GetCurrentProcess` returns the current-process pseudo-handle and
    // `token` points to writable storage for the newly opened token handle.
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0
        || token.is_null()
    {
        return Err(KeyError::Storage);
    }
    // SAFETY: successful `OpenProcessToken` returned a uniquely owned real handle.
    let token = unsafe { OwnedHandle::from_raw_handle(token) };
    let mut required = 0_u32;
    // SAFETY: this size query intentionally supplies no output buffer and gives
    // Windows a writable length output.
    unsafe {
        GetTokenInformation(
            token.as_raw_handle(),
            TokenUser,
            ptr::null_mut(),
            0,
            &mut required,
        );
    }
    if required < u32::try_from(size_of::<TOKEN_USER>()).map_err(|_| KeyError::Storage)?
        || required > MAX_TOKEN_INFORMATION_BYTES
    {
        return Err(KeyError::Storage);
    }
    let required_usize = usize::try_from(required).map_err(|_| KeyError::Storage)?;
    let word_count = required_usize.div_ceil(size_of::<u64>());
    let mut buffer = vec![0_u64; word_count];
    let mut returned = required;
    // SAFETY: the aligned buffer has at least `required` bytes and remains live;
    // `returned` is writable and the token handle is owned for the whole call.
    if unsafe {
        GetTokenInformation(
            token.as_raw_handle(),
            TokenUser,
            buffer.as_mut_ptr().cast(),
            required,
            &mut returned,
        )
    } == 0
        || returned < u32::try_from(size_of::<TOKEN_USER>()).map_err(|_| KeyError::Storage)?
        || returned > required
    {
        return Err(KeyError::Storage);
    }
    // SAFETY: the successful call initialized at least one aligned `TOKEN_USER`.
    let token_user = unsafe { &*buffer.as_ptr().cast::<TOKEN_USER>() };
    copy_bounded_sid(
        token_user.User.Sid,
        buffer.as_ptr().cast(),
        usize::try_from(returned).map_err(|_| KeyError::Storage)?,
    )
}

#[cfg(windows)]
fn copy_bounded_sid(
    sid: windows_sys::Win32::Security::PSID,
    region: *const u8,
    region_length: usize,
) -> Result<Vec<u8>, KeyError> {
    use windows_sys::Win32::Security::{GetLengthSid, IsValidSid, SECURITY_MAX_SID_SIZE};

    const SID_HEADER_BYTES: usize = 8;
    if sid.is_null() || region.is_null() {
        return Err(KeyError::Storage);
    }
    let region_start = region as usize;
    let region_end = region_start
        .checked_add(region_length)
        .ok_or(KeyError::Storage)?;
    let sid_start = sid as usize;
    let sid_header_end = sid_start
        .checked_add(SID_HEADER_BYTES)
        .ok_or(KeyError::Storage)?;
    if sid_start < region_start || sid_header_end > region_end {
        return Err(KeyError::Storage);
    }
    // SAFETY: the fixed SID header lies inside the live region supplied by a
    // successful Windows security API, so Windows may validate the structure.
    if unsafe { IsValidSid(sid) } == 0 {
        return Err(KeyError::Storage);
    }
    // SAFETY: `IsValidSid` accepted the SID while its backing region remains live.
    let sid_length =
        usize::try_from(unsafe { GetLengthSid(sid) }).map_err(|_| KeyError::Storage)?;
    let sid_end = sid_start.checked_add(sid_length).ok_or(KeyError::Storage)?;
    if sid_length > usize::try_from(SECURITY_MAX_SID_SIZE).map_err(|_| KeyError::Storage)?
        || sid_end > region_end
    {
        return Err(KeyError::Storage);
    }
    // SAFETY: the validated SID range lies wholly inside the live backing region.
    Ok(unsafe { std::slice::from_raw_parts(sid.cast(), sid_length) }.to_vec())
}

#[cfg(windows)]
fn well_known_sid(
    kind: windows_sys::Win32::Security::WELL_KNOWN_SID_TYPE,
) -> Result<Vec<u32>, KeyError> {
    use windows_sys::Win32::Security::{CreateWellKnownSid, SECURITY_MAX_SID_SIZE};

    let word_count =
        usize::try_from(SECURITY_MAX_SID_SIZE.div_ceil(4)).map_err(|_| KeyError::Storage)?;
    let mut words = vec![0_u32; word_count];
    let mut length = SECURITY_MAX_SID_SIZE;
    // SAFETY: the output buffer is `SECURITY_MAX_SID_SIZE` bytes and `length`
    // advertises exactly that capacity; no domain SID is required for these types.
    if unsafe {
        CreateWellKnownSid(
            kind,
            std::ptr::null_mut(),
            words.as_mut_ptr().cast(),
            &mut length,
        )
    } == 0
    {
        return Err(KeyError::Storage);
    }
    Ok(words)
}

fn with_derived_key<R>(
    key_material: &SecretBytes,
    domain: &[u8],
    operation: impl FnOnce(&[u8]) -> Result<R, KeyError>,
) -> Result<R, KeyError> {
    let derived = key_material.expose(|material| {
        let mut bytes = Vec::with_capacity(64 + domain.len());
        bytes.extend_from_slice(b"graphhelm-subkey-derivation-v1");
        bytes.extend_from_slice(&(domain.len() as u64).to_be_bytes());
        bytes.extend_from_slice(domain);
        hmac_sha256(material, &bytes)
    })?;
    operation(derived.as_slice())
}

fn authentication_bytes(
    key_material: &SecretBytes,
    purpose: &str,
    bytes: &[u8],
) -> Result<Vec<u8>, KeyError> {
    let authenticated = authentication_aad(purpose, bytes)?;
    with_derived_key(key_material, b"graphhelm-authentication-key-v1", |key| {
        Ok(hmac_sha256(key, &authenticated)?.to_vec())
    })
}

fn verify_authentication_bytes(
    key_material: &SecretBytes,
    purpose: &str,
    bytes: &[u8],
    tag: &[u8],
) -> Result<(), KeyError> {
    if tag.len() != 32 {
        return Err(KeyError::Integrity);
    }
    let authenticated = authentication_aad(purpose, bytes)?;
    with_derived_key(key_material, b"graphhelm-authentication-key-v1", |key| {
        let expected = hmac_sha256(key, &authenticated)?;
        if constant_time_eq(expected.as_slice(), tag) {
            Ok(())
        } else {
            Err(KeyError::Integrity)
        }
    })
}

fn authentication_aad(purpose: &str, bytes: &[u8]) -> Result<Vec<u8>, KeyError> {
    let mut aad = Vec::with_capacity(purpose.len() + bytes.len() + 32);
    push_field(&mut aad, b"graphhelm-authentication-v1")?;
    push_field(&mut aad, purpose.as_bytes())?;
    push_field(&mut aad, bytes)?;
    Ok(aad)
}

fn wrapping_aad(handle: &str, aad_sha256: &RawSha256) -> Result<Vec<u8>, KeyError> {
    let mut aad = Vec::with_capacity(256);
    push_field(&mut aad, b"graphhelm-wrapped-dek-v1")?;
    push_field(&mut aad, handle.as_bytes())?;
    push_field(&mut aad, aad_sha256.as_str().as_bytes())?;
    Ok(aad)
}

fn push_field(output: &mut Vec<u8>, bytes: &[u8]) -> Result<(), KeyError> {
    let length = u32::try_from(bytes.len()).map_err(|_| KeyError::Invalid)?;
    output.extend_from_slice(&length.to_be_bytes());
    output.extend_from_slice(bytes);
    Ok(())
}

fn raw_sha256(bytes: &[u8]) -> RawSha256 {
    RawSha256::parse(hex::encode(Sha256::digest(bytes)))
        .expect("SHA-256 output is always lowercase hexadecimal")
}

fn hmac_sha256(key: &[u8], bytes: &[u8]) -> Result<Zeroizing<[u8; 32]>, KeyError> {
    // RFC 2104 section 2 prescribes hashing a key longer than the block size down to the block
    // size. Rejecting it instead was a silent divergence from the standard: every current caller
    // passes a 32-byte derived key, so nothing failed, but a future caller with a longer key would
    // have seen `Invalid` rather than a correct tag.
    let mut shortened = Zeroizing::new([0_u8; 32]);
    let key = if key.len() > 64 {
        let mut hash = Sha256::new();
        hash.update(key);
        *shortened = *finalize_secret_sha256(hash);
        shortened.as_slice()
    } else {
        key
    };
    let mut inner_pad = Zeroizing::new(vec![0x36_u8; 64]);
    let mut outer_pad = Zeroizing::new(vec![0x5c_u8; 64]);
    for (index, byte) in key.iter().copied().enumerate() {
        inner_pad[index] ^= byte;
        outer_pad[index] ^= byte;
    }
    let mut inner = Sha256::new();
    inner.update(inner_pad.as_slice());
    inner.update(bytes);
    let inner_digest = finalize_secret_sha256(inner);
    let mut outer = Sha256::new();
    outer.update(outer_pad.as_slice());
    outer.update(inner_digest.as_slice());
    Ok(finalize_secret_sha256(outer))
}

fn finalize_secret_sha256(hash: Sha256) -> Zeroizing<[u8; 32]> {
    let mut output = Zeroizing::new([0_u8; 32]);
    let output_array: &mut sha2::digest::Output<Sha256> = (&mut *output).into();
    hash.finalize_into(output_array);
    output
}

fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    left.iter()
        .zip(right)
        .fold(0_u8, |difference, (left, right)| {
            difference | (left ^ right)
        })
        == 0
}

pub(crate) fn authenticate_internal(
    key_material: &SecretBytes,
    purpose: &str,
    bytes: &[u8],
) -> Result<Vec<u8>, KeyError> {
    authentication_bytes(key_material, purpose, bytes)
}

pub(crate) fn verify_internal(
    key_material: &SecretBytes,
    purpose: &str,
    bytes: &[u8],
    tag: &[u8],
) -> Result<(), KeyError> {
    verify_authentication_bytes(key_material, purpose, bytes, tag)
}

pub(crate) fn durable_sync_directory(directory: &AnchoredDirectory) -> Result<(), KeyError> {
    directory.sync()
}

#[cfg(test)]
mod tests {
    use std::{
        cell::Cell,
        fs,
        future::Future,
        io::{Seek, SeekFrom, Write},
        path::Path,
        pin::Pin,
        rc::Rc,
        sync::Arc,
        task::{Context, Poll, Wake, Waker},
        thread,
    };

    use graphhelm_events::{
        AuthenticateRequest, KeyError, KeyProvider, RevokeKeyRequest, SecretBytes,
        VerifyAuthenticationRequest, WrapKeyRequest,
    };
    use sha2::{Digest, Sha256};
    use tempfile::TempDir;
    use zeroize::Zeroizing;

    use super::{
        SealedKeyProvider, TestRacePoint, finalize_secret_sha256, install_test_race_hook, journal,
    };

    const JOURNAL_FILE: &str = journal::JOURNAL_FILE;

    fn key_material(seed: u8) -> Vec<u8> {
        (0..32).map(|offset| seed.wrapping_add(offset)).collect()
    }

    fn secure_directory(_path: &Path) {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(_path, fs::Permissions::from_mode(0o700)).unwrap();
        }
    }

    fn create_provider(path: &Path, seed: u8) -> SealedKeyProvider {
        SealedKeyProvider::create(path, "local-key-v1", SecretBytes::new(key_material(seed)))
            .unwrap()
    }

    fn overwrite_live_journal_in_place(provider: &SealedKeyProvider, bytes: &[u8]) {
        let mut live_journal = provider.journal.lock().unwrap();
        let identity_before = super::file_identity(&live_journal.file).unwrap();
        live_journal.file.set_len(0).unwrap();
        live_journal.file.seek(SeekFrom::Start(0)).unwrap();
        live_journal.file.write_all(bytes).unwrap();
        live_journal.file.flush().unwrap();
        live_journal.file.sync_all().unwrap();
        assert!(super::file_identity(&live_journal.file).unwrap() == identity_before);
        assert!(
            super::named_file_identity(
                &provider.directory.handle,
                &provider.directory.path,
                JOURNAL_FILE,
            )
            .unwrap()
                == identity_before
        );
    }

    /// Simulates a crash between `write_all` and `sync_all`: a partial record reaches the file
    /// without its terminating newline, so it was never acknowledged to any caller.
    fn append_torn_tail(provider: &SealedKeyProvider, partial: &[u8]) {
        let mut live_journal = provider.journal.lock().unwrap();
        live_journal.file.seek(SeekFrom::End(0)).unwrap();
        live_journal.file.write_all(partial).unwrap();
        live_journal.file.flush().unwrap();
        live_journal.file.sync_all().unwrap();
    }

    fn journal_bytes(provider: &SealedKeyProvider) -> Vec<u8> {
        fs::read(provider.directory.path.join(JOURNAL_FILE)).unwrap()
    }

    /// An interrupted append must not brick the provider. The torn record was never durable, so
    /// discarding it loses nothing that was ever reported as committed.
    #[test]
    fn interrupted_revocation_tail_is_discarded_and_the_provider_stays_usable() {
        let directory = TempDir::new().unwrap();
        secure_directory(directory.path());
        let provider = create_provider(directory.path(), 11);
        block_on(
            provider
                .revoke(RevokeKeyRequest::new("handle-committed", "operation-committed").unwrap()),
        )
        .unwrap();
        let committed = journal_bytes(&provider);
        assert_eq!(provider.epoch().unwrap(), 1);

        append_torn_tail(&provider, b"{\"epoch\":2,\"handle\":\"handle-torn\"");

        // Reading state must succeed and report only the committed epoch.
        assert_eq!(provider.epoch().unwrap(), 1);
        // The uncommitted bytes must be gone, so a later append writes at the right offset.
        assert_eq!(journal_bytes(&provider), committed);
        // And the journal must still accept new records.
        block_on(
            provider.revoke(RevokeKeyRequest::new("handle-after", "operation-after").unwrap()),
        )
        .unwrap();
        assert_eq!(provider.epoch().unwrap(), 2);
    }

    /// Truncation applies only to an unterminated tail. Corruption inside a committed record is
    /// adversarial and must still fail closed.
    #[test]
    fn corruption_inside_a_committed_record_still_fails_closed() {
        let directory = TempDir::new().unwrap();
        secure_directory(directory.path());
        let provider = create_provider(directory.path(), 13);
        block_on(
            provider
                .revoke(RevokeKeyRequest::new("handle-committed", "operation-committed").unwrap()),
        )
        .unwrap();

        let mut bytes = journal_bytes(&provider);
        let last_newline = bytes.iter().rposition(|byte| *byte == b'\n').unwrap();
        let target = bytes[..last_newline]
            .iter()
            .rposition(|byte| byte.is_ascii_alphanumeric())
            .unwrap();
        bytes[target] ^= 0x01;
        overwrite_live_journal_in_place(&provider, &bytes);

        assert_eq!(provider.epoch(), Err(KeyError::Integrity));
    }

    /// A journal whose header never completed has no committed prefix at all.
    #[test]
    fn a_journal_with_no_terminated_line_is_rejected() {
        let directory = TempDir::new().unwrap();
        secure_directory(directory.path());
        let provider = create_provider(directory.path(), 17);
        overwrite_live_journal_in_place(&provider, b"{\"formatVersion\":1,\"keyId\":\"local-key");

        assert_eq!(provider.epoch(), Err(KeyError::Integrity));
    }

    /// The HMAC is hand-rolled rather than taken from a crate, so it is pinned to the standard's
    /// own vectors. Case 6 covers a key longer than the 64-byte block, which RFC 2104 requires be
    /// hashed down rather than rejected.
    #[test]
    fn hmac_matches_rfc_4231_vectors_including_an_over_long_key() {
        fn hex(bytes: &[u8]) -> String {
            bytes.iter().map(|byte| format!("{byte:02x}")).collect()
        }

        let tag = super::hmac_sha256(b"Jefe", b"what do ya want for nothing?").unwrap();
        assert_eq!(
            hex(&tag[..]),
            "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843"
        );

        let over_long = vec![0xaa_u8; 131];
        let tag = super::hmac_sha256(
            &over_long,
            b"Test Using Larger Than Block-Size Key - Hash Key First",
        )
        .unwrap();
        assert_eq!(
            hex(&tag[..]),
            "60e431591ee0b67f0d8a26aacbf5b77f8e0bc6213728c5140546040f0ee37f54"
        );
    }

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
            match Pin::new(&mut future).poll(&mut context) {
                Poll::Ready(output) => return output,
                Poll::Pending => thread::park(),
            }
        }
    }

    #[test]
    fn authenticated_journal_regression_is_rejected_for_the_lifetime_of_an_open_provider() {
        let directory = TempDir::new().unwrap();
        let provider = create_provider(directory.path(), 104);
        let initial_journal = fs::read(directory.path().join(JOURNAL_FILE)).unwrap();
        let wrapped = block_on(
            provider.wrap(
                WrapKeyRequest::new(
                    "handle-live-floor",
                    SecretBytes::new(key_material(105)),
                    b"evidence-aad-v1".to_vec(),
                )
                .unwrap(),
            ),
        )
        .unwrap();
        let authentication = block_on(provider.authenticate(
            AuthenticateRequest::new("checkpoint", b"authenticated-state".to_vec()).unwrap(),
        ))
        .unwrap();

        block_on(
            provider.revoke(
                RevokeKeyRequest::new("handle-live-floor", "operation-live-floor").unwrap(),
            ),
        )
        .unwrap();
        assert!(matches!(
            block_on(provider.unwrap(wrapped.clone())),
            Err(KeyError::Unavailable)
        ));

        overwrite_live_journal_in_place(&provider, &initial_journal);

        assert_eq!(provider.epoch(), Err(KeyError::Integrity));
        assert!(matches!(
            block_on(
                provider.wrap(
                    WrapKeyRequest::new(
                        "handle-new",
                        SecretBytes::new(key_material(106)),
                        b"evidence-aad-v1".to_vec(),
                    )
                    .unwrap(),
                )
            ),
            Err(KeyError::Integrity)
        ));
        assert!(matches!(
            block_on(provider.unwrap(wrapped)),
            Err(KeyError::Integrity)
        ));
        assert!(matches!(
            block_on(provider.authenticate(
                AuthenticateRequest::new("checkpoint", b"new-state".to_vec()).unwrap(),
            )),
            Err(KeyError::Integrity)
        ));
        assert_eq!(
            block_on(
                provider.verify(
                    VerifyAuthenticationRequest::new(
                        "checkpoint",
                        b"authenticated-state".to_vec(),
                        authentication,
                    )
                    .unwrap(),
                )
            ),
            Err(KeyError::Integrity)
        );
        assert!(matches!(
            block_on(
                provider.revoke(RevokeKeyRequest::new("handle-new", "operation-new").unwrap(),)
            ),
            Err(KeyError::Integrity)
        ));
    }

    #[test]
    fn authenticated_journal_regression_is_shared_across_open_providers() {
        let directory = TempDir::new().unwrap();
        let provider_a = create_provider(directory.path(), 110);
        let provider_b = SealedKeyProvider::open(
            directory.path(),
            "local-key-v1",
            SecretBytes::new(key_material(110)),
        )
        .unwrap();
        let initial_journal = fs::read(directory.path().join(JOURNAL_FILE)).unwrap();
        let wrapped = block_on(
            provider_a.wrap(
                WrapKeyRequest::new(
                    "handle-shared-floor",
                    SecretBytes::new(key_material(111)),
                    b"evidence-aad-v1".to_vec(),
                )
                .unwrap(),
            ),
        )
        .unwrap();
        let authentication = block_on(provider_a.authenticate(
            AuthenticateRequest::new("checkpoint", b"authenticated-state".to_vec()).unwrap(),
        ))
        .unwrap();

        block_on(provider_b.revoke(
            RevokeKeyRequest::new("handle-shared-floor", "operation-shared-floor").unwrap(),
        ))
        .unwrap();
        assert_eq!(provider_b.epoch().unwrap(), 1);

        overwrite_live_journal_in_place(&provider_a, &initial_journal);

        assert_eq!(provider_a.epoch(), Err(KeyError::Integrity));
        assert!(matches!(
            block_on(provider_a.metadata()),
            Err(KeyError::Integrity)
        ));
        assert!(matches!(
            block_on(
                provider_a.wrap(
                    WrapKeyRequest::new(
                        "handle-new-shared-floor",
                        SecretBytes::new(key_material(112)),
                        b"evidence-aad-v1".to_vec(),
                    )
                    .unwrap(),
                )
            ),
            Err(KeyError::Integrity)
        ));
        assert!(matches!(
            block_on(provider_a.unwrap(wrapped)),
            Err(KeyError::Integrity)
        ));
        assert!(matches!(
            block_on(provider_a.authenticate(
                AuthenticateRequest::new("checkpoint", b"new-state".to_vec()).unwrap(),
            )),
            Err(KeyError::Integrity)
        ));
        assert_eq!(
            block_on(
                provider_a.verify(
                    VerifyAuthenticationRequest::new(
                        "checkpoint",
                        b"authenticated-state".to_vec(),
                        authentication,
                    )
                    .unwrap(),
                )
            ),
            Err(KeyError::Integrity)
        );
        assert!(matches!(
            block_on(
                provider_a.revoke(
                    RevokeKeyRequest::new("handle-new-shared-floor", "operation-new-shared-floor")
                        .unwrap(),
                )
            ),
            Err(KeyError::Integrity)
        ));
    }

    #[test]
    fn shared_floor_registry_releases_the_last_provider_entry() {
        let directory = TempDir::new().unwrap();
        let provider_a = create_provider(directory.path(), 113);
        let identity = provider_a.shared_floor.identity.clone();
        let provider_b = SealedKeyProvider::open(
            directory.path(),
            "local-key-v1",
            SecretBytes::new(key_material(113)),
        )
        .unwrap();

        {
            let registry = super::shared_floor_registry().lock().unwrap();
            assert_eq!(registry.get(&identity).unwrap().strong_count(), 2);
        }
        drop(provider_a);
        {
            let registry = super::shared_floor_registry().lock().unwrap();
            assert_eq!(registry.get(&identity).unwrap().strong_count(), 1);
        }
        drop(provider_b);
        assert!(
            !super::shared_floor_registry()
                .lock()
                .unwrap()
                .contains_key(&identity)
        );
    }

    #[test]
    fn authenticated_journal_divergence_at_the_live_epoch_is_rejected() {
        let directory = TempDir::new().unwrap();
        let provider = create_provider(directory.path(), 107);
        block_on(
            provider
                .revoke(RevokeKeyRequest::new("handle-accepted", "operation-accepted").unwrap()),
        )
        .unwrap();

        let divergent_directory = TempDir::new().unwrap();
        let divergent_provider = create_provider(divergent_directory.path(), 107);
        block_on(
            divergent_provider
                .revoke(RevokeKeyRequest::new("handle-divergent", "operation-divergent").unwrap()),
        )
        .unwrap();
        let divergent_journal = fs::read(divergent_directory.path().join(JOURNAL_FILE)).unwrap();

        overwrite_live_journal_in_place(&provider, &divergent_journal);

        assert_eq!(provider.epoch(), Err(KeyError::Integrity));
    }

    #[test]
    fn sha256_output_is_zeroizing_before_any_copy_or_use() {
        let mut hash = Sha256::new();
        hash.update(b"kek-derived-intermediate");

        let digest: Zeroizing<[u8; 32]> = finalize_secret_sha256(hash);

        assert_eq!(digest.len(), 32);
    }

    #[cfg(unix)]
    #[test]
    fn unix_owner_only_predicate_requires_the_effective_user() {
        assert!(super::unix_owner_only_metadata_is_valid(
            true, 0o700, 1000, 1000
        ));
        assert!(super::unix_owner_only_metadata_is_valid(
            false, 0o600, 1000, 1000
        ));
        assert!(!super::unix_owner_only_metadata_is_valid(
            true, 0o700, 1001, 1000
        ));
        assert!(!super::unix_owner_only_metadata_is_valid(
            false, 0o600, 1001, 1000
        ));
    }

    #[cfg(windows)]
    #[test]
    fn windows_owner_predicate_rejects_a_different_token_user_sid() {
        assert!(super::windows_owner_sid_matches(
            &[1, 2, 3, 4],
            &[1, 2, 3, 4]
        ));
        assert!(!super::windows_owner_sid_matches(
            &[1, 2, 3, 4],
            &[1, 2, 3, 5]
        ));
        assert!(!super::windows_owner_sid_matches(&[1, 2, 3, 4], &[1, 2, 3]));
    }

    #[test]
    fn transient_root_swap_cannot_receive_provider_writes() {
        let parent = TempDir::new().unwrap();
        let root = parent.path().join("provider-root");
        let attacker = parent.path().join("attacker-root");
        let displaced = parent.path().join("displaced-root");
        fs::create_dir(&root).unwrap();
        fs::create_dir(&attacker).unwrap();
        secure_directory(&root);
        secure_directory(&attacker);
        let provider = create_provider(&root, 101);
        for name in [".sealed-key-provider.lock", "keyring.v1.json", JOURNAL_FILE] {
            fs::copy(root.join(name), attacker.join(name)).unwrap();
        }
        let attacker_before = fs::read(attacker.join(JOURNAL_FILE)).unwrap();
        let swapped = Rc::new(Cell::new(false));
        let replacement_denied = Rc::new(Cell::new(false));
        let hook_swapped = Rc::clone(&swapped);
        let hook_denied = Rc::clone(&replacement_denied);
        let hook_root = root.clone();
        let hook_attacker = attacker.clone();
        let hook_displaced = displaced.clone();
        let _hook = install_test_race_hook(move |point| match point {
            TestRacePoint::BeforePathIo => {
                if fs::rename(&hook_root, &hook_displaced).is_err() {
                    hook_denied.set(true);
                    return;
                }
                fs::rename(&hook_attacker, &hook_root).unwrap();
                hook_swapped.set(true);
            }
            TestRacePoint::AfterPathIo if hook_swapped.replace(false) => {
                fs::rename(&hook_root, &hook_attacker).unwrap();
                fs::rename(&hook_displaced, &hook_root).unwrap();
            }
            TestRacePoint::AfterPathIo => {}
        });

        let result = provider.with_locked_state(|journal_file, state| {
            journal::append_revocation(
                journal_file,
                &provider.directory,
                provider.key_id.as_str(),
                &provider.key_material,
                state,
                RevokeKeyRequest::new("handle-root-race", "operation-root-race").unwrap(),
            )
        });

        assert!(result.is_ok() || replacement_denied.get());
        assert_eq!(
            fs::read(attacker.join(JOURNAL_FILE)).unwrap(),
            attacker_before
        );
    }

    #[test]
    fn transient_journal_rollback_cannot_hide_a_revocation() {
        let directory = TempDir::new().unwrap();
        let provider = create_provider(directory.path(), 102);
        let journal_path = directory.path().join(JOURNAL_FILE);
        let rollback_path = directory.path().join("rollback-journal");
        let displaced_path = directory.path().join("current-journal");
        fs::copy(&journal_path, &rollback_path).unwrap();
        let rollback_before = fs::read(&rollback_path).unwrap();
        provider
            .with_locked_state(|journal_file, state| {
                journal::append_revocation(
                    journal_file,
                    &provider.directory,
                    provider.key_id.as_str(),
                    &provider.key_material,
                    state,
                    RevokeKeyRequest::new("handle-journal-race", "operation-journal-race").unwrap(),
                )
            })
            .unwrap();

        let swapped = Rc::new(Cell::new(false));
        let replacement_denied = Rc::new(Cell::new(false));
        let hook_swapped = Rc::clone(&swapped);
        let hook_denied = Rc::clone(&replacement_denied);
        let hook_journal = journal_path.clone();
        let hook_rollback = rollback_path.clone();
        let hook_displaced = displaced_path.clone();
        let _hook = install_test_race_hook(move |point| match point {
            TestRacePoint::BeforePathIo => {
                if fs::rename(&hook_journal, &hook_displaced).is_err() {
                    hook_denied.set(true);
                    return;
                }
                fs::rename(&hook_rollback, &hook_journal).unwrap();
                hook_swapped.set(true);
            }
            TestRacePoint::AfterPathIo if hook_swapped.replace(false) => {
                fs::rename(&hook_journal, &hook_rollback).unwrap();
                fs::rename(&hook_displaced, &hook_journal).unwrap();
            }
            TestRacePoint::AfterPathIo => {}
        });

        let revoked =
            provider.with_locked_state(|_, state| Ok(state.is_revoked("handle-journal-race")));

        #[cfg(unix)]
        assert_eq!(revoked, Err(KeyError::Integrity));
        #[cfg(windows)]
        assert!(replacement_denied.get() || revoked == Ok(true));
        assert_eq!(fs::read(&rollback_path).unwrap(), rollback_before);
    }

    #[cfg(unix)]
    #[test]
    fn transient_lock_swap_cannot_create_a_split_writer() {
        use std::{fs::OpenOptions, os::unix::fs::OpenOptionsExt};

        use fs2::FileExt;

        let directory = TempDir::new().unwrap();
        let provider = create_provider(directory.path(), 103);
        let lock_path = directory.path().join(super::LOCK_FILE);
        let displaced = directory.path().join("displaced-lock");
        let blocked = Rc::new(Cell::new(false));
        let hook_blocked = Rc::clone(&blocked);
        let root = directory.path().to_owned();
        let _hook = install_test_race_hook(move |point| {
            if point != TestRacePoint::BeforePathIo || hook_blocked.get() {
                return;
            }
            fs::rename(&lock_path, &displaced).unwrap();
            OpenOptions::new()
                .read(true)
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&lock_path)
                .unwrap();
            let competing =
                super::AnchoredDirectory::open(&root, super::DirectoryAccess::Existing).unwrap();
            hook_blocked.set(competing.handle.try_lock_exclusive().is_err());
            drop(competing);
            fs::remove_file(&lock_path).unwrap();
            fs::rename(&displaced, &lock_path).unwrap();
        });

        assert_eq!(provider.epoch().unwrap(), 0);
        assert!(blocked.get());
    }
}
