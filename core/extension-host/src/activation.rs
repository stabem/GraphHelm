//! What is active, and what it is allowed to run.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const CLAIM_METADATA_VERSION: u8 = 1;
const CLAIM_FILE_NAME: &str = "activation.claim";
const CLAIM_SLOT_BYTES: usize = 1024;
const CLAIM_FILE_BYTES: u64 = (CLAIM_SLOT_BYTES * 2) as u64;

/// The record of an activated extension version.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ActivationRecord {
    package_root: PathBuf,
    executables: Vec<PathBuf>,
    package_digest: String,
}

impl ActivationRecord {
    /// A record for a package whose canonical executable was never recorded.
    #[must_use]
    pub fn for_package_without_recorded_executable(package_root: &Path) -> Self {
        Self {
            package_root: package_root.to_path_buf(),
            executables: Vec::new(),
            package_digest: String::new(),
        }
    }

    /// Mint a record by ACTIVATING a validated package.
    ///
    /// Taking the claim by reference is the point: there is no path from a validated package to an
    /// activation record that does not pass through holding the claim. Validation answers
    /// "well-formed"; only this constructor answers "active".
    #[must_use]
    pub fn activate(
        _claim: &ActivationClaim,
        package: &graphhelm_schema::ValidatedExtensionPackage,
        executable: PathBuf,
    ) -> Self {
        Self {
            package_root: executable
                .parent()
                .map_or_else(PathBuf::new, Path::to_path_buf),
            executables: vec![executable],
            package_digest: package.package_digest.clone(),
        }
    }

    /// Whether this record authorizes the given package.
    ///
    /// The comparison is the DIGEST, not the id. Comparing ids reads as "is this the same
    /// extension?" and answers yes for every version of it, so a record minted for 1.0.0 would
    /// authorize 1.1.0 -- and 1.1.0 is a different artifact that merely shares a name. Validation
    /// says well-formed, identity says same family, and neither says "this is what was activated".
    #[must_use]
    pub fn authorizes(&self, package: &graphhelm_schema::ValidatedExtensionPackage) -> bool {
        !self.package_digest.is_empty() && self.package_digest == package.package_digest
    }

    /// A record naming the executables that installation recorded.
    ///
    /// A list rather than an option, because the interesting failure is a state that names TWO --
    /// which an `Option` cannot represent and therefore cannot refuse.
    #[must_use]
    pub fn for_package_with_recorded_executables(
        package_root: &Path,
        executables: Vec<PathBuf>,
    ) -> Self {
        Self {
            package_root: package_root.to_path_buf(),
            executables,
            package_digest: String::new(),
        }
    }

    /// Where the package lives.
    #[must_use]
    pub fn package_root(&self) -> &Path {
        &self.package_root
    }

    /// Every executable this record names.
    #[must_use]
    pub fn recorded_executables(&self) -> &[PathBuf] {
        &self.executables
    }
}

/// Why a claim was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClaimRefusal {
    /// Another activation already holds the claim.
    AlreadyHeld,
    /// A pre-metadata claim cannot be proven stale and is therefore left untouched.
    LegacyUnverifiable,
    /// The claim path is a symbolic link or another reparse point.
    UnsafeClaimPath,
    /// The claim could not be written at all.
    Unwritable,
    /// This operating system has no proven process-lifetime claim authority.
    UnsupportedPlatform,
}

/// The observable state of an activation claim.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClaimStatus {
    /// A live operating-system lock is held.
    Held,
    /// Metadata says held, but its operating-system lock is available.
    Stale,
    /// No claim exists, or released metadata is present without a live lock.
    Free,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
enum PersistedClaimState {
    Held,
    Free,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ClaimMetadata {
    version: u8,
    state: PersistedClaimState,
    claim_id: String,
    owner_pid: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    diagnostic_time_unix_ms: Option<u64>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ClaimFramePayload {
    generation: u64,
    metadata: ClaimMetadata,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ClaimFrame {
    payload: ClaimFramePayload,
    checksum: [u8; 32],
}

impl ClaimMetadata {
    fn held() -> Self {
        Self {
            version: CLAIM_METADATA_VERSION,
            state: PersistedClaimState::Held,
            claim_id: uuid::Uuid::new_v4().to_string(),
            owner_pid: std::process::id(),
            diagnostic_time_unix_ms: diagnostic_time_unix_ms(),
        }
    }

    fn is_valid(&self) -> bool {
        self.version == CLAIM_METADATA_VERSION
            && self.owner_pid != 0
            && uuid::Uuid::parse_str(&self.claim_id).is_ok()
    }
}

/// An exclusive claim over the activation state.
#[derive(Debug)]
pub struct ActivationClaim {
    path: PathBuf,
    root: RootAnchor,
    file: File,
    metadata: ClaimMetadata,
    generation: u64,
}

#[derive(Debug)]
struct RootAnchor {
    path: PathBuf,
    directory: File,
    #[cfg(target_os = "linux")]
    namespace_files: [File; 2],
    #[cfg(target_os = "linux")]
    namespace_locked: bool,
    #[cfg(windows)]
    _ancestors: Vec<File>,
}

impl ActivationClaim {
    /// Take the claim, or refuse.
    ///
    /// # Errors
    ///
    /// Returns [`ClaimRefusal`] when the claim is already held or cannot be written.
    pub fn acquire(install_root: &Path) -> Result<Self, ClaimRefusal> {
        ensure_supported_platform()?;
        let mut root = open_root_anchor(install_root)?;
        lock_root(&mut root)?;
        let path = root.path.join(CLAIM_FILE_NAME);
        let mut file = match open_claim_child(&root, false) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                initialize_claim_child(&root)?
            }
            Err(error) => return Err(map_open_refusal(error)),
        };
        validate_regular_claim(&file)?;
        lock_claim(&file)?;
        verify_named_claim_identity(&root, &file)?;

        let (previous_generation, _) = read_metadata(&mut file)?;
        let generation = previous_generation
            .checked_add(1)
            .ok_or(ClaimRefusal::LegacyUnverifiable)?;
        let metadata = ClaimMetadata::held();
        write_metadata(&mut file, generation, &metadata)?;
        Ok(Self {
            path,
            root,
            file,
            metadata,
            generation,
        })
    }

    /// Inspect claim liveness without changing persisted state.
    ///
    /// # Errors
    ///
    /// Returns [`ClaimRefusal`] for unsafe, unreadable or unverifiable legacy claims.
    pub fn inspect(install_root: &Path) -> Result<ClaimStatus, ClaimRefusal> {
        ensure_supported_platform()?;
        let mut root = open_root_anchor(install_root)?;
        if !try_lock_root_for_inspection(&mut root)? {
            return Ok(ClaimStatus::Held);
        }
        let mut file = match open_claim_child(&root, false) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(ClaimStatus::Free);
            }
            Err(error) if is_lock_contention(&error) => return Ok(ClaimStatus::Held),
            Err(error) => return Err(map_open_refusal(error)),
        };
        validate_regular_claim(&file)?;

        match fs2::FileExt::try_lock_exclusive(&file) {
            Ok(()) => {}
            Err(error) if is_lock_contention(&error) => {
                return Ok(ClaimStatus::Held);
            }
            Err(_) => return Err(ClaimRefusal::Unwritable),
        }
        verify_named_claim_identity(&root, &file)?;

        let metadata = read_metadata(&mut file).map(|(_, metadata)| metadata);
        let _ = fs2::FileExt::unlock(&file);
        metadata.map(|metadata| match metadata.state {
            PersistedClaimState::Held => ClaimStatus::Stale,
            PersistedClaimState::Free => ClaimStatus::Free,
        })
    }

    /// Where the claim lives on disk.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The install root this claim is ANCHORED to.
    ///
    /// Every mutating entry point derives its target from this rather than taking a root
    /// argument, so a claim acquired for one root cannot be presented alongside another: the
    /// disagreement has no way to be spelled (#546). Before that, `uninstall_version` under a
    /// mismatched root removed a tree the claim never covered.
    ///
    /// The path is the one `acquire` walked, not the one it was handed: absolute, and lexically
    /// normalized component by component -- `.` dropped, `..` refused, and every component opened
    /// without following a link, so an anchored root is a path whose ancestors were all real
    /// directories at acquire time. A trailing separator is gone for the same reason, which is
    /// what `install.rs`'s probe would otherwise have to strip a second time.
    #[must_use]
    pub fn install_root(&self) -> &Path {
        &self.root.path
    }

    /// Whether [`Self::install_root`] STILL names the directory this claim anchored (#772).
    ///
    /// The claim walk opens every component without following a link, and that proves the
    /// ancestors were real directories AT ACQUIRE TIME. It cannot prove they stayed that way: the
    /// four lifecycle entry points reach the tree by PATH afterwards, and a directory above the
    /// root can be renamed away and replaced by a link between the two moments. Both probes in
    /// `require_unlinked_layout` then report ordinary directories, because `symlink_metadata`
    /// does not follow the FINAL component and resolves every intermediate one -- so the layout
    /// is read, and written, on the far side of the link. The failure is a SUCCESS, not a
    /// refusal.
    ///
    /// This is the discipline `verify_named_claim_identity` already applies to the claim file,
    /// pointed at the root: open the path by NAME -- deliberately following links, because
    /// landing somewhere else is exactly what is being detected -- and compare its identity
    /// against the handle taken at acquire time. Different identity means the name no longer
    /// reaches the anchored directory.
    ///
    /// **Reachability is not the same on both platforms, and it is measured rather than assumed.**
    /// On Windows an open handle inside a tree pins every directory above it: renaming an
    /// ancestor while the claim is held is refused, and succeeds once it is released. That cover
    /// is OVER-DETERMINED and not attributable to any single field -- the root's own directory
    /// handle alone refuses the rename, and so does a file handle anywhere inside the root. On
    /// Unix a rename with open descriptors inside is ordinary and permitted. So this check is the
    /// only cover on Unix, and on Windows it is a second one behind a platform guarantee.
    ///
    /// # Errors
    ///
    /// Returns [`ClaimRefusal::UnsafeClaimPath`] when the path no longer names the anchored
    /// directory, and [`ClaimRefusal::Unwritable`] when it cannot be opened or interrogated.
    pub fn root_still_anchored(&self) -> Result<(), ClaimRefusal> {
        let named = open_directory_by_name(&self.root.path)?;
        if file_identity(&self.root.directory)? != file_identity(&named)? {
            return Err(ClaimRefusal::UnsafeClaimPath);
        }
        Ok(())
    }
}

#[cfg(any(target_os = "linux", windows))]
fn ensure_supported_platform() -> Result<(), ClaimRefusal> {
    Ok(())
}

#[cfg(all(unix, not(target_os = "linux")))]
fn ensure_supported_platform() -> Result<(), ClaimRefusal> {
    Err(ClaimRefusal::UnsupportedPlatform)
}

#[cfg(not(any(unix, windows)))]
fn ensure_supported_platform() -> Result<(), ClaimRefusal> {
    Err(ClaimRefusal::UnsupportedPlatform)
}

impl Drop for ActivationClaim {
    fn drop(&mut self) {
        if verify_named_claim_identity(&self.root, &self.file).is_ok() {
            self.metadata.state = PersistedClaimState::Free;
            self.metadata.diagnostic_time_unix_ms = diagnostic_time_unix_ms();
            if let Some(generation) = self.generation.checked_add(1) {
                let _ = write_metadata(&mut self.file, generation, &self.metadata);
            }
        }
        let _ = fs2::FileExt::unlock(&self.file);
        unlock_root(&mut self.root);
    }
}

fn read_metadata(file: &mut File) -> Result<(u64, ClaimMetadata), ClaimRefusal> {
    if file.metadata().map_err(|_| ClaimRefusal::Unwritable)?.len() != CLAIM_FILE_BYTES {
        return Err(ClaimRefusal::LegacyUnverifiable);
    }
    file.seek(SeekFrom::Start(0))
        .map_err(|_| ClaimRefusal::Unwritable)?;
    let mut bytes = [0_u8; CLAIM_SLOT_BYTES * 2];
    file.read_exact(&mut bytes)
        .map_err(|_| ClaimRefusal::Unwritable)?;
    let first = decode_slot(&bytes[..CLAIM_SLOT_BYTES]);
    let second = decode_slot(&bytes[CLAIM_SLOT_BYTES..]);
    match (first, second) {
        (Some(first), Some(second)) if first.0 != second.0 => {
            Ok(if first.0 > second.0 { first } else { second })
        }
        (Some(frame), None) | (None, Some(frame)) => Ok(frame),
        _ => Err(ClaimRefusal::LegacyUnverifiable),
    }
}

fn decode_slot(slot: &[u8]) -> Option<(u64, ClaimMetadata)> {
    let length = usize::try_from(u32::from_le_bytes(slot.get(..4)?.try_into().ok()?)).ok()?;
    if length == 0 || length > CLAIM_SLOT_BYTES - 4 {
        return None;
    }
    let frame: ClaimFrame = serde_json::from_slice(slot.get(4..4 + length)?).ok()?;
    if !frame.payload.metadata.is_valid()
        || frame.payload.generation == 0
        || checksum(&frame.payload).ok()? != frame.checksum
    {
        return None;
    }
    Some((frame.payload.generation, frame.payload.metadata))
}

fn write_metadata(
    file: &mut File,
    generation: u64,
    metadata: &ClaimMetadata,
) -> Result<(), ClaimRefusal> {
    let payload = ClaimFramePayload {
        generation,
        metadata: metadata.clone(),
    };
    let frame = ClaimFrame {
        checksum: checksum(&payload)?,
        payload,
    };
    let encoded = serde_json::to_vec(&frame).map_err(|_| ClaimRefusal::Unwritable)?;
    if encoded.len() > CLAIM_SLOT_BYTES - 4 {
        return Err(ClaimRefusal::Unwritable);
    }
    let mut slot = [0_u8; CLAIM_SLOT_BYTES];
    slot[..4].copy_from_slice(&(encoded.len() as u32).to_le_bytes());
    slot[4..4 + encoded.len()].copy_from_slice(&encoded);
    let index = usize::try_from((generation - 1) % 2).map_err(|_| ClaimRefusal::Unwritable)?;
    file.seek(SeekFrom::Start((index * CLAIM_SLOT_BYTES) as u64))
        .map_err(|_| ClaimRefusal::Unwritable)?;
    file.write_all(&slot)
        .map_err(|_| ClaimRefusal::Unwritable)?;
    file.sync_all().map_err(|_| ClaimRefusal::Unwritable)
}

fn checksum(payload: &ClaimFramePayload) -> Result<[u8; 32], ClaimRefusal> {
    let bytes = serde_json::to_vec(payload).map_err(|_| ClaimRefusal::Unwritable)?;
    Ok(Sha256::digest(bytes).into())
}

fn initialize_claim_child(root: &RootAnchor) -> Result<File, ClaimRefusal> {
    let temporary = format!(".activation-{}.tmp", uuid::Uuid::new_v4());
    let mut file = open_child_file(root, &temporary, true).map_err(map_open_refusal)?;
    validate_regular_claim(&file)?;
    file.set_len(CLAIM_FILE_BYTES)
        .map_err(|_| ClaimRefusal::Unwritable)?;
    let mut metadata = ClaimMetadata::held();
    metadata.state = PersistedClaimState::Free;
    write_metadata(&mut file, 1, &metadata)?;

    finish_claim_publication(root, file, &temporary)
}

fn finish_claim_publication(
    root: &RootAnchor,
    file: File,
    temporary: &str,
) -> Result<File, ClaimRefusal> {
    let published = publish_child_without_replacement(root, &file, temporary, CLAIM_FILE_NAME);
    match published {
        Ok(()) => {
            let published = open_claim_child(root, false).map_err(map_open_refusal)?;
            validate_claim_kind(&published)?;
            if file_identity(&file)? != file_identity(&published)? {
                return Err(ClaimRefusal::UnsafeClaimPath);
            }
            if claim_link_count(&file)? != 2 || claim_link_count(&published)? != 2 {
                return Err(ClaimRefusal::UnsafeClaimPath);
            }
            drop(file);
            remove_child(root, temporary).map_err(|_| ClaimRefusal::Unwritable)?;
            validate_regular_claim(&published)?;
            Ok(published)
        }
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            drop(file);
            let _ = remove_child(root, temporary);
            open_claim_child(root, false).map_err(map_open_refusal)
        }
        Err(_) => {
            drop(file);
            let _ = remove_child(root, temporary);
            Err(ClaimRefusal::Unwritable)
        }
    }
}

fn diagnostic_time_unix_ms() -> Option<u64> {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()?
        .as_millis();
    u64::try_from(millis).ok()
}

fn is_lock_contention(error: &std::io::Error) -> bool {
    if error.kind() == std::io::ErrorKind::WouldBlock {
        return true;
    }
    #[cfg(windows)]
    {
        matches!(error.raw_os_error(), Some(32 | 33))
    }
    #[cfg(not(windows))]
    {
        false
    }
}

fn map_open_refusal(error: std::io::Error) -> ClaimRefusal {
    if is_lock_contention(&error) {
        ClaimRefusal::AlreadyHeld
    } else if is_unsafe_open(&error) {
        ClaimRefusal::UnsafeClaimPath
    } else {
        ClaimRefusal::Unwritable
    }
}

fn validate_regular_claim(file: &File) -> Result<(), ClaimRefusal> {
    validate_claim_kind(file)?;
    if claim_link_count(file)? != 1 {
        return Err(ClaimRefusal::UnsafeClaimPath);
    }
    Ok(())
}

fn validate_claim_kind(file: &File) -> Result<(), ClaimRefusal> {
    let metadata = file.metadata().map_err(|_| ClaimRefusal::Unwritable)?;
    if !metadata.is_file() || metadata_is_reparse_point(&metadata) {
        return Err(ClaimRefusal::UnsafeClaimPath);
    }
    Ok(())
}

#[cfg(unix)]
fn claim_link_count(file: &File) -> Result<u64, ClaimRefusal> {
    use std::os::unix::fs::MetadataExt;

    file.metadata()
        .map(|metadata| metadata.nlink())
        .map_err(|_| ClaimRefusal::Unwritable)
}

#[cfg(windows)]
fn claim_link_count(file: &File) -> Result<u64, ClaimRefusal> {
    use std::mem::MaybeUninit;
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{
        BY_HANDLE_FILE_INFORMATION, GetFileInformationByHandle,
    };

    let mut information = MaybeUninit::<BY_HANDLE_FILE_INFORMATION>::uninit();
    // SAFETY: the live file handle and correctly sized output remain valid for the call.
    if unsafe { GetFileInformationByHandle(file.as_raw_handle(), information.as_mut_ptr()) } == 0 {
        return Err(ClaimRefusal::Unwritable);
    }
    // SAFETY: success initialized the complete structure.
    Ok(u64::from(
        unsafe { information.assume_init() }.nNumberOfLinks,
    ))
}

#[cfg(not(any(unix, windows)))]
fn claim_link_count(_file: &File) -> Result<u64, ClaimRefusal> {
    Err(ClaimRefusal::UnsupportedPlatform)
}

#[cfg(windows)]
fn metadata_is_reparse_point(metadata: &std::fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;

    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
    metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

#[cfg(not(windows))]
fn metadata_is_reparse_point(_metadata: &std::fs::Metadata) -> bool {
    false
}

fn open_claim_child(root: &RootAnchor, create_new: bool) -> std::io::Result<File> {
    open_child_file(root, CLAIM_FILE_NAME, create_new)
}

fn lock_claim(file: &File) -> Result<(), ClaimRefusal> {
    fs2::FileExt::try_lock_exclusive(file).map_err(|error| {
        if is_lock_contention(&error) {
            ClaimRefusal::AlreadyHeld
        } else {
            ClaimRefusal::Unwritable
        }
    })
}

/// Open a DIRECTORY by name, following links on purpose.
///
/// Following is the point (#772): the question is whether the name still reaches the anchored
/// directory, and a redirected name landing somewhere else is exactly the answer being looked
/// for. A no-follow open would refuse the redirect instead of reporting it, and would also refuse
/// the ordinary macOS layout where `/tmp` resolves to `/private/tmp`.
///
/// `File::open` cannot do this on Windows -- opening a directory needs
/// `FILE_FLAG_BACKUP_SEMANTICS`, and without it every ordinary layout is refused. That was not
/// reasoned: the first version used `File::open` and the positive control in `tests/links.rs`
/// went red on a layout with no link in it at all, alongside four unrelated cells.
#[cfg(windows)]
fn open_directory_by_name(path: &Path) -> Result<File, ClaimRefusal> {
    use std::os::windows::fs::OpenOptionsExt;
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_FLAG_BACKUP_SEMANTICS, FILE_SHARE_READ, FILE_SHARE_WRITE,
    };

    std::fs::OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
        .open(path)
        .map_err(|_| ClaimRefusal::Unwritable)
}

/// The Unix face of [`open_directory_by_name`]. `File::open` opens a directory here.
#[cfg(not(windows))]
fn open_directory_by_name(path: &Path) -> Result<File, ClaimRefusal> {
    File::open(path).map_err(|_| ClaimRefusal::Unwritable)
}

fn verify_named_claim_identity(root: &RootAnchor, file: &File) -> Result<(), ClaimRefusal> {
    let named = open_claim_child(root, false).map_err(map_open_refusal)?;
    validate_regular_claim(&named)?;
    if file_identity(file)? != file_identity(&named)? {
        return Err(ClaimRefusal::UnsafeClaimPath);
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct FileIdentity {
    device: u64,
    file: u64,
}

#[cfg(unix)]
fn file_identity(file: &File) -> Result<FileIdentity, ClaimRefusal> {
    use std::os::unix::fs::MetadataExt;

    let metadata = file.metadata().map_err(|_| ClaimRefusal::Unwritable)?;
    Ok(FileIdentity {
        device: metadata.dev(),
        file: metadata.ino(),
    })
}

#[cfg(unix)]
fn open_root_anchor(path: &Path) -> Result<RootAnchor, ClaimRefusal> {
    use std::ffi::CString;
    use std::os::fd::{AsRawFd, FromRawFd};
    use std::os::unix::ffi::OsStrExt;

    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|_| ClaimRefusal::Unwritable)?
            .join(path)
    };
    let slash = CString::new("/").map_err(|_| ClaimRefusal::Unwritable)?;
    // SAFETY: the fixed NUL-terminated root path is live and ownership transfers once.
    let descriptor = unsafe {
        libc::open(
            slash.as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC | libc::O_NOFOLLOW,
        )
    };
    if descriptor < 0 {
        return Err(ClaimRefusal::Unwritable);
    }
    // SAFETY: descriptor is fresh and uniquely owned.
    let mut directory = unsafe { File::from_raw_fd(descriptor) };
    let mut normalized = PathBuf::from("/");
    for component in absolute.components() {
        match component {
            std::path::Component::RootDir | std::path::Component::CurDir => {}
            std::path::Component::Normal(part) => {
                let name =
                    CString::new(part.as_bytes()).map_err(|_| ClaimRefusal::UnsafeClaimPath)?;
                // SAFETY: retained directory and NUL-terminated component are live; no links are followed.
                let next = unsafe {
                    libc::openat(
                        directory.as_raw_fd(),
                        name.as_ptr(),
                        libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC | libc::O_NOFOLLOW,
                    )
                };
                if next < 0 {
                    let error = std::io::Error::last_os_error();
                    return Err(map_open_refusal(error));
                }
                // SAFETY: next is a fresh descriptor whose ownership transfers once.
                directory = unsafe { File::from_raw_fd(next) };
                normalized.push(part);
            }
            std::path::Component::ParentDir | std::path::Component::Prefix(_) => {
                return Err(ClaimRefusal::UnsafeClaimPath);
            }
        }
    }
    #[cfg(target_os = "linux")]
    let namespace_files = open_linux_namespace(&directory, &normalized)?;
    Ok(RootAnchor {
        path: normalized,
        directory,
        #[cfg(target_os = "linux")]
        namespace_files,
        #[cfg(target_os = "linux")]
        namespace_locked: false,
    })
}

#[cfg(target_os = "linux")]
fn open_linux_namespace(root_directory: &File, path: &Path) -> Result<[File; 2], ClaimRefusal> {
    use std::ffi::CString;
    use std::os::fd::{AsRawFd, FromRawFd};
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::MetadataExt;

    let effective_uid = unsafe { libc::geteuid() };
    let runtime_root = PathBuf::from(format!("/run/user/{effective_uid}"));
    let runtime_c =
        CString::new(runtime_root.as_os_str().as_bytes()).map_err(|_| ClaimRefusal::Unwritable)?;
    let runtime_descriptor = unsafe {
        libc::open(
            runtime_c.as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC | libc::O_NOFOLLOW,
        )
    };
    if runtime_descriptor < 0 {
        return Err(map_open_refusal(std::io::Error::last_os_error()));
    }
    let runtime = unsafe { File::from_raw_fd(runtime_descriptor) };
    let runtime_metadata = runtime.metadata().map_err(|_| ClaimRefusal::Unwritable)?;
    if !runtime_metadata.is_dir()
        || runtime_metadata.uid() != effective_uid
        || runtime_metadata.mode() & 0o077 != 0
    {
        return Err(ClaimRefusal::UnsafeClaimPath);
    }

    let namespace_name = c"graphhelm-activation-locks";
    if unsafe { libc::mkdirat(runtime.as_raw_fd(), namespace_name.as_ptr(), 0o700) } != 0 {
        let error = std::io::Error::last_os_error();
        if error.kind() != std::io::ErrorKind::AlreadyExists {
            return Err(ClaimRefusal::Unwritable);
        }
    }
    let directory_descriptor = unsafe {
        libc::openat(
            runtime.as_raw_fd(),
            namespace_name.as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC | libc::O_NOFOLLOW,
        )
    };
    if directory_descriptor < 0 {
        return Err(map_open_refusal(std::io::Error::last_os_error()));
    }
    let directory = unsafe { File::from_raw_fd(directory_descriptor) };
    let directory_metadata = directory.metadata().map_err(|_| ClaimRefusal::Unwritable)?;
    if !directory_metadata.is_dir() || directory_metadata.uid() != effective_uid {
        return Err(ClaimRefusal::UnsafeClaimPath);
    }
    if unsafe { libc::fchmod(directory.as_raw_fd(), 0o700) } != 0 {
        return Err(ClaimRefusal::Unwritable);
    }
    let directory_metadata = directory.metadata().map_err(|_| ClaimRefusal::Unwritable)?;
    if directory_metadata.mode() & 0o077 != 0 {
        return Err(ClaimRefusal::UnsafeClaimPath);
    }

    // TWO lock files, because a root has two names and each can lie independently.
    //
    // The PATH-named lock survives root replacement: rename the install root away and recreate
    // it -- same path, NEW inode -- and the replacement still computes the same lock file, so
    // the second acquire contends. `replacing_the_install_root_cannot_create_a_second_live_
    // authority` pins exactly that, and an identity-only name breaks it: the new inode computes
    // a new lock file and the replacement acquires freely.
    //
    // The IDENTITY-named lock survives aliasing: a bind mount gives one directory two paths --
    // different bytes, SAME inode -- and a path hash computes two lock files, so both acquire.
    // With the claim file intact the claim flock still refuses the alias (one inode under both
    // names), but the claim file lives inside the replaceable subtree: unlink it and the next
    // acquire opens a new inode with a free flock. The lock out here in /run/user is the one
    // authority OUTSIDE that subtree, and named by path bytes it defends nothing an alias can
    // reach. Symlink aliases never get this far -- the walk opens every component O_NOFOLLOW --
    // so mounts were the one alias channel still open. (#212 blocker, fourth bullet; read
    // against this site by N on #473; both directions measured on Linux before this shape.)
    //
    // Each name is blind to exactly the case the other catches, so the pair is held together:
    // acquisition takes both, path first then identity, and either contending refuses. The
    // fixed order cannot deadlock -- every process takes at most one of each, in the same order.
    //
    // DECLARED LIMIT: the identity half is what the kernel reports. A filesystem that
    // manufactures a distinct device number per mount of one underlying store (some network
    // filesystems do) makes one root look like two, and from this side that arrangement is
    // indistinguishable from two genuinely different roots. It degrades to the path-named
    // behaviour -- alias acquires a second lock -- never to anything worse. The lock directory
    // lives on tmpfs, so cross-boot instability of device numbers costs nothing: the locks are
    // gone before the numbers can move.
    let open_lock_file = |name: String| -> Result<File, ClaimRefusal> {
        let name = CString::new(name).map_err(|_| ClaimRefusal::Unwritable)?;
        let descriptor = unsafe {
            libc::openat(
                directory.as_raw_fd(),
                name.as_ptr(),
                libc::O_RDWR | libc::O_CREAT | libc::O_CLOEXEC | libc::O_NOFOLLOW,
                0o600,
            )
        };
        if descriptor < 0 {
            return Err(map_open_refusal(std::io::Error::last_os_error()));
        }
        let file = unsafe { File::from_raw_fd(descriptor) };
        let metadata = file.metadata().map_err(|_| ClaimRefusal::Unwritable)?;
        if !metadata.is_file()
            || metadata.uid() != effective_uid
            || metadata.mode() & 0o077 != 0
            || metadata.nlink() != 1
        {
            return Err(ClaimRefusal::UnsafeClaimPath);
        }
        Ok(file)
    };

    let path_digest = hex::encode(Sha256::digest(path.as_os_str().as_bytes()));
    let by_path = open_lock_file(format!("{path_digest}.lock"))?;
    let identity = file_identity(root_directory)?;
    let by_identity = open_lock_file(format!(
        "{:016x}-{:016x}.lock",
        identity.device, identity.file
    ))?;
    Ok([by_path, by_identity])
}

#[cfg(windows)]
fn open_root_anchor(path: &Path) -> Result<RootAnchor, ClaimRefusal> {
    use std::os::windows::fs::OpenOptionsExt;
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_READ, FILE_SHARE_WRITE,
    };

    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|_| ClaimRefusal::Unwritable)?
            .join(path)
    };
    let mut components = absolute.components();
    let prefix = match components.next() {
        Some(std::path::Component::Prefix(prefix)) => prefix,
        _ => return Err(ClaimRefusal::UnsafeClaimPath),
    };
    if !matches!(components.next(), Some(std::path::Component::RootDir)) {
        return Err(ClaimRefusal::UnsafeClaimPath);
    }
    let mut normalized = PathBuf::from(prefix.as_os_str());
    normalized.push(Path::new(std::path::MAIN_SEPARATOR_STR));
    let mut directory = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
        .open(&normalized)
        .map_err(map_open_refusal)?;
    validate_directory(&directory)?;
    let mut ancestors = Vec::new();
    for component in components {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::Normal(name) => {
                let next = nt_open_directory_child(&directory, name).map_err(map_open_refusal)?;
                validate_directory(&next)?;
                ancestors.push(directory);
                directory = next;
                normalized.push(name);
            }
            _ => return Err(ClaimRefusal::UnsafeClaimPath),
        }
    }
    Ok(RootAnchor {
        path: normalized,
        directory,
        _ancestors: ancestors,
    })
}

#[cfg(windows)]
fn validate_directory(directory: &File) -> Result<(), ClaimRefusal> {
    let metadata = directory.metadata().map_err(|_| ClaimRefusal::Unwritable)?;
    if !metadata.is_dir() || metadata_is_reparse_point(&metadata) {
        return Err(ClaimRefusal::UnsafeClaimPath);
    }
    Ok(())
}

#[cfg(unix)]
fn open_child_file(root: &RootAnchor, name: &str, create_new: bool) -> std::io::Result<File> {
    use std::ffi::CString;
    use std::os::fd::{AsRawFd, FromRawFd};

    let name = CString::new(name).map_err(|_| std::io::Error::other("invalid child"))?;
    let mut flags = libc::O_RDWR | libc::O_CLOEXEC | libc::O_NOFOLLOW;
    if create_new {
        flags |= libc::O_CREAT | libc::O_EXCL;
    }
    // SAFETY: retained directory and fixed NUL-terminated child are live.
    let descriptor =
        unsafe { libc::openat(root.directory.as_raw_fd(), name.as_ptr(), flags, 0o600) };
    if descriptor < 0 {
        return Err(std::io::Error::last_os_error());
    }
    // SAFETY: descriptor is fresh and ownership transfers once.
    Ok(unsafe { File::from_raw_fd(descriptor) })
}

#[cfg(windows)]
fn open_child_file(root: &RootAnchor, name: &str, create_new: bool) -> std::io::Result<File> {
    nt_open_file_child(&root.directory, name, create_new)
}

#[cfg(windows)]
fn nt_open_directory_child(directory: &File, name: &std::ffi::OsStr) -> std::io::Result<File> {
    nt_open_relative(directory, name, true, false)
}

#[cfg(windows)]
fn nt_open_file_child(directory: &File, name: &str, create_new: bool) -> std::io::Result<File> {
    nt_open_relative(directory, std::ffi::OsStr::new(name), false, create_new)
}

#[cfg(windows)]
fn nt_open_relative(
    directory: &File,
    name: &std::ffi::OsStr,
    is_directory: bool,
    create_new: bool,
) -> std::io::Result<File> {
    use std::mem::{size_of, zeroed};
    use std::os::windows::ffi::OsStrExt;
    use std::os::windows::io::{AsRawHandle, FromRawHandle};
    use windows_sys::Wdk::Foundation::OBJECT_ATTRIBUTES;
    use windows_sys::Wdk::Storage::FileSystem::{
        FILE_CREATE, FILE_DIRECTORY_FILE, FILE_NON_DIRECTORY_FILE, FILE_OPEN,
        FILE_OPEN_REPARSE_POINT, FILE_SYNCHRONOUS_IO_NONALERT, NtCreateFile,
    };
    use windows_sys::Win32::Foundation::{HANDLE, OBJ_CASE_INSENSITIVE, UNICODE_STRING};
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_ATTRIBUTE_NORMAL, FILE_LIST_DIRECTORY, FILE_READ_ATTRIBUTES, FILE_READ_DATA,
        FILE_SHARE_READ, FILE_SHARE_WRITE, FILE_TRAVERSE, FILE_WRITE_DATA, SYNCHRONIZE,
    };
    use windows_sys::Win32::System::IO::IO_STATUS_BLOCK;

    let mut wide = name.encode_wide().collect::<Vec<_>>();
    if wide.is_empty()
        || wide.len() > 255
        || wide.as_slice() == [46]
        || wide.as_slice() == [46, 46]
        || wide
            .iter()
            .any(|unit| matches!(*unit, 0 | 34 | 42 | 47 | 58 | 60 | 62 | 63 | 92 | 124))
    {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "unsafe child name",
        ));
    }
    let byte_length = wide
        .len()
        .checked_mul(2)
        .and_then(|length| u16::try_from(length).ok())
        .ok_or_else(|| std::io::Error::other("child name too long"))?;
    let unicode = UNICODE_STRING {
        Length: byte_length,
        MaximumLength: byte_length,
        Buffer: wide.as_mut_ptr(),
    };
    let attributes = OBJECT_ATTRIBUTES {
        Length: size_of::<OBJECT_ATTRIBUTES>() as u32,
        RootDirectory: directory.as_raw_handle(),
        ObjectName: &unicode,
        Attributes: OBJ_CASE_INSENSITIVE,
        SecurityDescriptor: std::ptr::null(),
        SecurityQualityOfService: std::ptr::null(),
    };
    let mut handle: HANDLE = std::ptr::null_mut();
    // SAFETY: zero is the documented initial state and all pointers remain live.
    let mut status: IO_STATUS_BLOCK = unsafe { zeroed() };
    let access = FILE_READ_ATTRIBUTES
        | SYNCHRONIZE
        | if is_directory {
            FILE_LIST_DIRECTORY | FILE_TRAVERSE
        } else {
            FILE_READ_DATA | FILE_WRITE_DATA
        };
    let options = FILE_OPEN_REPARSE_POINT
        | FILE_SYNCHRONOUS_IO_NONALERT
        | if is_directory {
            FILE_DIRECTORY_FILE
        } else {
            FILE_NON_DIRECTORY_FILE
        };
    // SAFETY: retained parent, bounded relative name, and output storage are live.
    let result = unsafe {
        NtCreateFile(
            &mut handle,
            access,
            &attributes,
            &mut status,
            std::ptr::null(),
            FILE_ATTRIBUTE_NORMAL,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            if create_new { FILE_CREATE } else { FILE_OPEN },
            options,
            std::ptr::null(),
            0,
        )
    };
    if result < 0 || handle.is_null() {
        return Err(map_ntstatus(result));
    }
    // SAFETY: successful NtCreateFile returned one owned handle.
    Ok(unsafe { File::from_raw_handle(handle) })
}

#[cfg(windows)]
fn map_ntstatus(status: i32) -> std::io::Error {
    use windows_sys::Win32::Foundation::{
        STATUS_DIRECTORY_IS_A_REPARSE_POINT, STATUS_FILE_IS_A_DIRECTORY,
        STATUS_IO_REPARSE_TAG_NOT_HANDLED, STATUS_LOCK_NOT_GRANTED, STATUS_NO_SUCH_FILE,
        STATUS_NOT_A_DIRECTORY, STATUS_OBJECT_NAME_COLLISION, STATUS_OBJECT_NAME_NOT_FOUND,
        STATUS_OBJECT_PATH_NOT_FOUND, STATUS_OBJECT_TYPE_MISMATCH,
        STATUS_REPARSE_POINT_ENCOUNTERED, STATUS_REPARSE_POINT_NOT_RESOLVED,
        STATUS_SHARING_VIOLATION, STATUS_STOPPED_ON_SYMLINK, STATUS_SYMLINK_CLASS_DISABLED,
    };

    if matches!(
        status,
        STATUS_NO_SUCH_FILE | STATUS_OBJECT_NAME_NOT_FOUND | STATUS_OBJECT_PATH_NOT_FOUND
    ) {
        std::io::Error::new(std::io::ErrorKind::NotFound, "child not found")
    } else if status == STATUS_OBJECT_NAME_COLLISION {
        std::io::Error::new(std::io::ErrorKind::AlreadyExists, "child already exists")
    } else if matches!(status, STATUS_SHARING_VIOLATION | STATUS_LOCK_NOT_GRANTED) {
        std::io::Error::new(std::io::ErrorKind::WouldBlock, "child is locked")
    } else if matches!(
        status,
        STATUS_NOT_A_DIRECTORY
            | STATUS_FILE_IS_A_DIRECTORY
            | STATUS_OBJECT_TYPE_MISMATCH
            | STATUS_DIRECTORY_IS_A_REPARSE_POINT
            | STATUS_REPARSE_POINT_ENCOUNTERED
            | STATUS_REPARSE_POINT_NOT_RESOLVED
            | STATUS_IO_REPARSE_TAG_NOT_HANDLED
            | STATUS_STOPPED_ON_SYMLINK
            | STATUS_SYMLINK_CLASS_DISABLED
    ) {
        std::io::Error::new(std::io::ErrorKind::InvalidInput, "unsafe reparse point")
    } else {
        std::io::Error::other(format!("NtCreateFile failed: {status:#x}"))
    }
}

#[cfg(unix)]
fn publish_child_without_replacement(
    root: &RootAnchor,
    _source_file: &File,
    source: &str,
    destination: &str,
) -> std::io::Result<()> {
    use std::ffi::CString;
    use std::os::fd::AsRawFd;

    let source = CString::new(source).map_err(|_| std::io::Error::other("invalid child"))?;
    let destination =
        CString::new(destination).map_err(|_| std::io::Error::other("invalid child"))?;
    // SAFETY: retained root and both bounded NUL-terminated names are live.
    if unsafe {
        libc::linkat(
            root.directory.as_raw_fd(),
            source.as_ptr(),
            root.directory.as_raw_fd(),
            destination.as_ptr(),
            0,
        )
    } == 0
    {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

#[cfg(windows)]
fn publish_child_without_replacement(
    root: &RootAnchor,
    _source_file: &File,
    source: &str,
    destination: &str,
) -> std::io::Result<()> {
    std::fs::hard_link(root.path.join(source), root.path.join(destination))
}

#[cfg(unix)]
fn remove_child(root: &RootAnchor, name: &str) -> std::io::Result<()> {
    use std::ffi::CString;
    use std::os::fd::AsRawFd;

    let name = CString::new(name).map_err(|_| std::io::Error::other("invalid child"))?;
    // SAFETY: retained root and bounded NUL-terminated child are live.
    if unsafe { libc::unlinkat(root.directory.as_raw_fd(), name.as_ptr(), 0) } == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

#[cfg(windows)]
fn remove_child(root: &RootAnchor, name: &str) -> std::io::Result<()> {
    std::fs::remove_file(root.path.join(name))
}

#[cfg(target_os = "linux")]
fn lock_root(root: &mut RootAnchor) -> Result<(), ClaimRefusal> {
    let refusal = |error: &std::io::Error| {
        if is_lock_contention(error) {
            ClaimRefusal::AlreadyHeld
        } else {
            ClaimRefusal::Unwritable
        }
    };
    // Both halves of the pair, in their fixed order (path-named, then identity-named). Either
    // contending refuses, and a refusal on the second releases the first -- a failed acquire
    // must not leave half an authority held.
    let [by_path, by_identity] = &root.namespace_files;
    fs2::FileExt::try_lock_exclusive(by_path).map_err(|error| refusal(&error))?;
    if let Err(error) = fs2::FileExt::try_lock_exclusive(by_identity) {
        let _ = fs2::FileExt::unlock(by_path);
        return Err(refusal(&error));
    }
    root.namespace_locked = true;
    Ok(())
}

#[cfg(all(unix, not(target_os = "linux")))]
fn lock_root(root: &mut RootAnchor) -> Result<(), ClaimRefusal> {
    let _ = root;
    Err(ClaimRefusal::UnsupportedPlatform)
}

#[cfg(windows)]
fn lock_root(_root: &mut RootAnchor) -> Result<(), ClaimRefusal> {
    Ok(())
}

#[cfg(target_os = "linux")]
fn try_lock_root_for_inspection(root: &mut RootAnchor) -> Result<bool, ClaimRefusal> {
    match lock_root(root) {
        Ok(()) => Ok(true),
        Err(ClaimRefusal::AlreadyHeld) => Ok(false),
        Err(error) => Err(error),
    }
}

#[cfg(all(unix, not(target_os = "linux")))]
fn try_lock_root_for_inspection(root: &mut RootAnchor) -> Result<bool, ClaimRefusal> {
    let _ = root;
    Err(ClaimRefusal::UnsupportedPlatform)
}

#[cfg(windows)]
fn try_lock_root_for_inspection(_root: &mut RootAnchor) -> Result<bool, ClaimRefusal> {
    Ok(true)
}

#[cfg(target_os = "linux")]
fn unlock_root(root: &mut RootAnchor) {
    if !root.namespace_locked {
        return;
    }
    let [by_path, by_identity] = &root.namespace_files;
    let _ = fs2::FileExt::unlock(by_identity);
    let _ = fs2::FileExt::unlock(by_path);
    root.namespace_locked = false;
}

#[cfg(all(unix, not(target_os = "linux")))]
fn unlock_root(root: &mut RootAnchor) {
    let _ = root;
}

#[cfg(windows)]
fn unlock_root(_root: &mut RootAnchor) {}

#[cfg(unix)]
fn is_unsafe_open(error: &std::io::Error) -> bool {
    matches!(
        error.raw_os_error(),
        Some(code) if code == libc::ELOOP || code == libc::ENOTDIR
    )
}

#[cfg(windows)]
fn is_unsafe_open(error: &std::io::Error) -> bool {
    error.kind() == std::io::ErrorKind::InvalidInput
}

#[cfg(windows)]
fn file_identity(file: &File) -> Result<FileIdentity, ClaimRefusal> {
    use std::mem::MaybeUninit;
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{
        BY_HANDLE_FILE_INFORMATION, GetFileInformationByHandle,
    };

    let mut information = MaybeUninit::<BY_HANDLE_FILE_INFORMATION>::uninit();
    // SAFETY: the live file handle and correctly sized output remain valid for the call.
    if unsafe { GetFileInformationByHandle(file.as_raw_handle(), information.as_mut_ptr()) } == 0 {
        return Err(ClaimRefusal::Unwritable);
    }
    // SAFETY: success initialized the complete structure.
    let information = unsafe { information.assume_init() };
    Ok(FileIdentity {
        device: u64::from(information.dwVolumeSerialNumber),
        file: (u64::from(information.nFileIndexHigh) << 32) | u64::from(information.nFileIndexLow),
    })
}

#[cfg(test)]
mod claim_publication_tests {
    use super::*;

    fn initialized_temporary(root: &RootAnchor, name: &str) -> File {
        let mut file = open_child_file(root, name, true).expect("temporary claim");
        file.set_len(CLAIM_FILE_BYTES).expect("claim size");
        let mut metadata = ClaimMetadata::held();
        metadata.state = PersistedClaimState::Free;
        write_metadata(&mut file, 1, &metadata).expect("claim metadata");
        file
    }

    #[cfg(unix)]
    #[test]
    fn swapped_temporary_inode_is_refused_without_writing_the_sentinel() {
        let install = tempfile::tempdir().expect("install root");
        let root = open_root_anchor(install.path()).expect("root anchor");
        let temporary = ".activation-swap.tmp";
        let original = initialized_temporary(&root, temporary);
        std::fs::rename(
            install.path().join(temporary),
            install.path().join("original.tmp"),
        )
        .expect("displace original temporary");
        let sentinel = install.path().join("sentinel");
        std::fs::write(&sentinel, b"sentinel").expect("sentinel");
        std::fs::hard_link(&sentinel, install.path().join(temporary)).expect("attacker hard link");

        assert_eq!(
            finish_claim_publication(&root, original, temporary).err(),
            Some(ClaimRefusal::UnsafeClaimPath)
        );
        assert_eq!(std::fs::read(sentinel).unwrap(), b"sentinel");
    }

    #[cfg(windows)]
    #[test]
    fn retained_temporary_handle_denies_name_replacement_until_publication() {
        let install = tempfile::tempdir().expect("install root");
        let root = open_root_anchor(install.path()).expect("root anchor");
        let temporary = ".activation-swap.tmp";
        let original = initialized_temporary(&root, temporary);

        assert!(
            std::fs::rename(
                install.path().join(temporary),
                install.path().join("displaced.tmp"),
            )
            .is_err(),
            "the retained no-share-delete handle must close the source-name swap window"
        );
        let published = finish_claim_publication(&root, original, temporary)
            .expect("publish retained temporary");
        assert_eq!(
            file_identity(&published),
            file_identity(&open_claim_child(&root, false).unwrap())
        );
    }
}

/// A durable step in switching the active version.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActivationStep {
    /// The staged copy has been verified against the manifest digest.
    StagedCopyVerified,
    /// The previous version is no longer the active one.
    PreviousVersionRetired,
    /// The new version is recorded as active.
    NewVersionRecorded,
}

/// The order the durable steps of an activation are written in.
#[must_use]
pub fn activation_steps() -> Vec<ActivationStep> {
    // Record the new version BEFORE retiring the previous one. Both orders complete identically;
    // they differ only in what a crash leaves behind.
    //
    // Retire-then-record leaves a machine with no active version at all -- which is not a rollback,
    // it is an outage, and it is indistinguishable from a machine that never had the extension.
    // Record-then-retire leaves two versions recorded for an instant, which is a state a reader can
    // resolve: the newest recorded one is active and the older is collectable.
    //
    // The rule is not "write in this order". It is: order the writes so that every intermediate a
    // crash can leave behind reads as the TRUTH.
    vec![
        ActivationStep::StagedCopyVerified,
        ActivationStep::NewVersionRecorded,
        ActivationStep::PreviousVersionRetired,
    ]
}
