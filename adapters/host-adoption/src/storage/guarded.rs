//! Guarded publication retains the file actually displaced by the kernel operation. These files
//! are recovery evidence and are never automatically unlinked: an existing writer may still hold
//! the displaced inode. Journaled identities distinguish staged candidates from displaced files.
use super::*;
impl Root {
    /// Retire a transaction-owned private activation pointer without deleting evidence. A final
    /// source swap is preserved under the inactive name and refused by the post-rename digest.
    pub fn retire_owned(&self, from: &str, to: &str, expected: &str) -> Result<(), AdoptionError> {
        name_ok(from)?;
        name_ok(to)?;
        self.verify()?;
        let source = self.source(from)?;
        if crate::apply::digest(&source.read()?) != expected {
            return Err(failed());
        }
        #[cfg(windows)]
        {
            let guard = backup::windows_nt_child_shared(
                &self.file,
                from,
                false,
                false,
                true,
                windows_sys::Win32::Storage::FileSystem::FILE_SHARE_READ,
            )?
            .ok_or_else(failed)?;
            if identity(&guard)? != identity(&source.file)? {
                return Err(failed());
            }
            rename_no_replace(&guard, &self.file, to)?;
            guard.sync_all().map_err(|error| failed_io(&error))?;
        }
        #[cfg(target_os = "linux")]
        {
            use std::os::fd::AsRawFd;
            let old = std::ffi::CString::new(from).map_err(|_| failed())?;
            let new = std::ffi::CString::new(to).map_err(|_| failed())?;
            // SAFETY: live retained directory and bounded, NUL-terminated relative names.
            if unsafe {
                libc::renameat2(
                    self.file.as_raw_fd(),
                    old.as_ptr(),
                    self.file.as_raw_fd(),
                    new.as_ptr(),
                    libc::RENAME_NOREPLACE,
                )
            } != 0
            {
                return Err(failed_io(&std::io::Error::last_os_error()));
            }
            self.file.sync_all().map_err(|error| failed_io(&error))?;
        }
        #[cfg(not(any(windows, target_os = "linux")))]
        return Err(failed());
        if crate::apply::digest(&self.source(to)?.read()?) != expected {
            return Err(failed());
        }
        self.verify()
    }
}
#[derive(Clone, Copy)]
pub(crate) enum PublicationBoundary {
    Validated,
    #[cfg(windows)]
    Detached,
    Published,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct GuardRecord {
    pub directory: String,
    pub source_identity: Identity,
    pub candidate_identity: Identity,
    pub expected_digest: String,
    pub replacement_digest: String,
    pub restoring: bool,
}

pub(crate) struct Prepared {
    source: Source,
    directory: File,
    candidate: Option<File>,
    access: Access,
    current_access: Access,
    pub record: GuardRecord,
}

impl Source {
    pub fn prepare(
        self,
        expected: &str,
        bytes: &[u8],
        current_metadata: &Access,
        metadata: &Access,
        restoring: bool,
    ) -> Result<Prepared, AdoptionError> {
        self.check_current(expected, current_metadata)?;
        let name = format!(".graphhelm-adoption-{}", uuid::Uuid::new_v4());
        #[cfg(unix)]
        let directory = backup::unix_create_child_dir(&self.parent, &name)?;
        #[cfg(windows)]
        let directory = {
            let directory = backup::windows_create_child(&self.parent, &name, true)?;
            graphhelm_sealed_key_provider::protect_owner_only(&directory).map_err(|_| failed())?;
            drop(directory);
            backup::windows_nt_child(&self.parent, &name, true, false, true)?.ok_or_else(failed)?
        };
        let mut candidate = create_child(&directory, "candidate")?;
        candidate
            .write_all(bytes)
            .map_err(|error| failed_io(&error))?;
        metadata.apply(&candidate)?;
        if access(&candidate)? != *metadata {
            return Err(failed());
        }
        candidate.sync_all().map_err(|error| failed_io(&error))?;
        #[cfg(unix)]
        {
            directory.sync_all().map_err(|error| failed_io(&error))?;
            // The journal must never refer to a guard directory whose parent entry has not
            // reached durable storage before an exchange can move the original into it.
            self.parent.sync_all().map_err(|error| failed_io(&error))?;
        }
        let record = GuardRecord {
            directory: name,
            source_identity: identity(&self.file)?,
            candidate_identity: identity(&candidate)?,
            expected_digest: expected.into(),
            replacement_digest: crate::apply::digest(bytes),
            restoring,
        };
        Ok(Prepared {
            source: self,
            directory,
            candidate: Some(candidate),
            access: metadata.clone(),
            current_access: current_metadata.clone(),
            record,
        })
    }
    fn check_current(&self, expected: &str, metadata: &Access) -> Result<(), AdoptionError> {
        let now = Root::reopen(&self.root)?.source(&self.relative)?;
        if identity(&now.parent)? != identity(&self.parent)?
            || identity(&now.file)? != identity(&self.file)?
            || crate::apply::digest(&now.read()?) != expected
            || access(&now.file)? != *metadata
        {
            return Err(AdoptionError {
                reason: AdoptionReason::PlanStale,
            });
        }
        Ok(())
    }
}
impl Prepared {
    /// The caller must durably journal `record` before this method. The observer persists the
    /// Windows detached phase before no-replace publication. Linux performs one atomic exchange.
    pub fn publish(
        mut self,
        boundary: &mut dyn FnMut(PublicationBoundary) -> Result<(), AdoptionError>,
    ) -> Result<(), AdoptionError> {
        #[cfg(windows)]
        let (_pins, _source_guard, _directory_guard) = self.windows_guards()?;
        self.source
            .check_current(&self.record.expected_digest, &self.current_access)?;
        let candidate_file = self.candidate.take().ok_or_else(failed)?;
        if identity(&candidate_file)? != self.record.candidate_identity
            || crate::apply::digest(&read_file(&candidate_file, 1024 * 1024)?)
                != self.record.replacement_digest
        {
            return Err(failed());
        }
        boundary(PublicationBoundary::Validated)?;
        #[cfg(target_os = "linux")]
        {
            use std::os::fd::AsRawFd;
            let target =
                std::ffi::CString::new(self.source.name.as_str()).map_err(|_| unsafe_path())?;
            let candidate = c"candidate";
            // RENAME_EXCHANGE never destroys the displaced destination. The private directory
            // now contains that exact inode, including writes through pre-existing descriptors.
            if unsafe {
                libc::renameat2(
                    self.directory.as_raw_fd(),
                    candidate.as_ptr(),
                    self.source.parent.as_raw_fd(),
                    target.as_ptr(),
                    libc::RENAME_EXCHANGE,
                )
            } != 0
            {
                return Err(failed_io(&std::io::Error::last_os_error()));
            }
            self.source
                .parent
                .sync_all()
                .map_err(|error| failed_io(&error))?;
            self.directory
                .sync_all()
                .map_err(|error| failed_io(&error))?;
        }
        #[cfg(windows)]
        {
            // The handle excludes competing write/delete opens. Capture that same handle, never
            // resolve a potentially exchanged final path. Both renames are anchored/no-replace.
            rename_no_replace(&_source_guard, &self.directory, "displaced")?;
            _source_guard
                .sync_all()
                .map_err(|error| failed_io(&error))?;
            // The caller persists Detached before this callback returns. The pre-existing intent
            // already records both identities if the process dies before that extra sync.
            boundary(PublicationBoundary::Detached)?;
            rename_no_replace(&candidate_file, &self.source.parent, &self.source.name)?;
            candidate_file
                .sync_all()
                .map_err(|error| failed_io(&error))?;
        }
        boundary(PublicationBoundary::Published)?;
        let displaced = displaced(&self.directory)?.ok_or_else(failed)?;
        if identity(&displaced)? != self.record.source_identity
            || crate::apply::digest(&read_file(&displaced, 1024 * 1024)?)
                != self.record.expected_digest
            || access(&displaced)? != self.current_access
        {
            return Err(failed());
        }
        let current = Root::reopen(&self.source.root)?.source(&self.source.relative)?;
        if identity(&current.file)? != self.record.candidate_identity
            || crate::apply::digest(&current.read()?) != self.record.replacement_digest
            || access(&current.file)? != self.access
        {
            return Err(failed());
        }
        Ok(())
    }
    #[cfg(windows)]
    fn windows_guards(&self) -> Result<(Vec<File>, File, File), AdoptionError> {
        use std::os::windows::fs::OpenOptionsExt;
        use std::path::Component;
        use windows_sys::Win32::Storage::FileSystem::{
            FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_LIST_DIRECTORY,
            FILE_READ_ATTRIBUTES, FILE_SHARE_READ, FILE_SHARE_WRITE, READ_CONTROL,
        };
        let target = self.source.root.path.join(&self.source.relative);
        let parent = target.parent().ok_or_else(unsafe_path)?;
        let mut volume = PathBuf::new();
        let mut names = Vec::new();
        for part in parent.components() {
            match part {
                Component::Prefix(_) | Component::RootDir => volume.push(part.as_os_str()),
                Component::Normal(name) => {
                    names.push(name.to_str().ok_or_else(unsafe_path)?.to_owned())
                }
                _ => return Err(unsafe_path()),
            }
        }
        let file = std::fs::OpenOptions::new()
            .access_mode(FILE_READ_ATTRIBUTES | FILE_LIST_DIRECTORY | READ_CONTROL)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
            .open(volume)
            .map_err(|error| failed_io(&error))?;
        let mut pins = vec![file];
        for name in names {
            let next = backup::windows_nt_child_shared(
                pins.last().ok_or_else(failed)?,
                &name,
                true,
                false,
                false,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
            )?
            .ok_or_else(failed)?;
            pins.push(next);
        }
        if identity(pins.last().ok_or_else(failed)?)? != identity(&self.source.parent)? {
            return Err(failed());
        }
        let directory = backup::windows_nt_child_shared(
            &self.source.parent,
            &self.record.directory,
            true,
            false,
            false,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
        )?
        .ok_or_else(failed)?;
        if identity(&directory)? != identity(&self.directory)? {
            return Err(failed());
        }
        let source = backup::windows_nt_child_shared(
            &self.source.parent,
            &self.source.name,
            false,
            false,
            true,
            FILE_SHARE_READ,
        )?
        .ok_or_else(failed)?;
        if identity(&source)? != self.record.source_identity {
            return Err(failed());
        }
        Ok((pins, source, directory))
    }
}
fn displaced(directory: &File) -> Result<Option<File>, AdoptionError> {
    #[cfg(target_os = "linux")]
    {
        open_child(directory, "candidate")
    }
    #[cfg(windows)]
    {
        open_child(directory, "displaced")
    }
}
impl GuardRecord {
    pub fn verify(
        &self,
        root: &Root,
        path: &str,
        source_metadata: &Access,
        candidate_metadata: &Access,
    ) -> Result<(), AdoptionError> {
        name_ok(&self.directory)?;
        let source = root.source(path)?;
        let directory = private_guard(&source.parent, &self.directory)?;
        let Some(displaced) = displaced(&directory)? else {
            // Before a Windows publication the backup name is absent. A staged candidate must
            // still match its journaled identity; otherwise the intermediate state is ambiguous.
            let candidate = open_child(&directory, "candidate")?.ok_or_else(failed)?;
            if identity(&candidate)? != self.candidate_identity
                || crate::apply::digest(&read_file(&candidate, 1024 * 1024)?)
                    != self.replacement_digest
                || access(&candidate)? != *candidate_metadata
            {
                return Err(failed());
            }
            return Ok(());
        };
        let found = identity(&displaced)?;
        #[cfg(target_os = "linux")]
        if found == self.candidate_identity {
            if crate::apply::digest(&read_file(&displaced, 1024 * 1024)?) != self.replacement_digest
                || access(&displaced)? != *candidate_metadata
            {
                return Err(failed());
            }
            return Ok(());
        }
        if found != self.source_identity
            || crate::apply::digest(&read_file(&displaced, 1024 * 1024)?) != self.expected_digest
            || access(&displaced)? != *source_metadata
        {
            return Err(failed());
        }
        Ok(())
    }
}

#[cfg(windows)]
fn rename_no_replace(source: &File, parent: &File, name: &str) -> Result<(), AdoptionError> {
    use std::mem::{offset_of, size_of};
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Wdk::Storage::FileSystem::{
        FILE_RENAME_INFORMATION, FileRenameInformation, NtSetInformationFile,
    };
    name_ok(name)?;
    let name = name.encode_utf16().collect::<Vec<_>>();
    let len = offset_of!(FILE_RENAME_INFORMATION, FileName) + name.len() * 2;
    let mut buffer = vec![0usize; len.div_ceil(size_of::<usize>())];
    let info = buffer.as_mut_ptr().cast::<FILE_RENAME_INFORMATION>();
    unsafe {
        (*info).Anonymous.ReplaceIfExists = false;
        (*info).RootDirectory = parent.as_raw_handle();
        (*info).FileNameLength = (name.len() * 2) as u32;
        std::ptr::copy_nonoverlapping(name.as_ptr(), (*info).FileName.as_mut_ptr(), name.len());
    }
    let source_identity = identity(source)?;
    let parent_identity = identity(parent)?;
    let mut status = unsafe { std::mem::zeroed() };
    // A refused no-replace rename changes nothing on disk, so a retry leaves exactly the state
    // a crash before the first attempt would: the journal/boundary ordering is untouched.
    retry_transient_rename(
        || unsafe {
            NtSetInformationFile(
                source.as_raw_handle(),
                &mut status,
                info.cast(),
                len as u32,
                FileRenameInformation,
            )
        },
        // Retry only while the retained handles still name what the caller validated.
        || Ok(identity(source)? == source_identity && identity(parent)? == parent_identity),
        std::time::Instant::now,
        std::thread::sleep,
    )
    .map_err(|failure| match failure {
        RenameFailure::Status(code) => failed_ntstatus(code, NtOperation::GuardedNoReplace),
        RenameFailure::Check(error) => error,
    })
}

/// Why a bounded rename retry ended without success.
#[cfg(any(windows, test))]
#[derive(Debug)]
enum RenameFailure {
    /// The last NTSTATUS the rename returned.
    Status(i32),
    /// Re-proving the handles between attempts failed.
    Check(AdoptionError),
}

/// Run `rename` (returning an NTSTATUS) until it succeeds, fails with a non-transient status,
/// or the bound is spent: at most four attempts, and no new attempt once 100 ms have passed
/// since the first failure (the same bound as the atomic-replace and backup renames). Between
/// attempts `unchanged` must re-prove that the handles are the ones validated; `false` ends the
/// loop with the last status. The clock bounds whether another call may begin, not how long a
/// native call itself takes.
#[cfg(any(windows, test))]
fn retry_transient_rename(
    mut rename: impl FnMut() -> i32,
    mut unchanged: impl FnMut() -> Result<bool, AdoptionError>,
    mut now: impl FnMut() -> std::time::Instant,
    mut sleep: impl FnMut(std::time::Duration),
) -> Result<(), RenameFailure> {
    use std::time::Duration;
    let mut retry_until = None;
    let mut attempt = 0;
    loop {
        let code = rename();
        if code >= 0 {
            return Ok(());
        }
        let retry_until = *retry_until.get_or_insert_with(|| now() + Duration::from_millis(100));
        if !transient_rename_status(code) || attempt == 3 || now() >= retry_until {
            return Err(RenameFailure::Status(code));
        }
        sleep(Duration::from_millis(20));
        if now() >= retry_until || !unchanged().map_err(RenameFailure::Check)? {
            return Err(RenameFailure::Status(code));
        }
        attempt += 1;
    }
}
impl GuardRecord {
    /// A Windows crash can leave the destination absent between the two no-replace renames.
    /// Restore the exclusively held displaced inode before any subsequent application/restore.
    pub fn reconcile(
        &self,
        root: &Root,
        path: &str,
        source_metadata: &Access,
    ) -> Result<(), AdoptionError> {
        #[cfg(windows)]
        {
            use windows_sys::Win32::Storage::FileSystem::FILE_SHARE_READ;
            let (parent, name) = root.destination_parent(path)?;
            if open_child(&parent, &name)?.is_some() {
                return Ok(());
            }
            let directory = private_guard(&parent, &self.directory)?;
            let guard = backup::windows_nt_child_shared(
                &directory,
                "displaced",
                false,
                false,
                true,
                FILE_SHARE_READ,
            )?
            .ok_or_else(failed)?;
            if identity(&guard)? != self.source_identity
                || crate::apply::digest(&read_file(&guard, 1024 * 1024)?) != self.expected_digest
                || access(&guard)? != *source_metadata
            {
                return Err(failed());
            }
            root.verify()?;
            rename_no_replace(&guard, &parent, &name)?;
            guard.sync_all().map_err(|error| failed_io(&error))?;
        }
        #[cfg(not(windows))]
        let _ = (root, path, source_metadata);
        Ok(())
    }
}

fn private_guard(parent: &File, name: &str) -> Result<File, AdoptionError> {
    let directory = directory_child(parent, name)?;
    #[cfg(unix)]
    backup::unix_check_private_dir(&directory)?;
    #[cfg(windows)]
    backup::windows_check_private(&directory)?;
    Ok(directory)
}

#[cfg(test)]
mod tests {
    use super::{RenameFailure, retry_transient_rename};
    use graphhelm_protocols::adoption::{AdoptionError, AdoptionReason};
    use std::cell::Cell;
    use std::time::{Duration, Instant};

    const ACCESS_DENIED: i32 = 0xc000_0022_u32 as i32;
    const SHARING_VIOLATION: i32 = 0xc000_0043_u32 as i32;
    const NAME_COLLISION: i32 = 0xc000_0035_u32 as i32;
    /// Win32 maps this to ERROR_ACCESS_DENIED too, but it is not a transient hold.
    const DELETE_PENDING: i32 = 0xc000_0056_u32 as i32;

    /// Feed `statuses` in order to the retry loop with a fake clock that only moves on sleep.
    /// Returns the result, the number of rename attempts and the number of pauses.
    fn run(
        statuses: &[i32],
        unchanged: bool,
        pause: Duration,
    ) -> (Result<(), RenameFailure>, usize, usize) {
        let start = Instant::now();
        let elapsed = Cell::new(Duration::ZERO);
        let attempts = Cell::new(0);
        let pauses = Cell::new(0);
        let result = retry_transient_rename(
            || {
                let index = attempts.get();
                attempts.set(index + 1);
                statuses[index]
            },
            || Ok(unchanged),
            || start + elapsed.get(),
            |_| {
                pauses.set(pauses.get() + 1);
                elapsed.set(elapsed.get() + pause);
            },
        );
        (result, attempts.get(), pauses.get())
    }

    fn status(result: Result<(), RenameFailure>) -> Option<i32> {
        match result {
            Ok(()) => None,
            Err(RenameFailure::Status(code)) => Some(code),
            Err(RenameFailure::Check(_)) => panic!("unexpected check failure"),
        }
    }

    #[test]
    fn two_sharing_violations_then_success_publishes() {
        let statuses = [SHARING_VIOLATION, SHARING_VIOLATION, 0];
        let (result, attempts, pauses) = run(&statuses, true, Duration::from_millis(20));
        assert_eq!(status(result), None);
        assert_eq!((attempts, pauses), (3, 2));
    }

    #[test]
    fn access_denied_then_success_publishes() {
        let (result, attempts, _) = run(&[ACCESS_DENIED, 0], true, Duration::from_millis(20));
        assert_eq!(status(result), None);
        assert_eq!(attempts, 2);
    }

    #[test]
    fn a_non_transient_status_fails_at_once() {
        for code in [NAME_COLLISION, DELETE_PENDING] {
            let (result, attempts, pauses) = run(&[code, 0], true, Duration::from_millis(20));
            assert_eq!(status(result), Some(code));
            assert_eq!((attempts, pauses), (1, 0));
        }
    }

    #[test]
    fn a_persistent_sharing_violation_fails_after_four_attempts() {
        let (result, attempts, pauses) = run(&[SHARING_VIOLATION; 8], true, Duration::ZERO);
        assert_eq!(status(result), Some(SHARING_VIOLATION));
        assert_eq!((attempts, pauses), (4, 3));
    }

    #[test]
    fn the_time_budget_stops_retries_before_the_attempt_bound() {
        // 60 ms per pause: the second pause crosses 100 ms, so no third attempt begins.
        let statuses = [SHARING_VIOLATION; 8];
        let (result, attempts, pauses) = run(&statuses, true, Duration::from_millis(60));
        assert_eq!(status(result), Some(SHARING_VIOLATION));
        assert_eq!((attempts, pauses), (2, 2));
    }

    #[test]
    fn a_changed_handle_ends_the_retry_with_the_last_status() {
        let statuses = [SHARING_VIOLATION, 0];
        let (result, attempts, _) = run(&statuses, false, Duration::from_millis(20));
        assert_eq!(status(result), Some(SHARING_VIOLATION));
        assert_eq!(attempts, 1);
    }

    #[test]
    fn a_failed_recheck_is_returned_not_retried() {
        let attempts = Cell::new(0);
        let result = retry_transient_rename(
            || {
                attempts.set(attempts.get() + 1);
                SHARING_VIOLATION
            },
            || {
                Err(AdoptionError {
                    reason: AdoptionReason::RecoveryRequired,
                })
            },
            Instant::now,
            |_| {},
        );
        let Err(RenameFailure::Check(error)) = result else {
            panic!("a failed recheck must be returned as is");
        };
        assert!(matches!(error.reason, AdoptionReason::RecoveryRequired));
        assert_eq!(attempts.get(), 1);
    }
}
