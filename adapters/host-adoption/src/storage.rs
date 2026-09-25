//! Retained-handle storage shared by apply and recovery. No source content is serialized here.
use crate::backup;
use graphhelm_protocols::adoption::{AdoptionError, AdoptionReason};
use serde::{Deserialize, Serialize};
#[cfg(test)]
use std::cell::Cell;
use std::cell::RefCell;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
mod guarded;
pub(crate) use guarded::{GuardRecord, PublicationBoundary};
#[cfg(target_os = "linux")]
mod security_unix;
#[cfg(windows)]
mod security_windows;
#[cfg(target_os = "linux")]
pub(crate) use security_unix::UnixSecurity as Access;
#[cfg(windows)]
pub(crate) use security_windows::WindowsSecurity as Access;

pub(crate) fn failed() -> AdoptionError {
    diagnostic_failure(None);
    AdoptionError {
        reason: AdoptionReason::RecoveryRequired,
    }
}

pub(crate) fn failed_io(error: &std::io::Error) -> AdoptionError {
    diagnostic_failure(Some(error));
    AdoptionError {
        reason: AdoptionReason::RecoveryRequired,
    }
}

#[cfg(windows)]
#[derive(Clone, Copy)]
pub(crate) enum NtOperation {
    AtomicReplace,
    GuardedNoReplace,
}

/// A Windows rename refused with STATUS_ACCESS_DENIED or STATUS_SHARING_VIOLATION (Win32
/// ERROR_ACCESS_DENIED 5 / ERROR_SHARING_VIOLATION 32) may only mean another process, such as a
/// real-time scanner, briefly holds the new file. Every rename that retries uses this one set.
#[cfg(any(windows, test))]
pub(crate) fn transient_rename_status(status: i32) -> bool {
    matches!(status as u32, 0xc000_0022 | 0xc000_0043)
}

#[cfg(windows)]
pub(crate) fn failed_ntstatus(status: i32, operation: NtOperation) -> AdoptionError {
    if diagnostic_enabled() {
        DIAGNOSTIC_STAGE.with(|stage| {
            let operation = match operation {
                NtOperation::AtomicReplace => "atomic_replace",
                NtOperation::GuardedNoReplace => "guarded_no_replace",
            };
            eprintln!(
                "GH_ADOPTION_DIAGNOSTIC stage={} operation={operation} os_code=none kind=ntstatus ntstatus={status}",
                stage.borrow().as_deref().unwrap_or("unknown"),
            );
        });
    }
    AdoptionError {
        reason: AdoptionReason::RecoveryRequired,
    }
}

/// Test fixtures can opt into a redacted breadcrumb when a broad storage error is returned.
/// The normal error contract stays unchanged and no diagnostic is emitted unless the fixture
/// explicitly enables `GRAPHHELM_ADOPTION_DIAGNOSTICS`.
pub(crate) fn diagnostic_stage(stage: &'static str) {
    if diagnostic_enabled() {
        DIAGNOSTIC_STAGE.with(|current| *current.borrow_mut() = Some(stage.into()));
    }
}

pub(crate) fn diagnostic_operation(index: usize) {
    if diagnostic_enabled() {
        DIAGNOSTIC_STAGE.with(|current| {
            *current.borrow_mut() = Some(format!("apply.operation[{index}].source"));
        });
    }
}

pub(crate) fn diagnostic_reset() {
    DIAGNOSTIC_STAGE.with(|current| *current.borrow_mut() = None);
}

#[cfg(test)]
pub(crate) struct DiagnosticGuard {
    previous: bool,
}

#[cfg(test)]
pub(crate) fn diagnostic_scope() -> DiagnosticGuard {
    let previous = DIAGNOSTIC_REQUESTED.with(|requested| {
        let previous = requested.get();
        requested.set(true);
        previous
    });
    DiagnosticGuard { previous }
}

#[cfg(test)]
impl Drop for DiagnosticGuard {
    fn drop(&mut self) {
        DIAGNOSTIC_REQUESTED.with(|requested| requested.set(self.previous));
        diagnostic_reset();
    }
}

fn diagnostic_failure(error: Option<&std::io::Error>) {
    if !diagnostic_enabled() {
        return;
    }
    DIAGNOSTIC_STAGE.with(|stage| {
        eprintln!(
            "GH_ADOPTION_DIAGNOSTIC stage={} os_code={} kind={}",
            stage.borrow().as_deref().unwrap_or("unknown"),
            error
                .and_then(std::io::Error::raw_os_error)
                .map_or_else(|| "none".to_owned(), |code| code.to_string()),
            error.map_or_else(|| "none".to_owned(), |value| format!("{:?}", value.kind())),
        );
    });
}

fn diagnostic_enabled() -> bool {
    cfg!(debug_assertions)
        && (diagnostic_requested() || std::env::var_os("GRAPHHELM_ADOPTION_DIAGNOSTICS").is_some())
}

#[cfg(test)]
fn diagnostic_requested() -> bool {
    DIAGNOSTIC_REQUESTED.with(Cell::get)
}

#[cfg(not(test))]
fn diagnostic_requested() -> bool {
    false
}

thread_local! {
    static DIAGNOSTIC_STAGE: RefCell<Option<String>> = const { RefCell::new(None) };
    #[cfg(test)]
    static DIAGNOSTIC_REQUESTED: Cell<bool> = const { Cell::new(false) };
    #[cfg(all(test, windows))]
    static RELEASE_ON_ATOMIC_PUBLISH_FAILURE: RefCell<Option<std::sync::mpsc::Sender<()>>> =
        const { RefCell::new(None) };
    #[cfg(all(test, windows))]
    static RELEASE_ACKNOWLEDGEMENT: RefCell<Option<std::sync::mpsc::Receiver<()>>> =
        const { RefCell::new(None) };
}

#[cfg(all(test, windows))]
fn observe_atomic_publish_failure(status: i32) {
    eprintln!(
        "GH_ADOPTION_ATOMIC_PUBLISH_FAILURE status=0x{:08x}",
        status as u32
    );
    RELEASE_ON_ATOMIC_PUBLISH_FAILURE.with(|sender| {
        if let Some(sender) = sender.borrow().as_ref() {
            let _ = sender.send(());
        }
    });
    RELEASE_ACKNOWLEDGEMENT.with(|acknowledgement| {
        if let Some(receiver) = acknowledgement.borrow_mut().take() {
            receiver
                .recv_timeout(std::time::Duration::from_secs(5))
                .expect("release thread must acknowledge the dropped handle");
        }
    });
}

#[cfg(all(test, windows))]
fn set_release_on_atomic_publish_failure(
    sender: Option<std::sync::mpsc::Sender<()>>,
    acknowledgement: Option<std::sync::mpsc::Receiver<()>>,
) {
    RELEASE_ON_ATOMIC_PUBLISH_FAILURE.with(|slot| *slot.borrow_mut() = sender);
    RELEASE_ACKNOWLEDGEMENT.with(|slot| *slot.borrow_mut() = acknowledgement);
}
pub(crate) fn unsafe_path() -> AdoptionError {
    AdoptionError {
        reason: AdoptionReason::PathUnsafe,
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Identity {
    pub device: u64,
    pub file: u64,
}
pub(crate) fn identity(file: &File) -> Result<Identity, AdoptionError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let m = file.metadata().map_err(|error| failed_io(&error))?;
        Ok(Identity {
            device: m.dev(),
            file: m.ino(),
        })
    }
    #[cfg(windows)]
    {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::Storage::FileSystem::{
            BY_HANDLE_FILE_INFORMATION, GetFileInformationByHandle,
        };
        let mut info = std::mem::MaybeUninit::<BY_HANDLE_FILE_INFORMATION>::uninit();
        if unsafe { GetFileInformationByHandle(file.as_raw_handle(), info.as_mut_ptr()) } == 0 {
            return Err(failed_io(&std::io::Error::last_os_error()));
        }
        let info = unsafe { info.assume_init() };
        Ok(Identity {
            device: info.dwVolumeSerialNumber.into(),
            file: (u64::from(info.nFileIndexHigh) << 32) | u64::from(info.nFileIndexLow),
        })
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct RootRecord {
    pub path: PathBuf,
    pub identity: Identity,
}
pub(crate) struct Root {
    pub file: File,
    pub record: RootRecord,
}
pub(crate) struct AuthorityLock(File);
impl Drop for AuthorityLock {
    fn drop(&mut self) {
        // CLOEXEC descriptors can be inherited by a concurrent fork until its exec. Closing
        // only our descriptor would leave the shared flock held during that unrelated window.
        let _ = fs2::FileExt::unlock(&self.0);
    }
}

/// Compare physical ancestors, including Windows path aliases/casing. The candidate may not yet
/// exist; every existing ancestor is still opened without following links before any creation.
pub(crate) fn require_disjoint_state(path: &Path, roots: &[&Root]) -> Result<(), AdoptionError> {
    if path.parent().is_none() || roots.iter().any(|root| root.record.path.parent().is_none()) {
        return Err(unsafe_path());
    }
    let mut state_identity = None;
    for ancestor in path.ancestors().take_while(|part| part.parent().is_some()) {
        match std::fs::symlink_metadata(ancestor) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(_) => return Err(unsafe_path()),
            Ok(_) => {}
        }
        let current = Root::observe(ancestor)?;
        if roots
            .iter()
            .any(|root| root.record.identity == current.record.identity)
        {
            return Err(unsafe_path());
        }
        if ancestor == path {
            state_identity = Some(current.record.identity);
        }
    }
    if let Some(state_identity) = state_identity {
        for root in roots {
            for ancestor in root
                .record
                .path
                .ancestors()
                .take_while(|part| part.parent().is_some())
            {
                if Root::observe(ancestor)?.record.identity == state_identity {
                    return Err(unsafe_path());
                }
            }
        }
    }
    Ok(())
}
impl Root {
    pub fn open(path: &Path, create: bool) -> Result<Self, AdoptionError> {
        let absolute = std::path::absolute(path).map_err(|_| unsafe_path())?;
        #[cfg(unix)]
        let file = backup::unix_open_dir_chain(&absolute, create)?;
        #[cfg(windows)]
        let file = backup::windows_open_directory_chain_access(&absolute, create, true)?;
        let record = RootRecord {
            path: absolute,
            identity: identity(&file)?,
        };
        Ok(Self { file, record })
    }
    pub fn observe(path: &Path) -> Result<Self, AdoptionError> {
        let absolute = std::path::absolute(path).map_err(|_| unsafe_path())?;
        #[cfg(unix)]
        let file = backup::unix_open_dir_chain(&absolute, false)?;
        #[cfg(windows)]
        let file = backup::windows_open_directory_chain(&absolute, false)?;
        let record = RootRecord {
            path: absolute,
            identity: identity(&file)?,
        };
        Ok(Self { file, record })
    }
    pub fn reopen(record: &RootRecord) -> Result<Self, AdoptionError> {
        let root = Self::open(&record.path, false)?;
        if root.record.identity != record.identity {
            return Err(unsafe_path());
        }
        Ok(root)
    }
    pub fn verify(&self) -> Result<(), AdoptionError> {
        Self::reopen(&self.record).map(|_| ())
    }
    pub fn private_child(&self, name: &str) -> Result<File, AdoptionError> {
        name_ok(name)?;
        #[cfg(unix)]
        {
            let file = backup::unix_open_child_dir(&self.file, name, true)?;
            backup::unix_check_private_dir(&file)?;
            Ok(file)
        }
        #[cfg(windows)]
        {
            backup::windows_open_or_create_private_dir(&self.file, name)
        }
    }
    /// Exclusive creation records ownership; an existing directory is never adopted implicitly.
    pub fn create_private_child(&self, name: &str) -> Result<Self, AdoptionError> {
        name_ok(name)?;
        self.verify()?;
        #[cfg(unix)]
        let file = backup::unix_create_child_dir(&self.file, name)?;
        #[cfg(windows)]
        let file = {
            let file = backup::windows_create_child(&self.file, name, true)?;
            graphhelm_sealed_key_provider::protect_owner_only(&file).map_err(|_| failed())?;
            file
        };
        let record = RootRecord {
            path: self.record.path.join(name),
            identity: identity(&file)?,
        };
        #[cfg(unix)]
        self.file.sync_all().map_err(|error| failed_io(&error))?;
        Ok(Self { file, record })
    }
    pub fn source(&self, relative: &str) -> Result<Source, AdoptionError> {
        self.source_optional(relative)?.ok_or(AdoptionError {
            reason: AdoptionReason::PlanStale,
        })
    }
    pub fn source_optional(&self, relative: &str) -> Result<Option<Source>, AdoptionError> {
        relative_ok(relative)?;
        self.verify()?;
        let mut parent = self.file.try_clone().map_err(|error| failed_io(&error))?;
        let mut components = relative.split('/').peekable();
        while let Some(part) = components.next() {
            if components.peek().is_none() {
                let Some(file) = open_child(&parent, part)? else {
                    return Ok(None);
                };
                regular(&file)?;
                return Ok(Some(Source {
                    parent,
                    file,
                    name: part.into(),
                    root: self.record.clone(),
                    relative: relative.into(),
                }));
            }
            let Some(next) = open_optional_directory_child(&parent, part)? else {
                return Ok(None);
            };
            parent = next;
        }
        Err(unsafe_path())
    }
    // Only `guarded::reconcile` calls this, and only on Windows; the unix branch below is kept so
    // the walk stays one function on both platforms (#869).
    #[cfg_attr(not(windows), allow(dead_code))]
    pub fn destination_parent(&self, relative: &str) -> Result<(File, String), AdoptionError> {
        relative_ok(relative)?;
        self.verify()?;
        let mut parent = self.file.try_clone().map_err(|error| failed_io(&error))?;
        let mut components = relative.split('/').peekable();
        while let Some(part) = components.next() {
            if components.peek().is_none() {
                return Ok((parent, part.into()));
            }
            #[cfg(unix)]
            {
                parent = backup::unix_open_child_dir(&parent, part, false)?;
            }
            #[cfg(windows)]
            {
                parent = backup::windows_nt_child(&parent, part, true, false, true)?
                    .ok_or_else(unsafe_path)?;
            }
        }
        Err(unsafe_path())
    }
    pub fn lock(&self, name: &str) -> Result<AuthorityLock, AdoptionError> {
        let file = match open_child(&self.file, name)? {
            Some(f) => f,
            None => create_child(&self.file, name)?,
        };
        regular(&file)?;
        fs2::FileExt::try_lock_exclusive(&file).map_err(|_| AdoptionError {
            reason: AdoptionReason::Busy,
        })?;
        let lock = AuthorityLock(file);
        self.verify()?;
        let current = open_child(&self.file, name)?.ok_or_else(unsafe_path)?;
        if identity(&current)? != identity(&lock.0)? {
            return Err(unsafe_path());
        }
        Ok(lock)
    }
}

fn open_optional_directory_child(parent: &File, name: &str) -> Result<Option<File>, AdoptionError> {
    name_ok(name)?;
    #[cfg(unix)]
    {
        let Some(file) = open_child(parent, name)? else {
            return Ok(None);
        };
        if !file.metadata().map_err(|error| failed_io(&error))?.is_dir() {
            return Err(unsafe_path());
        }
        Ok(Some(file))
    }
    #[cfg(windows)]
    {
        backup::windows_open_child(parent, name, true)
    }
}
pub(crate) fn relative_ok(value: &str) -> Result<(), AdoptionError> {
    if value.is_empty() || value.len() > 4096 {
        return Err(unsafe_path());
    }
    for part in value.split('/') {
        name_ok(part)?
    }
    Ok(())
}
fn name_ok(value: &str) -> Result<(), AdoptionError> {
    if value.is_empty()
        || matches!(value, "." | "..")
        || value.contains(['\\', '/', ':', '\0'])
        || value.ends_with(['.', ' '])
    {
        return Err(unsafe_path());
    }
    Ok(())
}
pub(crate) fn open_child(parent: &File, name: &str) -> Result<Option<File>, AdoptionError> {
    name_ok(name)?;
    #[cfg(windows)]
    {
        backup::windows_open_child(parent, name, false)
    }
    #[cfg(unix)]
    {
        use std::os::fd::{AsRawFd, FromRawFd};
        let name = std::ffi::CString::new(name).map_err(|_| unsafe_path())?;
        let fd = unsafe {
            libc::openat(
                parent.as_raw_fd(),
                name.as_ptr(),
                libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK,
            )
        };
        if fd < 0 {
            if std::io::Error::last_os_error().kind() == std::io::ErrorKind::NotFound {
                return Ok(None);
            }
            return Err(unsafe_path());
        }
        Ok(Some(unsafe { File::from_raw_fd(fd) }))
    }
}
pub(crate) fn create_child(parent: &File, name: &str) -> Result<File, AdoptionError> {
    name_ok(name)?;
    #[cfg(windows)]
    {
        let f = backup::windows_create_child(parent, name, false)?;
        graphhelm_sealed_key_provider::protect_owner_only(&f).map_err(|_| failed())?;
        Ok(f)
    }
    #[cfg(unix)]
    {
        use std::os::fd::{AsRawFd, FromRawFd};
        let name = std::ffi::CString::new(name).map_err(|_| unsafe_path())?;
        let fd = unsafe {
            libc::openat(
                parent.as_raw_fd(),
                name.as_ptr(),
                libc::O_RDWR | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW | libc::O_CLOEXEC,
                0o600,
            )
        };
        if fd < 0 {
            return Err(failed());
        }
        Ok(unsafe { File::from_raw_fd(fd) })
    }
}
fn regular(file: &File) -> Result<(), AdoptionError> {
    let m = file.metadata().map_err(|error| failed_io(&error))?;
    if !m.is_file() {
        return Err(unsafe_path());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if m.nlink() != 1 {
            return Err(unsafe_path());
        }
    }
    #[cfg(windows)]
    {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::Storage::FileSystem::{
            BY_HANDLE_FILE_INFORMATION, GetFileInformationByHandle,
        };
        let mut info = std::mem::MaybeUninit::<BY_HANDLE_FILE_INFORMATION>::uninit();
        if unsafe { GetFileInformationByHandle(file.as_raw_handle(), info.as_mut_ptr()) } == 0 {
            return Err(failed_io(&std::io::Error::last_os_error()));
        }
        if unsafe { info.assume_init() }.nNumberOfLinks != 1 {
            return Err(unsafe_path());
        }
    }
    Ok(())
}
pub(crate) fn read_file(file: &File, limit: u64) -> Result<Vec<u8>, AdoptionError> {
    regular(file)?;
    if file.metadata().map_err(|error| failed_io(&error))?.len() > limit {
        return Err(AdoptionError {
            reason: AdoptionReason::LimitExceeded,
        });
    }
    let mut file = file;
    file.seek(SeekFrom::Start(0))
        .map_err(|error| failed_io(&error))?;
    let mut bytes = Vec::new();
    file.take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| failed_io(&error))?;
    if bytes.len() as u64 > limit {
        return Err(AdoptionError {
            reason: AdoptionReason::LimitExceeded,
        });
    }
    Ok(bytes)
}
pub(crate) fn read_child(
    parent: &File,
    name: &str,
    limit: u64,
) -> Result<Option<Vec<u8>>, AdoptionError> {
    open_child(parent, name)?
        .map(|f| read_file(&f, limit))
        .transpose()
}
pub(crate) fn access(file: &File) -> Result<Access, AdoptionError> {
    #[cfg(windows)]
    {
        Access::read(file)
    }
    #[cfg(target_os = "linux")]
    {
        Access::from_file(file)
    }
}
pub(crate) struct Source {
    parent: File,
    pub file: File,
    name: String,
    root: RootRecord,
    relative: String,
}
impl Source {
    pub fn read(&self) -> Result<Vec<u8>, AdoptionError> {
        read_file(&self.file, 1024 * 1024)
    }
}
pub(crate) fn write_atomic(parent: &File, name: &str, bytes: &[u8]) -> Result<(), AdoptionError> {
    let mut tmp = Temporary::new(parent)?;
    tmp.file
        .write_all(bytes)
        .map_err(|error| failed_io(&error))?;
    tmp.file.sync_all().map_err(|error| failed_io(&error))?;
    tmp.publish(parent, name)
}

#[cfg(windows)]
fn destination_identity(parent: &File, name: &str) -> Result<Option<Identity>, AdoptionError> {
    let destination = match backup::windows_open_child(parent, name, false) {
        Ok(destination) => destination,
        Err(error) if error.reason == AdoptionReason::CoverageIncomplete => {
            return Err(failed());
        }
        Err(error) => return Err(error),
    };
    destination.map(|file| identity(&file)).transpose()
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
    fn new(parent: &File) -> Result<Self, AdoptionError> {
        let name = format!(".adoption-{}.pending", uuid::Uuid::new_v4());
        Ok(Self {
            file: create_child(parent, &name)?,
            #[cfg(unix)]
            parent: parent.try_clone().map_err(|error| failed_io(&error))?,
            #[cfg(unix)]
            name,
            published: false,
        })
    }
    fn publish(&mut self, parent: &File, name: &str) -> Result<(), AdoptionError> {
        name_ok(name)?;
        #[cfg(unix)]
        {
            use std::os::fd::AsRawFd;
            let old = std::ffi::CString::new(self.name.as_str()).map_err(|_| unsafe_path())?;
            let new = std::ffi::CString::new(name).map_err(|_| unsafe_path())?;
            if unsafe {
                libc::renameat(
                    self.parent.as_raw_fd(),
                    old.as_ptr(),
                    parent.as_raw_fd(),
                    new.as_ptr(),
                )
            } != 0
            {
                return Err(failed());
            }
            self.published = true;
            parent.sync_all().map_err(|error| failed_io(&error))?;
        }
        #[cfg(windows)]
        {
            use std::mem::{offset_of, size_of};
            use std::os::windows::io::AsRawHandle;
            use std::time::{Duration, Instant};
            use windows_sys::Wdk::Storage::FileSystem::{
                FILE_RENAME_INFORMATION, FileRenameInformation, NtSetInformationFile,
            };
            use windows_sys::Win32::System::IO::IO_STATUS_BLOCK;
            let destination_name = name;
            let name = destination_name.encode_utf16().collect::<Vec<_>>();
            let len = offset_of!(FILE_RENAME_INFORMATION, FileName) + name.len() * 2;
            let mut buf = vec![0usize; len.div_ceil(size_of::<usize>())];
            let info = buf.as_mut_ptr().cast::<FILE_RENAME_INFORMATION>();
            unsafe {
                (*info).Anonymous.ReplaceIfExists = true;
                (*info).RootDirectory = parent.as_raw_handle();
                (*info).FileNameLength = (name.len() * 2) as u32;
                std::ptr::copy_nonoverlapping(
                    name.as_ptr(),
                    (*info).FileName.as_mut_ptr(),
                    name.len(),
                );
            }
            let mut status: IO_STATUS_BLOCK = unsafe { std::mem::zeroed() };
            let source_identity = identity(&self.file)?;
            let parent_identity = identity(parent)?;
            let expected_destination_identity = destination_identity(parent, destination_name)?;
            let mut retry_until = None;
            let mut attempt = 0;
            let result = loop {
                let code = unsafe {
                    NtSetInformationFile(
                        self.file.as_raw_handle(),
                        &mut status,
                        info.cast(),
                        len as u32,
                        FileRenameInformation,
                    )
                };
                if code >= 0 {
                    self.published = true;
                    self.file.sync_all().map_err(|error| failed_io(&error))?;
                    break Ok(());
                }
                #[cfg(test)]
                observe_atomic_publish_failure(code);
                let retry_until =
                    retry_until.get_or_insert_with(|| Instant::now() + Duration::from_millis(100));
                let retryable = transient_rename_status(code);
                if !retryable || attempt == 3 || Instant::now() >= *retry_until {
                    break Err(failed_ntstatus(code, NtOperation::AtomicReplace));
                }
                // This clock bounds whether another native call may begin. It does not bound
                // the duration of NtSetInformationFile itself.
                std::thread::sleep(Duration::from_millis(20));
                if Instant::now() >= *retry_until
                    || identity(&self.file)? != source_identity
                    || identity(parent)? != parent_identity
                    || destination_identity(parent, destination_name)?
                        != expected_destination_identity
                {
                    break Err(failed_ntstatus(code, NtOperation::AtomicReplace));
                }
                if Instant::now() >= *retry_until {
                    break Err(failed_ntstatus(code, NtOperation::AtomicReplace));
                }
                attempt += 1;
            };
            result?;
        }
        Ok(())
    }
}
impl Drop for Temporary {
    fn drop(&mut self) {
        if self.published {
            return;
        }
        #[cfg(unix)]
        {
            use std::os::fd::AsRawFd;
            if let Ok(name) = std::ffi::CString::new(self.name.as_str()) {
                unsafe { libc::unlinkat(self.parent.as_raw_fd(), name.as_ptr(), 0) };
            }
        }
        #[cfg(windows)]
        {
            use std::os::windows::io::AsRawHandle;
            use windows_sys::Win32::Storage::FileSystem::{
                FILE_DISPOSITION_INFO, FileDispositionInfo, SetFileInformationByHandle,
            };
            let info = FILE_DISPOSITION_INFO { DeleteFile: true };
            unsafe {
                SetFileInformationByHandle(
                    self.file.as_raw_handle(),
                    FileDispositionInfo,
                    (&raw const info).cast(),
                    std::mem::size_of_val(&info) as u32,
                )
            };
        }
    }
}

pub(crate) fn directory_child(parent: &File, name: &str) -> Result<File, AdoptionError> {
    name_ok(name)?;
    #[cfg(unix)]
    {
        backup::unix_open_child_dir(parent, name, false)
    }
    #[cfg(windows)]
    {
        backup::windows_open_child(parent, name, true)?.ok_or_else(unsafe_path)
    }
}

#[cfg(all(test, windows))]
mod windows_atomic_publish_tests {
    use super::{set_release_on_atomic_publish_failure, write_atomic};
    use graphhelm_protocols::adoption::AdoptionReason;
    use std::io::Read;
    use std::os::windows::fs::OpenOptionsExt;
    use windows_sys::Win32::Storage::FileSystem::{FILE_SHARE_READ, FILE_SHARE_WRITE};

    #[test]
    fn destination_released_after_first_native_failure_is_published() {
        let root = tempfile::tempdir().unwrap();
        let parent = crate::backup::windows_open_directory_chain(root.path(), true).unwrap();
        let destination = root.path().join("current.json");
        std::fs::write(&destination, b"old").unwrap();
        let held = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
            .open(&destination)
            .unwrap();
        let (release_signal, release_ready) = std::sync::mpsc::channel();
        let (release_ack, release_acknowledged) = std::sync::mpsc::channel();
        let release = std::thread::spawn(move || {
            release_ready.recv().unwrap();
            drop(held);
            release_ack.send(()).unwrap();
        });
        let publish_parent = parent.try_clone().unwrap();
        let publish = std::thread::spawn(move || {
            set_release_on_atomic_publish_failure(Some(release_signal), Some(release_acknowledged));
            let result = write_atomic(&publish_parent, "current.json", b"new");
            set_release_on_atomic_publish_failure(None, None);
            result
        });

        publish.join().unwrap().unwrap();
        release.join().unwrap();
        assert_eq!(std::fs::read(&destination).unwrap(), b"new");
    }

    #[test]
    fn persistent_native_refusal_stays_recovery_required_and_preserves_destination() {
        let root = tempfile::tempdir().unwrap();
        let parent = crate::backup::windows_open_directory_chain(root.path(), true).unwrap();
        let destination = root.path().join("current.json");
        std::fs::write(&destination, b"old").unwrap();
        let held = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
            .open(&destination)
            .unwrap();

        let error = write_atomic(&parent, "current.json", b"new").unwrap_err();
        drop(held);
        assert_eq!(error.reason, AdoptionReason::RecoveryRequired);
        let mut bytes = Vec::new();
        std::fs::File::open(&destination)
            .unwrap()
            .read_to_end(&mut bytes)
            .unwrap();
        assert_eq!(bytes, b"old");
    }

    #[test]
    fn destination_denied_for_identity_read_stays_recovery_required() {
        let root = tempfile::tempdir().unwrap();
        let parent = crate::backup::windows_open_directory_chain(root.path(), true).unwrap();
        let destination = root.path().join("current.json");
        std::fs::write(&destination, b"old").unwrap();
        let held = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .share_mode(FILE_SHARE_WRITE)
            .open(&destination)
            .unwrap();

        let error = write_atomic(&parent, "current.json", b"new").unwrap_err();
        drop(held);
        assert_eq!(error.reason, AdoptionReason::RecoveryRequired);
        assert_eq!(std::fs::read(&destination).unwrap(), b"old");
    }

    #[test]
    fn destination_swap_after_native_failure_is_refused_and_preserved() {
        let root = tempfile::tempdir().unwrap();
        let parent = crate::backup::windows_open_directory_chain(root.path(), true).unwrap();
        let destination = root.path().join("current.json");
        std::fs::write(&destination, b"old").unwrap();
        let held = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
            .open(&destination)
            .unwrap();
        let (release_signal, release_ready) = std::sync::mpsc::channel();
        let (release_ack, release_acknowledged) = std::sync::mpsc::channel();
        let replacement_path = destination.clone();
        let release = std::thread::spawn(move || {
            release_ready.recv().unwrap();
            drop(held);
            std::fs::remove_file(&replacement_path).unwrap();
            std::fs::write(&replacement_path, b"replacement").unwrap();
            release_ack.send(()).unwrap();
        });
        let publish_parent = parent.try_clone().unwrap();
        let publish = std::thread::spawn(move || {
            set_release_on_atomic_publish_failure(Some(release_signal), Some(release_acknowledged));
            let result = write_atomic(&publish_parent, "current.json", b"new");
            set_release_on_atomic_publish_failure(None, None);
            result
        });

        let error = publish.join().unwrap().unwrap_err();
        release.join().unwrap();
        assert_eq!(error.reason, AdoptionReason::RecoveryRequired);
        assert_eq!(std::fs::read(&destination).unwrap(), b"replacement");
    }
}
