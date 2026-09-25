//! Handle-relative publication for Windows. The output parent is kept open without
//! FILE_SHARE_DELETE, and both creation and rename resolve against that handle.

use std::ffi::{OsStr, OsString};
use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::mem::{offset_of, size_of, zeroed};
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
use std::os::windows::io::{AsRawHandle, FromRawHandle};
use std::path::Path;

use windows_sys::Wdk::Foundation::OBJECT_ATTRIBUTES;
use windows_sys::Wdk::Storage::FileSystem::{
    FILE_CREATE, FILE_NON_DIRECTORY_FILE, FILE_OPEN_REPARSE_POINT, FILE_RENAME_INFORMATION,
    FILE_SYNCHRONOUS_IO_NONALERT, FileRenameInformation, NtCreateFile, NtSetInformationFile,
};
use windows_sys::Win32::Foundation::{HANDLE, UNICODE_STRING};
use windows_sys::Win32::Storage::FileSystem::{
    DELETE, FILE_ADD_FILE, FILE_ATTRIBUTE_NORMAL, FILE_ATTRIBUTE_REPARSE_POINT,
    FILE_DISPOSITION_INFO, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
    FILE_READ_ATTRIBUTES, FILE_READ_DATA, FILE_SHARE_READ, FILE_SHARE_WRITE, FILE_TRAVERSE,
    FILE_WRITE_DATA, FileDispositionInfo, GetFinalPathNameByHandleW, SYNCHRONIZE,
    SetFileInformationByHandle,
};
use windows_sys::Win32::System::IO::IO_STATUS_BLOCK;

fn nt_error(operation: &str, status: i32) -> io::Error {
    io::Error::other(format!(
        "KIDX5 {operation} refused (NTSTATUS 0x{:08x})",
        status as u32
    ))
}

fn name_utf16(name: &OsStr) -> io::Result<Vec<u16>> {
    let wide = name.encode_wide().collect::<Vec<_>>();
    if wide.is_empty()
        || wide.len() > 255
        || wide.iter().any(|c| matches!(*c, 0 | 0x2f | 0x5c | 0x3a))
        || wide.last().is_some_and(|c| matches!(*c, 0x20 | 0x2e))
        || wide == [0x2e]
        || wide == [0x2e, 0x2e]
    {
        return Err(io::Error::other("KIDX5 invalid output file name"));
    }
    Ok(wide)
}

pub(super) fn open_parent(path: &Path) -> io::Result<File> {
    let file = OpenOptions::new()
        .access_mode(FILE_ADD_FILE | FILE_TRAVERSE | FILE_READ_ATTRIBUTES | SYNCHRONIZE)
        // Denying DELETE sharing prevents a direct rename of this parent while it is held.
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_dir() || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err(io::Error::other(
            "KIDX5 output parent is not a plain directory",
        ));
    }
    Ok(file)
}

fn final_path(file: &File) -> io::Result<Vec<String>> {
    let mut wide = vec![0u16; 32768];
    // SAFETY: the retained handle and writable UTF-16 buffer stay live through the call.
    let length = unsafe {
        GetFinalPathNameByHandleW(
            file.as_raw_handle(),
            wide.as_mut_ptr(),
            wide.len() as u32,
            0,
        )
    } as usize;
    if length == 0 || length >= wide.len() {
        return Err(io::Error::other("KIDX5 output parent location unavailable"));
    }
    let path = std::path::PathBuf::from(OsString::from_wide(&wide[..length]));
    Ok(path
        .components()
        .map(|component| component.as_os_str().to_string_lossy().to_lowercase())
        .collect())
}

pub(super) fn parent_is_within_repo(parent: &File, repo_root: &Path) -> io::Result<bool> {
    let repo = OpenOptions::new()
        .access_mode(FILE_TRAVERSE | FILE_READ_ATTRIBUTES | SYNCHRONIZE)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
        .open(repo_root)?;
    if !repo.metadata()?.is_dir() {
        return Err(io::Error::other("KIDX5 repository root is not a directory"));
    }
    let parent_path = final_path(parent)?;
    let repo_path = final_path(&repo)?;
    Ok(parent_path.len() >= repo_path.len()
        && parent_path
            .iter()
            .zip(repo_path.iter())
            .all(|(a, b)| a == b))
}

fn create_temp(parent: &File, name: &OsStr) -> io::Result<File> {
    let mut wide = name_utf16(name)?;
    let byte_len = u16::try_from(wide.len() * 2)
        .map_err(|_| io::Error::other("KIDX5 temporary name is too long"))?;
    let unicode = UNICODE_STRING {
        Length: byte_len,
        MaximumLength: byte_len,
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
    // SAFETY: zero is the documented initial state and all pointers stay live through the call.
    let mut status_block: IO_STATUS_BLOCK = unsafe { zeroed() };
    // SAFETY: the retained parent and bounded relative name stay live. FILE_CREATE prevents
    // opening an attacker-owned existing entry; the handle transfers to File exactly once.
    let status = unsafe {
        NtCreateFile(
            &mut handle,
            DELETE | FILE_READ_DATA | FILE_WRITE_DATA | FILE_READ_ATTRIBUTES | SYNCHRONIZE,
            &attributes,
            &mut status_block,
            std::ptr::null(),
            FILE_ATTRIBUTE_NORMAL,
            0,
            FILE_CREATE,
            FILE_NON_DIRECTORY_FILE | FILE_OPEN_REPARSE_POINT | FILE_SYNCHRONOUS_IO_NONALERT,
            std::ptr::null(),
            0,
        )
    };
    if status < 0 || handle.is_null() {
        return Err(nt_error("temporary create", status));
    }
    // SAFETY: successful NtCreateFile returned one owned handle.
    Ok(unsafe { File::from_raw_handle(handle) })
}

struct Temporary(File, bool);

impl Drop for Temporary {
    fn drop(&mut self) {
        if self.1 {
            return;
        }
        let disposition = FILE_DISPOSITION_INFO { DeleteFile: true };
        // SAFETY: delete applies to our retained file handle, never to a mutable path name.
        unsafe {
            SetFileInformationByHandle(
                self.0.as_raw_handle(),
                FileDispositionInfo,
                (&raw const disposition).cast(),
                size_of::<FILE_DISPOSITION_INFO>() as u32,
            );
        }
    }
}

pub(super) fn write_atomic(parent: &File, destination: &OsStr, bytes: &[u8]) -> io::Result<()> {
    let temporary_name = format!(".keel-{}.pending", uuid::Uuid::new_v4());
    write_atomic_named(parent, destination, bytes, OsStr::new(&temporary_name))
}

fn write_atomic_named(
    parent: &File,
    destination: &OsStr,
    bytes: &[u8],
    temporary_name: &OsStr,
) -> io::Result<()> {
    let dest = name_utf16(destination)?;
    let mut temp = Temporary(create_temp(parent, temporary_name)?, false);
    temp.0.write_all(bytes)?;
    temp.0.sync_all()?;

    let info_len = offset_of!(FILE_RENAME_INFORMATION, FileName)
        .checked_add(dest.len() * size_of::<u16>())
        .ok_or_else(|| io::Error::other("KIDX5 destination name is too long"))?;
    let mut storage = vec![0usize; info_len.div_ceil(size_of::<usize>())];
    let info = storage.as_mut_ptr().cast::<FILE_RENAME_INFORMATION>();
    // SAFETY: aligned storage covers the header and complete UTF-16 name.
    unsafe {
        (*info).Anonymous.ReplaceIfExists = true;
        (*info).RootDirectory = parent.as_raw_handle();
        (*info).FileNameLength = (dest.len() * 2) as u32;
        std::ptr::copy_nonoverlapping(dest.as_ptr(), (*info).FileName.as_mut_ptr(), dest.len());
    }
    // SAFETY: zero is the documented initial state and all handles/buffers remain live.
    let mut status_block: IO_STATUS_BLOCK = unsafe { zeroed() };
    // SAFETY: the source file handle, retained output parent, and initialized rename buffer live.
    let status = unsafe {
        NtSetInformationFile(
            temp.0.as_raw_handle(),
            &mut status_block,
            info.cast(),
            info_len as u32,
            FileRenameInformation,
        )
    };
    if status < 0 {
        return Err(nt_error("atomic publish", status));
    }
    temp.1 = true;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{open_parent, parent_is_within_repo, write_atomic_named};
    use std::ffi::OsStr;
    use std::fs;

    #[test]
    fn occupied_temporary_entry_refuses_without_overwriting_either_file() {
        // Contract: the temporary file must be created exclusively relative to the retained
        // parent. This catches a reversion to opening an existing attacker-controlled name.
        let directory = tempfile::tempdir().unwrap();
        let temp_name = ".keel-occupied.pending";
        fs::write(directory.path().join(temp_name), b"attacker entry").unwrap();
        fs::write(directory.path().join("index.json"), b"old index").unwrap();
        let parent = open_parent(directory.path()).unwrap();
        let error = write_atomic_named(
            &parent,
            OsStr::new("index.json"),
            b"new index",
            OsStr::new(temp_name),
        )
        .unwrap_err();
        assert!(error.to_string().contains("KIDX5"), "{error}");
        assert_eq!(
            fs::read(directory.path().join(temp_name)).unwrap(),
            b"attacker entry"
        );
        assert_eq!(
            fs::read(directory.path().join("index.json")).unwrap(),
            b"old index"
        );
    }

    #[test]
    fn retained_directory_location_exposes_a_repository_descendant() {
        // The caller's path check can be raced before this handle is acquired. The final
        // location must be derived from the handle, not checked through that mutable alias.
        let sandbox = tempfile::tempdir().unwrap();
        let repo = sandbox.path().join("repo");
        let inside = repo.join("output");
        let outside = sandbox.path().join("outside");
        fs::create_dir_all(&inside).unwrap();
        fs::create_dir(&outside).unwrap();
        assert!(parent_is_within_repo(&open_parent(&inside).unwrap(), &repo).unwrap());
        assert!(!parent_is_within_repo(&open_parent(&outside).unwrap(), &repo).unwrap());
    }
}
