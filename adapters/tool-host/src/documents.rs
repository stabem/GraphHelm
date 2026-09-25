//! Bounded owner edits of project text, never an operational graph mutation.
//!
//! A nonblocking project lock serializes cooperating Runtime writers. An expected content
//! digest detects edits since loading. Handle-relative traversal and identity checks refuse
//! hostile file or directory swaps. External editors can still mutate an opened file in place;
//! these checks do not claim filesystem transactions against noncooperating processes.

use std::ffi::OsStr;
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use graphhelm_runtime::context::{secret_shaped, sensitive_path};
use graphhelm_tool_broker::record::digest_hex;

pub const MAX_DOCUMENT_BYTES: usize = 128 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocumentSnapshot {
    pub content: String,
    pub content_sha256: String,
}

/// Diagnostics deliberately carry neither the path nor the rejected content.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum DocumentError {
    #[error("the document path is not permitted")]
    InvalidPath,
    #[error("the document contains protected content")]
    ProtectedContent,
    #[error("the document exceeds the text size limit")]
    TooLarge,
    #[error("the document is not regular UTF-8 text")]
    NotText,
    #[error("the document is unavailable")]
    Unavailable,
    #[error(
        "the save could not be confirmed; the document may have changed, reload before retrying"
    )]
    SaveUnconfirmed,
    #[error("the document changed; reload before saving")]
    Conflict,
    #[error("another project edit is in progress; retry")]
    Busy,
}

pub struct ProjectDocuments {
    root: PathBuf,
    root_handle: File,
}

struct OpenedDocument {
    parent: File,
    file: File,
    name: String,
}

pub struct ProjectDocumentsLock {
    root: PathBuf,
    #[cfg(unix)]
    _file: File,
    #[cfg(windows)]
    _mutex: WindowsProjectMutex,
}

#[cfg(windows)]
struct WindowsProjectMutex {
    handle: windows_sys::Win32::Foundation::HANDLE,
    key: String,
    _thread_bound: std::marker::PhantomData<std::rc::Rc<()>>,
}

#[cfg(windows)]
impl Drop for WindowsProjectMutex {
    fn drop(&mut self) {
        use windows_sys::Win32::Foundation::CloseHandle;
        use windows_sys::Win32::System::Threading::ReleaseMutex;

        // SAFETY: `handle` is an owned mutex held by this thread. This thread-bound guard
        // releases and closes it exactly once during drop.
        unsafe {
            ReleaseMutex(self.handle);
            CloseHandle(self.handle);
        }
        release_windows_reservation(&self.key);
    }
}

#[cfg(windows)]
fn windows_reservations() -> &'static std::sync::Mutex<std::collections::BTreeSet<String>> {
    static RESERVATIONS: std::sync::OnceLock<std::sync::Mutex<std::collections::BTreeSet<String>>> =
        std::sync::OnceLock::new();
    RESERVATIONS.get_or_init(|| std::sync::Mutex::new(std::collections::BTreeSet::new()))
}

#[cfg(windows)]
fn release_windows_reservation(key: &str) {
    let mut reservations = windows_reservations()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    reservations.remove(key);
}

impl ProjectDocuments {
    /// Opens an explicitly selected existing project. No current-directory fallback.
    ///
    /// # Errors
    /// Refuses missing directories and link/reparse components.
    pub fn open(root: &Path) -> Result<Self, DocumentError> {
        let (root, root_handle) = open_project_directory(root)?;
        Ok(Self { root, root_handle })
    }

    /// Reads one whole bounded document, never a truncated editor buffer.
    ///
    /// # Errors
    /// Refuses unsafe paths, unavailable files, non-text or protected content, and excess size.
    pub fn read(&self, relative: &str) -> Result<DocumentSnapshot, DocumentError> {
        let document = self.open_document(relative)?;
        read_document(&document.file)
    }

    /// Atomically replaces one existing document; lock acquisition never waits.
    /// Permissions are copied from the existing file. A conflict leaves it unchanged.
    ///
    /// # Errors
    /// Refuses invalid content/paths, unavailable files, a held lock, and stale hashes.
    pub fn save(
        &self,
        relative: &str,
        expected_sha256: &str,
        content: &str,
    ) -> Result<DocumentSnapshot, DocumentError> {
        validate_content(content)?;
        let lock = self.lock()?;
        self.save_locked(&lock, relative, expected_sha256, content)
    }

    /// Atomically replaces a document while holding the caller's project lock.
    pub fn save_locked(
        &self,
        lock: &ProjectDocumentsLock,
        relative: &str,
        expected_sha256: &str,
        content: &str,
    ) -> Result<DocumentSnapshot, DocumentError> {
        if self.root != lock.root {
            return Err(DocumentError::InvalidPath);
        }
        validate_content(content)?;
        let document = self.open_document(relative)?;
        let before = read_document(&document.file)?;
        if before.content_sha256 != expected_sha256 {
            return Err(DocumentError::Conflict);
        }
        let permissions = document
            .file
            .metadata()
            .map_err(|_| DocumentError::Unavailable)?
            .permissions();
        if permissions.readonly() {
            return Err(DocumentError::Unavailable);
        }
        #[cfg(target_os = "linux")]
        let unix_security = UnixSecurity::from_file(&document.file)?;
        #[cfg(all(unix, not(target_os = "linux")))]
        {
            // POSIX ACL APIs differ across Unix targets. Refuse before creating a temporary
            // file until this target has a metadata copier, rather than silently losing ACLs.
            return Err(DocumentError::Unavailable);
        }
        #[cfg(windows)]
        let security = WindowsSecurity::read(&document.file)?;
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let temporary_name = format!(
            ".graphhelm-document-{}-{}.tmp",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        );
        let mut temporary = Temporary::create(&document.parent, &temporary_name)?;
        // Windows requires its access restrictions before plaintext is written. Unix creates
        // the temporary with mode 0600 and applies the captured ACL only after the first sync.
        #[cfg(windows)]
        {
            security.apply(&temporary.file)?;
            let copied = WindowsSecurity::read(&temporary.file)?;
            if copied != security {
                return Err(DocumentError::Unavailable);
            }
        }
        #[cfg(not(target_os = "linux"))]
        temporary
            .file
            .set_permissions(permissions)
            .map_err(|_| DocumentError::Unavailable)?;
        #[cfg(target_os = "linux")]
        UnixSecurity::apply_security_xattrs(&temporary.file, &unix_security.security_xattrs)?;
        temporary
            .file
            .write_all(content.as_bytes())
            .and_then(|()| temporary.file.sync_all())
            .map_err(|_| DocumentError::Unavailable)?;
        #[cfg(target_os = "linux")]
        {
            // Writing can clear setuid/setgid. Restore and verify all captured metadata after
            // the plaintext is durable, then sync again so the replacement carries it.
            unix_security.apply(&temporary.file)?;
            temporary
                .file
                .sync_all()
                .map_err(|_| DocumentError::Unavailable)?;
        }
        // Recheck the complete path and content immediately before publication.
        let current = self.open_document(relative)?;
        if !same_opened_document(&document, &current)?
            || read_document(&current.file)?.content_sha256 != expected_sha256
        {
            return Err(DocumentError::Conflict);
        }
        #[cfg(windows)]
        if WindowsSecurity::read(&current.file)? != security {
            return Err(DocumentError::Conflict);
        }
        #[cfg(target_os = "linux")]
        if UnixSecurity::from_file(&current.file)? != unix_security {
            return Err(DocumentError::Conflict);
        }
        // Windows refuses replacement while the target has any open handle. The retained
        // directory handle still anchors publication after these verified file handles close.
        drop(current);
        drop(document.file);
        temporary.publish(&document.parent, &document.name)?;
        Ok(snapshot(content.to_owned()))
    }

    fn open_document(&self, relative: &str) -> Result<OpenedDocument, DocumentError> {
        validate_path(relative)?;
        let mut parts = relative.split('/');
        let mut parent = self
            .root_handle
            .try_clone()
            .map_err(|_| DocumentError::Unavailable)?;
        let mut name = parts.next().ok_or(DocumentError::InvalidPath)?.to_owned();
        for next in parts {
            parent = open_child_directory(&parent, OsStr::new(&name))?;
            name = next.to_owned();
        }
        let file = open_child_file(&parent, OsStr::new(&name))?;
        Ok(OpenedDocument { parent, file, name })
    }

    /// Acquires the project writer lock without waiting.
    pub fn lock(&self) -> Result<ProjectDocumentsLock, DocumentError> {
        #[cfg(unix)]
        #[allow(clippy::needless_return)]
        {
            // `try_clone`/dup would share flock state with the retained root handle. Opening
            // `.` relative to that handle creates an independent open-file description so a
            // second writer in this process still receives Busy.
            let file = open_child_directory(&self.root_handle, OsStr::new("."))?;
            file.try_lock().map_err(|_| DocumentError::Busy)?;
            return Ok(ProjectDocumentsLock {
                root: self.root.clone(),
                _file: file,
            });
        }
        #[cfg(windows)]
        {
            Ok(ProjectDocumentsLock {
                root: self.root.clone(),
                _mutex: self.acquire_windows_lock()?,
            })
        }
        #[cfg(not(any(unix, windows)))]
        {
            Err(DocumentError::Unavailable)
        }
    }

    #[cfg(windows)]
    fn acquire_windows_lock(&self) -> Result<WindowsProjectMutex, DocumentError> {
        use windows_sys::Win32::Foundation::{
            CloseHandle, WAIT_ABANDONED, WAIT_OBJECT_0, WAIT_TIMEOUT,
        };
        use windows_sys::Win32::System::Threading::{CreateMutexW, WaitForSingleObject};

        // The global kernel namespace is stable across processes, sessions, and TEMP settings.
        // File identity prevents casing, verbatim-prefix, and other path aliases from splitting it.
        let identity = file_identity(&self.root_handle)?;
        let mut bytes = Vec::with_capacity(16);
        bytes.extend_from_slice(&identity.device.to_le_bytes());
        bytes.extend_from_slice(&identity.file.to_le_bytes());
        let key = digest_hex(&bytes).to_ascii_lowercase();
        {
            let mut reservations = windows_reservations()
                .lock()
                .map_err(|_| DocumentError::Unavailable)?;
            if !reservations.insert(key.clone()) {
                return Err(DocumentError::Busy);
            }
        }
        let name = format!("Global\\GraphHelmOwnerDocuments-{key}");
        let mut wide_name = name.encode_utf16().collect::<Vec<_>>();
        wide_name.push(0);
        // SAFETY: the security descriptor pointer is null, initial ownership is false, and
        // `wide_name` is a live, nul-terminated UTF-16 buffer for the duration of the call.
        let handle = unsafe { CreateMutexW(std::ptr::null(), 0, wide_name.as_ptr()) };
        if handle.is_null() {
            release_windows_reservation(&key);
            return Err(DocumentError::Unavailable);
        }
        // SAFETY: `handle` was returned by CreateMutexW and remains live for this call.
        let wait = unsafe { WaitForSingleObject(handle, 0) };
        match wait {
            WAIT_OBJECT_0 | WAIT_ABANDONED => Ok(WindowsProjectMutex {
                handle,
                key,
                _thread_bound: std::marker::PhantomData,
            }),
            WAIT_TIMEOUT => {
                // SAFETY: this branch did not acquire the mutex; it closes its owned handle once.
                unsafe { CloseHandle(handle) };
                release_windows_reservation(&key);
                Err(DocumentError::Busy)
            }
            _ => {
                // SAFETY: this branch did not acquire the mutex; it closes its owned handle once.
                unsafe { CloseHandle(handle) };
                release_windows_reservation(&key);
                Err(DocumentError::Unavailable)
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct FileIdentity {
    device: u64,
    file: u64,
}

fn same_opened_document(
    original: &OpenedDocument,
    current: &OpenedDocument,
) -> Result<bool, DocumentError> {
    Ok(
        file_identity(&original.parent)? == file_identity(&current.parent)?
            && file_identity(&original.file)? == file_identity(&current.file)?,
    )
}

#[cfg(unix)]
fn file_identity(file: &File) -> Result<FileIdentity, DocumentError> {
    use std::os::unix::fs::MetadataExt;
    let metadata = file.metadata().map_err(|_| DocumentError::Unavailable)?;
    Ok(FileIdentity {
        device: metadata.dev(),
        file: metadata.ino(),
    })
}

#[cfg(windows)]
fn file_identity(file: &File) -> Result<FileIdentity, DocumentError> {
    use std::mem::MaybeUninit;
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{
        BY_HANDLE_FILE_INFORMATION, GetFileInformationByHandle,
    };
    let mut information = MaybeUninit::<BY_HANDLE_FILE_INFORMATION>::uninit();
    // SAFETY: the live file handle initializes the fixed-size output on success.
    if unsafe { GetFileInformationByHandle(file.as_raw_handle(), information.as_mut_ptr()) } == 0 {
        return Err(DocumentError::Unavailable);
    }
    // SAFETY: successful GetFileInformationByHandle initialized every field.
    let information = unsafe { information.assume_init() };
    Ok(FileIdentity {
        device: u64::from(information.dwVolumeSerialNumber),
        file: (u64::from(information.nFileIndexHigh) << 32) | u64::from(information.nFileIndexLow),
    })
}

fn open_project_directory(path: &Path) -> Result<(PathBuf, File), DocumentError> {
    use std::path::Component;

    let absolute = std::path::absolute(path).map_err(|_| DocumentError::Unavailable)?;
    let mut components = absolute.components();
    #[cfg(unix)]
    let (mut handle, mut normalized) = match components.next() {
        Some(Component::RootDir) => (
            open_directory_path(Path::new("/"))?,
            PathBuf::from(std::path::MAIN_SEPARATOR_STR),
        ),
        _ => return Err(DocumentError::InvalidPath),
    };
    #[cfg(windows)]
    let (mut handle, mut normalized) = {
        let prefix = match components.next() {
            Some(Component::Prefix(prefix)) => prefix,
            _ => return Err(DocumentError::InvalidPath),
        };
        if !matches!(components.next(), Some(Component::RootDir)) {
            return Err(DocumentError::InvalidPath);
        }
        let mut anchor = PathBuf::from(prefix.as_os_str());
        anchor.push(std::path::MAIN_SEPARATOR_STR);
        (open_directory_path(&anchor)?, anchor)
    };
    for component in components {
        match component {
            Component::Normal(name) => {
                handle = open_child_directory(&handle, name)?;
                normalized.push(name);
            }
            Component::CurDir => {}
            Component::ParentDir => {
                handle = open_child_directory(&handle, OsStr::new(".."))?;
                if !normalized.pop() {
                    return Err(DocumentError::InvalidPath);
                }
            }
            Component::Prefix(_) | Component::RootDir => return Err(DocumentError::InvalidPath),
        }
    }
    Ok((normalized, handle))
}

#[cfg(unix)]
fn open_directory_path(path: &Path) -> Result<File, DocumentError> {
    use std::os::unix::fs::OpenOptionsExt;
    let mut options = OpenOptions::new();
    options
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC);
    let file = options.open(path).map_err(|_| DocumentError::Unavailable)?;
    if !file
        .metadata()
        .map_err(|_| DocumentError::Unavailable)?
        .is_dir()
    {
        return Err(DocumentError::InvalidPath);
    }
    Ok(file)
}

#[cfg(windows)]
fn open_directory_path(path: &Path) -> Result<File, DocumentError> {
    use std::os::windows::fs::OpenOptionsExt;
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_LIST_DIRECTORY, FILE_READ_ATTRIBUTES, FILE_SHARE_DELETE, FILE_SHARE_READ,
        FILE_SHARE_WRITE, FILE_TRAVERSE, SYNCHRONIZE,
    };
    let mut options = OpenOptions::new();
    options
        .read(true)
        .access_mode(FILE_LIST_DIRECTORY | FILE_TRAVERSE | FILE_READ_ATTRIBUTES | SYNCHRONIZE)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
        .custom_flags(0x0200_0000 | 0x0020_0000);
    let file = options.open(path).map_err(|_| DocumentError::Unavailable)?;
    validate_opened_entry(&file, true)?;
    Ok(file)
}

#[cfg(unix)]
fn open_child_directory(parent: &File, name: &OsStr) -> Result<File, DocumentError> {
    use std::ffi::CString;
    use std::os::fd::{AsRawFd, FromRawFd};
    use std::os::unix::ffi::OsStrExt;
    let name = CString::new(name.as_bytes()).map_err(|_| DocumentError::InvalidPath)?;
    // SAFETY: the retained parent descriptor and NUL-terminated child name are live.
    let fd = unsafe {
        libc::openat(
            parent.as_raw_fd(),
            name.as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
    if fd < 0 {
        return Err(DocumentError::InvalidPath);
    }
    // SAFETY: openat returned one fresh owned descriptor.
    let file = unsafe { File::from_raw_fd(fd) };
    Ok(file)
}

#[cfg(unix)]
fn open_child_file(parent: &File, name: &OsStr) -> Result<File, DocumentError> {
    use std::ffi::CString;
    use std::os::fd::{AsRawFd, FromRawFd};
    use std::os::unix::ffi::OsStrExt;
    let name = CString::new(name.as_bytes()).map_err(|_| DocumentError::InvalidPath)?;
    // SAFETY: the retained parent descriptor and NUL-terminated child name are live.
    let fd = unsafe {
        libc::openat(
            parent.as_raw_fd(),
            name.as_ptr(),
            libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK,
        )
    };
    if fd < 0 {
        // `O_NOFOLLOW` on a link answers `ELOOP` (Linux, macOS) or `EMLINK` (FreeBSD): the same
        // refusal Windows reaches through `validate_opened_entry`'s reparse-point check. `name`
        // is one component beneath an open directory and the call carries no `O_DIRECTORY`, so
        // neither errno has another source here. Every other failure stays `Unavailable`.
        let error = std::io::Error::last_os_error().raw_os_error();
        return Err(match error {
            Some(code) if code == libc::ELOOP || code == libc::EMLINK => DocumentError::InvalidPath,
            _ => DocumentError::Unavailable,
        });
    }
    // SAFETY: openat returned one fresh owned descriptor.
    let file = unsafe { File::from_raw_fd(fd) };
    validate_opened_entry(&file, false)?;
    Ok(file)
}

#[cfg(windows)]
fn open_child_directory(parent: &File, name: &OsStr) -> Result<File, DocumentError> {
    nt_open_child(parent, name, true, false)
}

#[cfg(windows)]
fn open_child_file(parent: &File, name: &OsStr) -> Result<File, DocumentError> {
    nt_open_child(parent, name, false, false)
}

#[cfg(windows)]
fn nt_open_child(
    parent: &File,
    name: &OsStr,
    directory: bool,
    create: bool,
) -> Result<File, DocumentError> {
    use std::mem::{size_of, zeroed};
    use std::os::windows::io::{AsRawHandle, FromRawHandle};
    use windows_sys::Wdk::Foundation::OBJECT_ATTRIBUTES;
    use windows_sys::Wdk::Storage::FileSystem::{
        FILE_CREATE, FILE_DIRECTORY_FILE, FILE_NON_DIRECTORY_FILE, FILE_OPEN,
        FILE_OPEN_REPARSE_POINT, FILE_SYNCHRONOUS_IO_NONALERT, NtCreateFile,
    };
    use windows_sys::Win32::Foundation::{HANDLE, UNICODE_STRING};
    use windows_sys::Win32::Storage::FileSystem::{
        DELETE, FILE_ATTRIBUTE_NORMAL, FILE_LIST_DIRECTORY, FILE_READ_ATTRIBUTES, FILE_READ_DATA,
        FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, FILE_TRAVERSE, FILE_WRITE_ATTRIBUTES,
        FILE_WRITE_DATA, READ_CONTROL, SYNCHRONIZE, WRITE_DAC, WRITE_OWNER,
    };
    use windows_sys::Win32::System::IO::IO_STATUS_BLOCK;

    use std::os::windows::ffi::OsStrExt;
    let mut wide = name.encode_wide().collect::<Vec<_>>();
    let byte_length = wide
        .len()
        .checked_mul(2)
        .and_then(|length| u16::try_from(length).ok())
        .ok_or(DocumentError::InvalidPath)?;
    let unicode = UNICODE_STRING {
        Length: byte_length,
        MaximumLength: byte_length,
        Buffer: wide.as_mut_ptr(),
    };
    let attributes = OBJECT_ATTRIBUTES {
        Length: size_of::<OBJECT_ATTRIBUTES>() as u32,
        RootDirectory: parent.as_raw_handle(),
        ObjectName: &unicode,
        Attributes: 0,
        SecurityDescriptor: std::ptr::null(),
        SecurityQualityOfService: std::ptr::null(),
    };
    let mut handle: HANDLE = std::ptr::null_mut();
    // SAFETY: zero is the documented initial state and every supplied pointer stays live.
    let mut status: IO_STATUS_BLOCK = unsafe { zeroed() };
    let access = if directory {
        FILE_LIST_DIRECTORY | FILE_TRAVERSE | FILE_READ_ATTRIBUTES | SYNCHRONIZE
    } else if create {
        FILE_READ_DATA
            | FILE_WRITE_DATA
            | FILE_READ_ATTRIBUTES
            | FILE_WRITE_ATTRIBUTES
            | READ_CONTROL
            | WRITE_DAC
            | WRITE_OWNER
            | DELETE
            | SYNCHRONIZE
    } else {
        FILE_READ_DATA | FILE_READ_ATTRIBUTES | READ_CONTROL | SYNCHRONIZE
    };
    let options = FILE_OPEN_REPARSE_POINT
        | FILE_SYNCHRONOUS_IO_NONALERT
        | if directory {
            FILE_DIRECTORY_FILE
        } else {
            FILE_NON_DIRECTORY_FILE
        };
    // SAFETY: parent and bounded relative name remain live; the returned handle transfers once.
    let result = unsafe {
        NtCreateFile(
            &mut handle,
            access,
            &attributes,
            &mut status,
            std::ptr::null(),
            FILE_ATTRIBUTE_NORMAL,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            if create { FILE_CREATE } else { FILE_OPEN },
            options,
            std::ptr::null(),
            0,
        )
    };
    if result < 0 || handle.is_null() {
        return Err(DocumentError::Unavailable);
    }
    // SAFETY: successful NtCreateFile returned one owned handle.
    let file = unsafe { File::from_raw_handle(handle) };
    validate_opened_entry(&file, directory)?;
    Ok(file)
}

fn validate_opened_entry(file: &File, directory: bool) -> Result<(), DocumentError> {
    let metadata = file.metadata().map_err(|_| DocumentError::Unavailable)?;
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x400 != 0 {
            return Err(DocumentError::InvalidPath);
        }
    }
    if metadata.is_dir() != directory || (!directory && !metadata.is_file()) {
        return Err(DocumentError::InvalidPath);
    }
    Ok(())
}

fn validate_path(path: &str) -> Result<(), DocumentError> {
    graphhelm_tool_broker::path::RelativePath::parse(path)
        .map_err(|_| DocumentError::InvalidPath)?;
    if sensitive_path(path) || secret_shaped(path) {
        return Err(DocumentError::ProtectedContent);
    }
    for segment in path.split('/') {
        let stem = segment.split('.').next().unwrap_or("").to_ascii_uppercase();
        let reserved = matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
            || ["COM", "LPT"].iter().any(|prefix| {
                stem.strip_prefix(prefix).is_some_and(|suffix| {
                    matches!(
                        suffix,
                        "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"
                    )
                })
            });
        if reserved
            || segment.ends_with(['.', ' '])
            || segment.contains([':', '*', '?', '"', '<', '>', '|'])
        {
            return Err(DocumentError::InvalidPath);
        }
    }
    let extension = Path::new(path)
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if !matches!(
        extension.as_str(),
        "md" | "mdx" | "txt" | "rst" | "adoc" | "json" | "yaml" | "yml" | "toml" | "csv"
    ) {
        return Err(DocumentError::NotText);
    }
    Ok(())
}

/// Checks document bytes without reading or mutating the project.
///
/// # Errors
/// Refuses excess size, NUL bytes, and secret-shaped content.
pub fn validate_content(content: &str) -> Result<(), DocumentError> {
    if content.len() > MAX_DOCUMENT_BYTES {
        return Err(DocumentError::TooLarge);
    }
    if content.contains('\0') {
        return Err(DocumentError::NotText);
    }
    if secret_shaped(content) {
        return Err(DocumentError::ProtectedContent);
    }
    Ok(())
}

fn reject_multi_link_file(file: &File) -> Result<(), DocumentError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if file
            .metadata()
            .map_err(|_| DocumentError::Unavailable)?
            .nlink()
            != 1
        {
            return Err(DocumentError::InvalidPath);
        }
    }
    #[cfg(windows)]
    {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::Storage::FileSystem::{
            BY_HANDLE_FILE_INFORMATION, GetFileInformationByHandle,
        };
        let mut information = BY_HANDLE_FILE_INFORMATION::default();
        // SAFETY: the handle is borrowed from a live File and the output points to a
        // writable, correctly initialized Windows structure for the duration of the call.
        if unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut information) } == 0 {
            return Err(DocumentError::Unavailable);
        }
        if information.nNumberOfLinks != 1 {
            return Err(DocumentError::InvalidPath);
        }
    }
    Ok(())
}

/// `security.*` extended attributes as `(name, value)` pairs.
#[cfg(target_os = "linux")]
type SecurityXattrs = Vec<(Vec<u8>, Vec<u8>)>;

#[cfg(target_os = "linux")]
#[derive(Clone, Debug, PartialEq, Eq)]
struct UnixSecurity {
    uid: u32,
    gid: u32,
    mode: u32,
    acl: Option<Vec<u8>>,
    security_xattrs: SecurityXattrs,
}

#[cfg(target_os = "linux")]
impl UnixSecurity {
    const ACL_XATTR: &'static [u8] = b"system.posix_acl_access\0";
    const MAX_ACL_BYTES: usize = 64 * 1024;

    fn from_file(file: &File) -> Result<Self, DocumentError> {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};

        let metadata = file.metadata().map_err(|_| DocumentError::Unavailable)?;
        if !metadata.is_file() {
            return Err(DocumentError::NotText);
        }
        Ok(Self {
            uid: metadata.uid(),
            gid: metadata.gid(),
            mode: metadata.permissions().mode() & 0o7777,
            acl: Self::read_acl(file)?,
            security_xattrs: Self::read_security_xattrs(file)?,
        })
    }

    fn read_security_xattrs(file: &File) -> Result<SecurityXattrs, DocumentError> {
        use std::os::unix::io::AsRawFd;

        const MAX_LIST_BYTES: usize = 64 * 1024;
        let size = unsafe { libc::flistxattr(file.as_raw_fd(), std::ptr::null_mut(), 0) };
        if size < 0 {
            let error = std::io::Error::last_os_error().raw_os_error();
            return if error == Some(libc::ENOTSUP) || error == Some(libc::ENODATA) {
                Ok(Vec::new())
            } else {
                Err(DocumentError::Unavailable)
            };
        }
        let size = usize::try_from(size).map_err(|_| DocumentError::Unavailable)?;
        if size > MAX_LIST_BYTES {
            return Err(DocumentError::Unavailable);
        }
        let mut names = vec![0; size];
        let read =
            unsafe { libc::flistxattr(file.as_raw_fd(), names.as_mut_ptr().cast(), names.len()) };
        if read < 0 || usize::try_from(read).ok() != Some(size) {
            return Err(DocumentError::Unavailable);
        }
        let mut result = Vec::new();
        let mut total_value_bytes = 0usize;
        for name in names[..size].split(|byte| *byte == 0) {
            if !name.starts_with(b"security.") {
                continue;
            }
            let name = name.to_vec();
            let mut name_c = name.clone();
            name_c.push(0);
            let value_size = unsafe {
                libc::fgetxattr(
                    file.as_raw_fd(),
                    name_c.as_ptr().cast(),
                    std::ptr::null_mut(),
                    0,
                )
            };
            if value_size < 0 {
                return Err(DocumentError::Unavailable);
            }
            let value_size = usize::try_from(value_size).map_err(|_| DocumentError::Unavailable)?;
            total_value_bytes = total_value_bytes
                .checked_add(value_size)
                .ok_or(DocumentError::Unavailable)?;
            if value_size > Self::MAX_ACL_BYTES || total_value_bytes > MAX_LIST_BYTES {
                return Err(DocumentError::Unavailable);
            }
            let mut value = vec![0; value_size];
            let read = unsafe {
                libc::fgetxattr(
                    file.as_raw_fd(),
                    name_c.as_ptr().cast(),
                    value.as_mut_ptr().cast(),
                    value.len(),
                )
            };
            if read < 0 || usize::try_from(read).ok() != Some(value_size) {
                return Err(DocumentError::Unavailable);
            }
            result.push((name, value));
        }
        result.sort_by(|left, right| left.0.cmp(&right.0));
        Ok(result)
    }

    fn read_acl(file: &File) -> Result<Option<Vec<u8>>, DocumentError> {
        use std::os::unix::io::AsRawFd;

        let size = unsafe {
            libc::fgetxattr(
                file.as_raw_fd(),
                Self::ACL_XATTR.as_ptr().cast(),
                std::ptr::null_mut(),
                0,
            )
        };
        if size < 0 {
            let error = std::io::Error::last_os_error().raw_os_error();
            // Linux defines EOPNOTSUPP as the same value as ENOTSUP; one comparison covers
            // both names without an unreachable-pattern warning under -D warnings.
            return if error == Some(libc::ENODATA) || error == Some(libc::ENOTSUP) {
                Ok(None)
            } else {
                Err(DocumentError::Unavailable)
            };
        }
        let size = usize::try_from(size).map_err(|_| DocumentError::Unavailable)?;
        if size > Self::MAX_ACL_BYTES {
            return Err(DocumentError::Unavailable);
        }
        let mut acl = vec![0; size];
        let read = unsafe {
            libc::fgetxattr(
                file.as_raw_fd(),
                Self::ACL_XATTR.as_ptr().cast(),
                acl.as_mut_ptr().cast(),
                acl.len(),
            )
        };
        if read < 0 || usize::try_from(read).ok() != Some(size) {
            return Err(DocumentError::Unavailable);
        }
        Ok(Some(acl))
    }

    fn apply(&self, file: &File) -> Result<(), DocumentError> {
        use std::os::unix::fs::PermissionsExt;
        use std::os::unix::io::AsRawFd;

        let fd = file.as_raw_fd();
        // SAFETY: fd belongs to the live temporary File, and the uid/gid values came from
        // fstat on the existing regular file. The temporary is not published yet.
        if unsafe { libc::fchown(fd, self.uid, self.gid) } != 0 {
            return Err(DocumentError::Unavailable);
        }
        Self::apply_security_xattrs(file, &self.security_xattrs)?;
        // The temporary is still 0600 here. Replace any inherited ACL before restoring the
        // original mode, so named entries cannot become active before the final verification.
        match &self.acl {
            Some(acl) => {
                // SAFETY: fd is live and acl points to immutable bytes for this syscall.
                if unsafe {
                    libc::fsetxattr(
                        fd,
                        Self::ACL_XATTR.as_ptr().cast(),
                        acl.as_ptr().cast(),
                        acl.len(),
                        0,
                    )
                } != 0
                {
                    return Err(DocumentError::Unavailable);
                }
            }
            None => {
                // A parent default ACL may have been inherited by the temporary. Remove it so
                // the replacement does not gain permissions the original did not have.
                // SAFETY: fd is live and the xattr name is NUL-terminated.
                if unsafe { libc::fremovexattr(fd, Self::ACL_XATTR.as_ptr().cast()) } != 0 {
                    let error = std::io::Error::last_os_error().raw_os_error();
                    // EOPNOTSUPP is ENOTSUP's Linux alias, so this accepts either spelling.
                    if error != Some(libc::ENODATA) && error != Some(libc::ENOTSUP) {
                        return Err(DocumentError::Unavailable);
                    }
                }
            }
        }
        file.set_permissions(std::fs::Permissions::from_mode(self.mode))
            .map_err(|_| DocumentError::Unavailable)?;
        if Self::from_file(file)? != *self {
            return Err(DocumentError::Unavailable);
        }
        Ok(())
    }

    fn apply_security_xattrs(
        file: &File,
        expected: &[(Vec<u8>, Vec<u8>)],
    ) -> Result<(), DocumentError> {
        use std::os::unix::io::AsRawFd;

        // Only security.* is copied. User xattrs can contain unrelated secrets and are outside
        // the replacement security contract. Any inherited security label not in the source is
        // removed before verification.
        let existing = Self::read_security_xattrs(file)?;
        for (name, _) in &existing {
            let mut name_c = name.clone();
            name_c.push(0);
            if !expected.iter().any(|(wanted, _)| wanted == name)
                && unsafe { libc::fremovexattr(file.as_raw_fd(), name_c.as_ptr().cast()) } != 0
            {
                return Err(DocumentError::Unavailable);
            }
        }
        for (name, value) in expected {
            let mut name_c = name.clone();
            name_c.push(0);
            if unsafe {
                libc::fsetxattr(
                    file.as_raw_fd(),
                    name_c.as_ptr().cast(),
                    value.as_ptr().cast(),
                    value.len(),
                    0,
                )
            } != 0
            {
                return Err(DocumentError::Unavailable);
            }
        }
        if Self::read_security_xattrs(file)? != expected {
            return Err(DocumentError::Unavailable);
        }
        Ok(())
    }
}

fn read_document(file: &File) -> Result<DocumentSnapshot, DocumentError> {
    if !file
        .metadata()
        .map_err(|_| DocumentError::Unavailable)?
        .is_file()
    {
        return Err(DocumentError::NotText);
    }
    reject_multi_link_file(file)?;
    let mut bytes = Vec::new();
    file.take((MAX_DOCUMENT_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| DocumentError::Unavailable)?;
    if bytes.len() > MAX_DOCUMENT_BYTES {
        return Err(DocumentError::TooLarge);
    }
    let content = String::from_utf8(bytes).map_err(|_| DocumentError::NotText)?;
    validate_content(&content)?;
    Ok(snapshot(content))
}

fn snapshot(content: String) -> DocumentSnapshot {
    DocumentSnapshot {
        content_sha256: digest_hex(content.as_bytes()),
        content,
    }
}

/// Windows owner/group/DACL preservation, including inheritance protection. SACL audit
/// entries are not copied: reading them requires a privilege this editor does not request.
#[cfg(windows)]
struct WindowsSecurity {
    // DWORD alignment for the self-relative security descriptor returned by Windows.
    words: Vec<u32>,
    length: u32,
    protected: bool,
}

#[cfg(windows)]
impl PartialEq for WindowsSecurity {
    fn eq(&self, other: &Self) -> bool {
        use windows_sys::Win32::Security::SE_DACL_AUTO_INHERITED;
        // SetFileSecurityW preserves the access entries but clears this historical flag
        // (measured headers 0x84040001 -> 0x80040001, every other byte identical).
        // It records how the DACL was computed, not its permissions. Compare every owner,
        // group, ACL and protection byte; ignore only that provenance flag. In the Windows
        // self-relative descriptor the control field occupies bytes 2..4 of the first DWORD.
        let provenance = u32::from(SE_DACL_AUTO_INHERITED) << 16;
        self.length == other.length
            && self.protected == other.protected
            && self.words.len() == other.words.len()
            && self
                .words
                .iter()
                .zip(&other.words)
                .enumerate()
                .all(|(index, (left, right))| {
                    if index == 0 {
                        left & !provenance == right & !provenance
                    } else {
                        left == right
                    }
                })
    }
}

#[cfg(windows)]
impl Eq for WindowsSecurity {}

#[cfg(windows)]
impl WindowsSecurity {
    fn read(file: &File) -> Result<Self, DocumentError> {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::Security::{
            DACL_SECURITY_INFORMATION, GROUP_SECURITY_INFORMATION, GetKernelObjectSecurity,
            GetSecurityDescriptorControl, OWNER_SECURITY_INFORMATION, SE_DACL_PROTECTED,
        };
        let information =
            OWNER_SECURITY_INFORMATION | GROUP_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION;
        let mut length = 0u32;
        // SAFETY: path is NUL-terminated; a null buffer with zero length only queries size.
        unsafe {
            GetKernelObjectSecurity(
                file.as_raw_handle(),
                information,
                std::ptr::null_mut(),
                0,
                &mut length,
            );
        }
        if length == 0 || length > 64 * 1024 {
            return Err(DocumentError::Unavailable);
        }
        let mut words = vec![0u32; (length as usize).div_ceil(4)];
        let mut needed = length;
        // SAFETY: the aligned buffer has at least length writable bytes; all pointers live
        // through the synchronous call. A descriptor that grew is refused, never truncated.
        let read = unsafe {
            GetKernelObjectSecurity(
                file.as_raw_handle(),
                information,
                words.as_mut_ptr().cast(),
                length,
                &mut needed,
            )
        };
        if read == 0 || needed > length {
            return Err(DocumentError::Unavailable);
        }
        let mut control = 0u16;
        let mut revision = 0u32;
        // SAFETY: the descriptor was fully initialized by a successful GetFileSecurityW.
        if unsafe {
            GetSecurityDescriptorControl(words.as_mut_ptr().cast(), &mut control, &mut revision)
        } == 0
        {
            return Err(DocumentError::Unavailable);
        }
        Ok(Self {
            words,
            length: needed,
            protected: control & SE_DACL_PROTECTED != 0,
        })
    }

    fn apply(&self, file: &File) -> Result<(), DocumentError> {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::Security::{
            DACL_SECURITY_INFORMATION, EqualSid, GROUP_SECURITY_INFORMATION,
            GetSecurityDescriptorDacl, GetSecurityDescriptorGroup, GetSecurityDescriptorOwner,
            OWNER_SECURITY_INFORMATION, SE_DACL_PROTECTED, SetKernelObjectSecurity,
            SetSecurityDescriptorControl,
        };
        let destination = Self::read(file)?;
        let mut information = DACL_SECURITY_INFORMATION;
        let mut descriptor_words = self.words.clone();
        let descriptor = descriptor_words.as_mut_ptr().cast();
        // SAFETY: this mutable aligned copy is a complete self-relative descriptor.
        // SetFileSecurityW reads protection from the descriptor control, not information flags.
        if unsafe {
            SetSecurityDescriptorControl(
                descriptor,
                SE_DACL_PROTECTED,
                if self.protected { SE_DACL_PROTECTED } else { 0 },
            )
        } == 0
        {
            return Err(DocumentError::Unavailable);
        }
        let mut owner = std::ptr::null_mut();
        let mut group = std::ptr::null_mut();
        let mut dacl = std::ptr::null_mut();
        let mut defaulted = 0;
        let mut present = 0;
        // SAFETY: these queries only read the live descriptor and return pointers inside it.
        if unsafe {
            GetSecurityDescriptorOwner(descriptor, &mut owner, &mut defaulted) == 0
                || GetSecurityDescriptorGroup(descriptor, &mut group, &mut defaulted) == 0
                || GetSecurityDescriptorDacl(descriptor, &mut present, &mut dacl, &mut defaulted)
                    == 0
        } || present == 0
        {
            return Err(DocumentError::Unavailable);
        }
        let target_descriptor = destination.words.as_ptr().cast_mut().cast();
        let mut target_owner = std::ptr::null_mut();
        let mut target_group = std::ptr::null_mut();
        // SAFETY: destination is another complete Windows descriptor, alive through this call.
        if unsafe {
            GetSecurityDescriptorOwner(target_descriptor, &mut target_owner, &mut defaulted) == 0
                || GetSecurityDescriptorGroup(target_descriptor, &mut target_group, &mut defaulted)
                    == 0
        } || owner.is_null()
            || group.is_null()
            || target_owner.is_null()
            || target_group.is_null()
        {
            return Err(DocumentError::Unavailable);
        }
        // Existing identical identities need no WRITE_OWNER privilege. Requesting it anyway
        // refused ordinary owner-editable files on D:'s inherited Modify ACL (error 5).
        // Different identities still require the actual permission; failure stays closed.
        // SAFETY: all four SID pointers came from successful descriptor queries above.
        if unsafe { EqualSid(owner, target_owner) } == 0 {
            information |= OWNER_SECURITY_INFORMATION;
        }
        if unsafe { EqualSid(group, target_group) } == 0 {
            information |= GROUP_SECURITY_INFORMATION;
        }
        // SAFETY: path and complete descriptor remain live through this synchronous call.
        // SetNamedSecurityInfoW re-inherits ACEs and changed the observed D: descriptor;
        // SetFileSecurityW copies it without synthesizing additional inherited entries.
        if unsafe { SetKernelObjectSecurity(file.as_raw_handle(), information, descriptor) } == 0 {
            return Err(DocumentError::Unavailable);
        }
        Ok(())
    }

    #[cfg(test)]
    fn read_path(path: &Path) -> Result<Self, DocumentError> {
        use std::os::windows::fs::OpenOptionsExt;
        use windows_sys::Win32::Storage::FileSystem::READ_CONTROL;

        let mut options = OpenOptions::new();
        options.read(true).access_mode(READ_CONTROL);
        let file = options.open(path).map_err(|_| DocumentError::Unavailable)?;
        Self::read(&file)
    }

    #[cfg(test)]
    fn apply_path(&self, path: &Path) -> Result<(), DocumentError> {
        use std::os::windows::fs::OpenOptionsExt;
        use windows_sys::Win32::Storage::FileSystem::{READ_CONTROL, WRITE_DAC};

        let mut options = OpenOptions::new();
        options.read(true).access_mode(READ_CONTROL | WRITE_DAC);
        let file = options.open(path).map_err(|_| DocumentError::Unavailable)?;
        self.apply(&file)
    }
}

struct Temporary {
    file: File,
    #[cfg(unix)]
    parent: File,
    #[cfg(unix)]
    name: String,
    published: bool,
}

impl Temporary {
    fn create(parent: &File, name: &str) -> Result<Self, DocumentError> {
        #[cfg(unix)]
        let file = {
            use std::ffi::CString;
            use std::os::fd::{AsRawFd, FromRawFd};
            let child = CString::new(name).map_err(|_| DocumentError::InvalidPath)?;
            // SAFETY: retained parent and NUL-terminated unique child name remain live.
            let fd = unsafe {
                libc::openat(
                    parent.as_raw_fd(),
                    child.as_ptr(),
                    libc::O_RDWR
                        | libc::O_CREAT
                        | libc::O_EXCL
                        | libc::O_NOFOLLOW
                        | libc::O_CLOEXEC,
                    0o600,
                )
            };
            if fd < 0 {
                return Err(DocumentError::Unavailable);
            }
            // SAFETY: openat returned one fresh owned descriptor.
            unsafe { File::from_raw_fd(fd) }
        };
        #[cfg(windows)]
        let file = nt_open_child(parent, OsStr::new(name), false, true)?;
        #[cfg(not(any(unix, windows)))]
        return Err(DocumentError::Unavailable);
        Ok(Self {
            file,
            #[cfg(unix)]
            parent: parent.try_clone().map_err(|_| DocumentError::Unavailable)?,
            #[cfg(unix)]
            name: name.to_owned(),
            published: false,
        })
    }

    fn publish(&mut self, parent: &File, destination: &str) -> Result<(), DocumentError> {
        #[cfg(unix)]
        #[allow(clippy::needless_return)]
        {
            use std::ffi::CString;
            use std::os::fd::AsRawFd;
            let source =
                CString::new(self.name.as_str()).map_err(|_| DocumentError::InvalidPath)?;
            let destination = CString::new(destination).map_err(|_| DocumentError::InvalidPath)?;
            // SAFETY: both retained directory descriptors and names stay live through renameat.
            if unsafe {
                libc::renameat(
                    self.parent.as_raw_fd(),
                    source.as_ptr(),
                    parent.as_raw_fd(),
                    destination.as_ptr(),
                )
            } != 0
            {
                return Err(DocumentError::SaveUnconfirmed);
            }
            self.published = true;
            parent
                .sync_all()
                .map_err(|_| DocumentError::SaveUnconfirmed)?;
            return Ok(());
        }
        #[cfg(windows)]
        {
            use std::mem::{offset_of, size_of, zeroed};
            use std::os::windows::io::AsRawHandle;
            use windows_sys::Wdk::Storage::FileSystem::{
                FILE_RENAME_INFORMATION, FileRenameInformation, NtSetInformationFile,
            };
            use windows_sys::Win32::System::IO::IO_STATUS_BLOCK;
            let name = destination.encode_utf16().collect::<Vec<_>>();
            let bytes = offset_of!(FILE_RENAME_INFORMATION, FileName)
                .checked_add(name.len() * size_of::<u16>())
                .ok_or(DocumentError::Unavailable)?;
            let mut storage = vec![0usize; bytes.div_ceil(size_of::<usize>())];
            let info = storage.as_mut_ptr().cast::<FILE_RENAME_INFORMATION>();
            // SAFETY: aligned storage is large enough for the header and complete UTF-16 name.
            unsafe {
                (*info).Anonymous.ReplaceIfExists = true;
                (*info).RootDirectory = parent.as_raw_handle();
                (*info).FileNameLength =
                    u32::try_from(name.len() * 2).map_err(|_| DocumentError::Unavailable)?;
                std::ptr::copy_nonoverlapping(
                    name.as_ptr(),
                    (*info).FileName.as_mut_ptr(),
                    name.len(),
                );
            }
            // SAFETY: zero is the documented initial state; all supplied handles and buffers live.
            let mut io_status: IO_STATUS_BLOCK = unsafe { zeroed() };
            // SAFETY: source handle, target directory handle, and initialized buffer are live.
            let status = unsafe {
                NtSetInformationFile(
                    self.file.as_raw_handle(),
                    &mut io_status,
                    info.cast_const().cast(),
                    u32::try_from(bytes).map_err(|_| DocumentError::Unavailable)?,
                    FileRenameInformation,
                )
            };
            if status < 0 {
                return Err(DocumentError::SaveUnconfirmed);
            }
            self.published = true;
            self.file
                .sync_all()
                .map_err(|_| DocumentError::SaveUnconfirmed)?;
            Ok(())
        }
        #[cfg(not(any(unix, windows)))]
        Err(DocumentError::Unavailable)
    }
}

impl Drop for Temporary {
    fn drop(&mut self) {
        if self.published {
            return;
        }
        #[cfg(unix)]
        {
            use std::ffi::CString;
            use std::os::fd::AsRawFd;
            if let Ok(name) = CString::new(self.name.as_str()) {
                // SAFETY: the retained parent and NUL-terminated child name remain live.
                unsafe { libc::unlinkat(self.parent.as_raw_fd(), name.as_ptr(), 0) };
            }
        }
        #[cfg(windows)]
        {
            use std::os::windows::io::AsRawHandle;
            use windows_sys::Win32::Storage::FileSystem::{
                FILE_DISPOSITION_INFO, FileDispositionInfo, SetFileInformationByHandle,
            };
            let disposition = FILE_DISPOSITION_INFO { DeleteFile: true };
            // SAFETY: the owned temporary handle and initialized disposition stay live.
            unsafe {
                SetFileInformationByHandle(
                    self.file.as_raw_handle(),
                    FileDispositionInfo,
                    (&raw const disposition).cast(),
                    std::mem::size_of::<FILE_DISPOSITION_INFO>() as u32,
                )
            };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_and_conflict_preserve_other_files() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("rule.md"), "Old rule").unwrap();
        std::fs::write(dir.path().join("other.md"), "Unrelated").unwrap();
        let documents = ProjectDocuments::open(dir.path()).unwrap();
        let old = documents.read("rule.md").unwrap();
        let saved = documents
            .save("rule.md", &old.content_sha256, "New rule")
            .unwrap();
        assert_eq!(documents.read("rule.md").unwrap(), saved);
        assert_eq!(
            documents.save("rule.md", &old.content_sha256, "Stale"),
            Err(DocumentError::Conflict)
        );
        assert_eq!(
            std::fs::read_to_string(dir.path().join("rule.md")).unwrap(),
            "New rule"
        );
        assert_eq!(documents.read("other.md").unwrap().content, "Unrelated");
    }

    #[test]
    fn rejects_escape_and_windows_aliases() {
        for path in [
            "../rule.md",
            "/rule.md",
            "docs\\rule.md",
            "docs/rule.md:stream",
            "docs./rule.md",
            "docs /rule.md",
            "CON.md",
            "LPT1.txt",
            "COM¹.md",
        ] {
            assert_eq!(
                validate_path(path),
                Err(DocumentError::InvalidPath),
                "{path}"
            );
        }
    }

    #[test]
    fn refuses_binary_secrets_and_large_files() {
        let dir = tempfile::tempdir().unwrap();
        let documents = ProjectDocuments::open(dir.path()).unwrap();
        for (bytes, expected) in [
            (vec![0xff], DocumentError::NotText),
            (vec![0], DocumentError::NotText),
            (
                b"password=unsafe-example".to_vec(),
                DocumentError::ProtectedContent,
            ),
            (vec![b'a'; MAX_DOCUMENT_BYTES + 1], DocumentError::TooLarge),
        ] {
            std::fs::write(dir.path().join("rule.md"), bytes).unwrap();
            assert_eq!(documents.read("rule.md"), Err(expected));
        }
        assert_eq!(
            validate_path(".graphhelm/rule.md"),
            Err(DocumentError::ProtectedContent)
        );
        assert_eq!(
            validate_path("credential.key"),
            Err(DocumentError::ProtectedContent)
        );
    }

    #[test]
    fn project_lock_refuses_without_waiting() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("rule.md"), "Rule").unwrap();
        let documents = ProjectDocuments::open(dir.path()).unwrap();
        let before = documents.read("rule.md").unwrap();
        let _held = documents.lock().unwrap();
        assert!(!dir.path().join(".graphhelm-owner-documents.lock").exists());
        assert!(!dir.path().join(".graphhelm").exists());
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
        assert_eq!(
            documents.save("rule.md", &before.content_sha256, "Changed"),
            Err(DocumentError::Busy)
        );
    }

    #[test]
    fn project_root_parent_components_are_walked_by_handle() {
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().join("project");
        let child = project.join("child");
        std::fs::create_dir_all(&child).unwrap();
        std::fs::write(project.join("rule.md"), "Rule").unwrap();
        let through_parent = child.join("..");
        let documents = ProjectDocuments::open(&through_parent).unwrap();
        assert_eq!(documents.read("rule.md").unwrap().content, "Rule");
    }

    #[cfg(windows)]
    #[test]
    fn windows_path_aliases_share_one_project_lock() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("rule.md"), "Rule").unwrap();
        let ordinary = ProjectDocuments::open(dir.path()).unwrap();
        let verbatim = PathBuf::from(format!(r"\\?\{}", dir.path().display()));
        let aliased = ProjectDocuments::open(&verbatim).unwrap();
        let _held = ordinary.lock().unwrap();
        assert_eq!(aliased.lock().err(), Some(DocumentError::Busy));
    }

    #[cfg(windows)]
    #[test]
    fn project_lock_is_shared_across_processes_with_different_temp_settings() {
        use std::process::Child;
        use std::process::{Command, Stdio};
        use std::time::{Duration, Instant};

        struct ChildGuard(Option<Child>);
        impl Drop for ChildGuard {
            fn drop(&mut self) {
                if let Some(mut child) = self.0.take() {
                    let _ = child.kill();
                    let _ = child.wait();
                }
            }
        }

        let dir = tempfile::tempdir().unwrap();
        let other_temp = tempfile::tempdir().unwrap();
        let marker = other_temp.path().join("lock-held");
        std::fs::write(dir.path().join("rule.md"), "Rule").unwrap();
        let documents = ProjectDocuments::open(dir.path()).unwrap();
        let before = documents.read("rule.md").unwrap();
        let child = Command::new(std::env::current_exe().unwrap())
            .arg("--exact")
            .arg("documents::tests::windows_project_lock_child")
            .arg("--nocapture")
            .env("GRAPHHELM_DOCUMENT_LOCK_TEST_ROOT", dir.path())
            .env("GRAPHHELM_DOCUMENT_LOCK_TEST_MARKER", &marker)
            .env("TEMP", other_temp.path())
            .env("TMP", other_temp.path())
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .spawn()
            .unwrap();
        let mut child = ChildGuard(Some(child));
        let deadline = Instant::now() + Duration::from_secs(10);
        while !marker.is_file() {
            if child.0.as_mut().unwrap().try_wait().unwrap().is_some() {
                panic!("document-lock child exited before acquiring the lock");
            }
            if Instant::now() >= deadline {
                panic!("document-lock child did not acquire the lock before the deadline");
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(
            documents.save("rule.md", &before.content_sha256, "Changed"),
            Err(DocumentError::Busy)
        );
        drop(child.0.as_mut().unwrap().stdin.take());
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if let Some(status) = child.0.as_mut().unwrap().try_wait().unwrap() {
                assert!(status.success());
                break;
            }
            if Instant::now() >= deadline {
                panic!("document-lock child did not exit before the deadline");
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        child.0.take();
    }

    #[cfg(windows)]
    #[test]
    fn windows_project_lock_child() {
        use std::io::Read;

        let Some(root) = std::env::var_os("GRAPHHELM_DOCUMENT_LOCK_TEST_ROOT") else {
            return;
        };
        let marker = std::env::var_os("GRAPHHELM_DOCUMENT_LOCK_TEST_MARKER").unwrap();
        let documents = ProjectDocuments::open(Path::new(&root)).unwrap();
        let _held = documents.lock().unwrap();
        std::fs::write(marker, b"held").unwrap();
        let mut byte = [0_u8; 1];
        assert_eq!(std::io::stdin().read(&mut byte).unwrap(), 0);
    }

    #[test]
    fn refused_save_preserves_original_and_no_temporary_file() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("rule.md"), "Rule").unwrap();
        let documents = ProjectDocuments::open(dir.path()).unwrap();
        let before = documents.read("rule.md").unwrap();
        assert_eq!(
            documents.save("rule.md", &before.content_sha256, "password=unsafe-example"),
            Err(DocumentError::ProtectedContent)
        );
        assert_eq!(documents.read("rule.md").unwrap(), before);
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[cfg(unix)]
    #[test]
    fn preserves_permissions() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("rule.md");
        std::fs::write(&path, "Rule").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o640)).unwrap();
        let documents = ProjectDocuments::open(dir.path()).unwrap();
        let before = documents.read("rule.md").unwrap();
        documents
            .save("rule.md", &before.content_sha256, "Changed")
            .unwrap();
        assert_eq!(
            std::fs::metadata(path).unwrap().permissions().mode() & 0o777,
            0o640
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn preserves_unix_owner_group_and_posix_acl() {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        use std::os::unix::io::AsRawFd;

        const ACL_NAME: &[u8] = b"system.posix_acl_access\0";
        const ACL_USER_OBJ: u16 = 0x0001;
        const ACL_USER: u16 = 0x0002;
        const ACL_GROUP_OBJ: u16 = 0x0004;
        const ACL_MASK: u16 = 0x0010;
        const ACL_OTHER: u16 = 0x0020;

        fn acl_entry(tag: u16, perm: u16, id: u32) -> [u8; 8] {
            let mut entry = [0; 8];
            entry[..2].copy_from_slice(&tag.to_ne_bytes());
            entry[2..4].copy_from_slice(&perm.to_ne_bytes());
            entry[4..].copy_from_slice(&id.to_ne_bytes());
            entry
        }

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("rule.md");
        std::fs::write(&path, "Rule").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o640)).unwrap();
        let acl = [
            0x0002_u32.to_ne_bytes().as_slice(),
            &acl_entry(ACL_USER_OBJ, 0o6, 0),
            &acl_entry(ACL_USER, 0o4, 65_534),
            &acl_entry(ACL_GROUP_OBJ, 0o4, 0),
            &acl_entry(ACL_MASK, 0o4, 0),
            &acl_entry(ACL_OTHER, 0o0, 0),
        ]
        .concat();
        let descriptor = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&path)
            .unwrap();
        let before_security_xattrs = UnixSecurity::read_security_xattrs(&descriptor).unwrap();
        let set = unsafe {
            libc::fsetxattr(
                descriptor.as_raw_fd(),
                ACL_NAME.as_ptr().cast(),
                acl.as_ptr().cast(),
                acl.len(),
                0,
            )
        };
        let acl_supported = if set == 0 {
            true
        } else {
            let error = std::io::Error::last_os_error().raw_os_error();
            // Linux defines EOPNOTSUPP as the same value as ENOTSUP.
            assert!(error == Some(libc::ENOTSUP) || error == Some(libc::ENODATA));
            false
        };
        let before = std::fs::metadata(&path).unwrap();
        let before_acl = acl_supported.then(|| read_acl_xattr_for_test(&path));
        let documents = ProjectDocuments::open(dir.path()).unwrap();
        let snapshot = documents.read("rule.md").unwrap();
        documents
            .save("rule.md", &snapshot.content_sha256, "Changed")
            .unwrap();
        let after = std::fs::metadata(&path).unwrap();
        assert_eq!(after.uid(), before.uid());
        assert_eq!(after.gid(), before.gid());
        assert_eq!(
            after.permissions().mode() & 0o7777,
            before.permissions().mode() & 0o7777
        );
        if let Some(before_acl) = before_acl {
            assert_eq!(read_acl_xattr_for_test(&path), before_acl);
        }
        let after = File::open(&path).unwrap();
        assert_eq!(
            UnixSecurity::read_security_xattrs(&after).unwrap(),
            before_security_xattrs
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn unix_security_equality_detects_metadata_drift() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("rule.md");
        std::fs::write(&path, "Rule").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o640)).unwrap();
        let before = UnixSecurity::from_file(&File::open(&path).unwrap()).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        let after = UnixSecurity::from_file(&File::open(&path).unwrap()).unwrap();
        assert_ne!(after, before);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn preserves_unix_setid_bits_after_write() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("rule.md");
        std::fs::write(&path, "Rule").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o6600)).unwrap();
        let before_mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(before_mode & 0o6000, 0o6000);
        let documents = ProjectDocuments::open(dir.path()).unwrap();
        let snapshot = documents.read("rule.md").unwrap();
        documents
            .save("rule.md", &snapshot.content_sha256, "Changed")
            .unwrap();
        let after_mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(after_mode & 0o6000, before_mode & 0o6000);
    }

    #[cfg(target_os = "linux")]
    fn read_acl_xattr_for_test(path: &Path) -> Vec<u8> {
        use std::os::unix::io::AsRawFd;

        const ACL_NAME: &[u8] = b"system.posix_acl_access\0";
        let file = File::open(path).unwrap();
        let size = unsafe {
            libc::fgetxattr(
                file.as_raw_fd(),
                ACL_NAME.as_ptr().cast(),
                std::ptr::null_mut(),
                0,
            )
        };
        assert!(size >= 0);
        let mut acl = vec![0; size as usize];
        let read = unsafe {
            libc::fgetxattr(
                file.as_raw_fd(),
                ACL_NAME.as_ptr().cast(),
                acl.as_mut_ptr().cast(),
                acl.len(),
            )
        };
        assert_eq!(read, size);
        acl
    }

    #[cfg(windows)]
    #[test]
    fn saves_under_inherited_modify_acl_without_write_owner() {
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::Foundation::LocalFree;
        use windows_sys::Win32::Security::Authorization::{
            ConvertStringSecurityDescriptorToSecurityDescriptorW, SE_FILE_OBJECT,
            SetNamedSecurityInfoW,
        };
        use windows_sys::Win32::Security::{
            DACL_SECURITY_INFORMATION, GetSecurityDescriptorDacl,
            PROTECTED_DACL_SECURITY_INFORMATION,
        };
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().join("project");
        std::fs::create_dir(&project).unwrap();
        // D:'s observed inherited rights grant Modify, not WRITE_OWNER. Reproduce those
        // rights on an isolated fixture instead of depending on the machine's volume ACL.
        let sddl: Vec<u16> = "D:(A;OICI;0x1301bf;;;AU)"
            .encode_utf16()
            .chain(Some(0))
            .collect();
        let mut descriptor = std::ptr::null_mut();
        let mut size = 0;
        // SAFETY: a fixed valid SDDL input, live out-pointers; Windows allocates descriptor.
        assert_ne!(
            unsafe {
                ConvertStringSecurityDescriptorToSecurityDescriptorW(
                    sddl.as_ptr(),
                    1,
                    &mut descriptor,
                    &mut size,
                )
            },
            0
        );
        let mut dacl = std::ptr::null_mut();
        let mut present = 0;
        let mut defaulted = 0;
        // SAFETY: descriptor came from the successful converter and remains allocated.
        assert_ne!(
            unsafe {
                GetSecurityDescriptorDacl(descriptor, &mut present, &mut dacl, &mut defaulted)
            },
            0
        );
        let name: Vec<u16> = project.as_os_str().encode_wide().chain(Some(0)).collect();
        // SAFETY: name and descriptor-backed DACL remain live during the call.
        let applied = unsafe {
            SetNamedSecurityInfoW(
                name.as_ptr(),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                dacl,
                std::ptr::null(),
            )
        };
        // SAFETY: the converter allocates with LocalAlloc and this is its sole release.
        unsafe {
            LocalFree(descriptor);
        }
        assert_eq!(applied, 0);
        let path = project.join("rule.md");
        std::fs::write(&path, "Rule").unwrap();
        let before_security = WindowsSecurity::read_path(&path).unwrap();
        let documents = ProjectDocuments::open(&project).unwrap();
        let before = documents.read("rule.md").unwrap();
        documents
            .save("rule.md", &before.content_sha256, "Changed")
            .unwrap();
        assert_eq!(documents.read("rule.md").unwrap().content, "Changed");
        assert!(WindowsSecurity::read_path(&path).unwrap() == before_security);
    }

    #[cfg(windows)]
    #[test]
    fn preserves_windows_owner_group_and_protected_dacl() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("rule.md");
        std::fs::write(&path, "Rule").unwrap();
        let mut protected = WindowsSecurity::read_path(&path).unwrap();
        protected.protected = true;
        protected.apply_path(&path).unwrap();
        let before_security = WindowsSecurity::read_path(&path).unwrap();
        assert!(before_security.protected);
        let documents = ProjectDocuments::open(dir.path()).unwrap();
        let before = documents.read("rule.md").unwrap();
        documents
            .save("rule.md", &before.content_sha256, "Changed")
            .unwrap();
        assert!(WindowsSecurity::read_path(&path).unwrap() == before_security);
    }

    #[cfg(windows)]
    #[test]
    fn refuses_links_when_creation_is_permitted() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("rule.md"), "Rule").unwrap();
        if std::os::windows::fs::symlink_file(
            dir.path().join("rule.md"),
            dir.path().join("link.md"),
        )
        .is_err()
        {
            return; // Windows requires developer mode or symlink privilege.
        }
        let documents = ProjectDocuments::open(dir.path()).unwrap();
        assert_eq!(documents.read("link.md"), Err(DocumentError::InvalidPath));
    }

    #[cfg(any(unix, windows))]
    #[test]
    fn refuses_swapped_ancestor_links_without_touching_the_target() {
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().join("project");
        let documents_dir = project.join("docs");
        let outside = dir.path().join("outside");
        std::fs::create_dir_all(&documents_dir).unwrap();
        std::fs::create_dir(&outside).unwrap();
        std::fs::write(documents_dir.join("rule.md"), "Original").unwrap();
        std::fs::write(outside.join("rule.md"), "Outside").unwrap();
        let documents = ProjectDocuments::open(&project).unwrap();
        let before = documents.read("docs/rule.md").unwrap();
        let retained = project.join("retained-docs");
        std::fs::rename(&documents_dir, &retained).unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(&outside, &documents_dir).unwrap();
        #[cfg(windows)]
        if std::os::windows::fs::symlink_dir(&outside, &documents_dir).is_err() {
            return; // Windows requires developer mode or symlink privilege.
        }

        assert_eq!(
            documents.read("docs/rule.md"),
            Err(DocumentError::InvalidPath)
        );
        assert_eq!(
            documents.save("docs/rule.md", &before.content_sha256, "Changed"),
            Err(DocumentError::InvalidPath)
        );
        assert_eq!(
            std::fs::read_to_string(outside.join("rule.md")).unwrap(),
            "Outside"
        );
        assert_eq!(
            std::fs::read_to_string(retained.join("rule.md")).unwrap(),
            "Original"
        );
    }

    #[cfg(unix)]
    #[test]
    fn detects_real_directory_swap_before_publication() {
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().join("project");
        let documents_dir = project.join("docs");
        std::fs::create_dir_all(&documents_dir).unwrap();
        std::fs::write(documents_dir.join("rule.md"), "Same content").unwrap();
        let documents = ProjectDocuments::open(&project).unwrap();
        let original = documents.open_document("docs/rule.md").unwrap();
        let retained = project.join("retained-docs");
        std::fs::rename(&documents_dir, &retained).unwrap();
        std::fs::create_dir(&documents_dir).unwrap();
        std::fs::write(documents_dir.join("rule.md"), "Same content").unwrap();
        let impostor = documents.open_document("docs/rule.md").unwrap();

        assert!(!same_opened_document(&original, &impostor).unwrap());
        assert_eq!(
            std::fs::read_to_string(documents_dir.join("rule.md")).unwrap(),
            "Same content"
        );
        assert_eq!(
            std::fs::read_to_string(retained.join("rule.md")).unwrap(),
            "Same content"
        );
    }

    #[cfg(windows)]
    #[test]
    fn detects_real_file_swap_before_publication() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("rule.md");
        std::fs::write(&path, "Same content").unwrap();
        let documents = ProjectDocuments::open(dir.path()).unwrap();
        let original = documents.open_document("rule.md").unwrap();
        let retained = dir.path().join("retained-rule.md");
        std::fs::rename(&path, &retained).unwrap();
        std::fs::write(&path, "Same content").unwrap();
        let impostor = documents.open_document("rule.md").unwrap();

        assert!(!same_opened_document(&original, &impostor).unwrap());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "Same content");
        assert_eq!(std::fs::read_to_string(&retained).unwrap(), "Same content");
    }

    #[cfg(unix)]
    #[test]
    fn refuses_links() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("rule.md"), "Rule").unwrap();
        std::os::unix::fs::symlink(dir.path().join("rule.md"), dir.path().join("link.md")).unwrap();
        let documents = ProjectDocuments::open(dir.path()).unwrap();
        assert_eq!(documents.read("link.md"), Err(DocumentError::InvalidPath));
    }

    /// Each errno `open_child_file` can meet on Unix, asserted against the refusal it maps to: a
    /// link (live or dangling) is the `InvalidPath` Windows returns, and an absent name is still
    /// `Unavailable` rather than a link refusal.
    #[cfg(unix)]
    #[test]
    fn unix_open_failures_keep_links_and_absences_apart() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("rule.md"), "Rule").unwrap();
        std::os::unix::fs::symlink(dir.path().join("rule.md"), dir.path().join("live.md")).unwrap();
        std::os::unix::fs::symlink(dir.path().join("absent.md"), dir.path().join("dangling.md"))
            .unwrap();
        let documents = ProjectDocuments::open(dir.path()).unwrap();

        assert_eq!(documents.read("live.md"), Err(DocumentError::InvalidPath));
        assert_eq!(
            documents.read("dangling.md"),
            Err(DocumentError::InvalidPath),
            "a link is refused as a link whether or not its target exists"
        );
        assert_eq!(
            documents.read("missing.md"),
            Err(DocumentError::Unavailable),
            "an absent document is not a link refusal"
        );
        assert_eq!(documents.read("rule.md").unwrap().content, "Rule");
    }

    #[test]
    fn refuses_hard_links_to_protected_files_before_read_or_save() {
        let dir = tempfile::tempdir().unwrap();
        let protected = dir.path().join(".graphhelm").join("secret.md");
        std::fs::create_dir_all(protected.parent().unwrap()).unwrap();
        std::fs::write(&protected, "private rule").unwrap();
        std::fs::hard_link(&protected, dir.path().join("public.md")).unwrap();
        let documents = ProjectDocuments::open(dir.path()).unwrap();

        assert_eq!(
            documents.read("public.md"),
            Err(DocumentError::InvalidPath),
            "a hard-link alias must not expose protected plaintext"
        );
        assert_eq!(
            documents.save(
                "public.md",
                &digest_hex(b"private rule"),
                "changed through alias"
            ),
            Err(DocumentError::InvalidPath),
            "a hard-link alias must not permit writes"
        );
        assert_eq!(
            std::fs::read_to_string(protected).unwrap(),
            "private rule",
            "refused access must leave the protected inode unchanged"
        );
    }
}
