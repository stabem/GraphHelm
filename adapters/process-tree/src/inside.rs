//! Which processes hold a directory (#1456): their current directory or their executable is
//! inside it. A worktree cannot be deleted on Windows while a process sits in it, so the merge
//! cleanup stops these first. Only what the OS reports is matched; nothing is inferred from names.
use std::path::{Path, PathBuf};

/// `path` lexically inside `dir` (or equal to it), compared case-insensitively on Windows with
/// either separator. Both sides are taken as given, never resolved through a link.
#[must_use]
pub fn path_is_inside(path: &Path, dir: &Path) -> bool {
    let norm = |p: &Path| {
        let mut s = p.to_string_lossy().replace('\\', "/");
        if let Some(rest) = s.strip_prefix("//?/") {
            s = rest.to_owned();
        }
        while s.ends_with('/') && s.len() > 1 {
            s.pop();
        }
        if cfg!(windows) { s.to_lowercase() } else { s }
    };
    let (p, d) = (norm(path), norm(dir));
    !d.is_empty() && (p == d || p.starts_with(&format!("{d}/")))
}

/// Every other process whose current directory or executable lies inside `dir`. The calling
/// process is never listed. A process the caller may not inspect is skipped, not guessed at.
#[must_use]
pub fn processes_inside(dir: &Path) -> Vec<u32> {
    let me = std::process::id();
    list_processes()
        .into_iter()
        .filter(|&pid| pid != me && pid != 0)
        .filter(|&pid| {
            places_of(pid)
                .iter()
                .any(|place| path_is_inside(place, dir))
        })
        .collect()
}

#[cfg(target_os = "linux")]
fn list_processes() -> Vec<u32> {
    std::fs::read_dir("/proc")
        .map(|entries| {
            entries
                .filter_map(Result::ok)
                .filter_map(|e| e.file_name().to_str()?.parse().ok())
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(target_os = "linux")]
fn places_of(pid: u32) -> Vec<PathBuf> {
    ["cwd", "exe"]
        .iter()
        .filter_map(|link| std::fs::read_link(format!("/proc/{pid}/{link}")).ok())
        .collect()
}

#[cfg(all(unix, not(target_os = "linux")))]
fn list_processes() -> Vec<u32> {
    Vec::new()
}

#[cfg(all(unix, not(target_os = "linux")))]
fn places_of(_pid: u32) -> Vec<PathBuf> {
    Vec::new()
}

#[cfg(windows)]
fn list_processes() -> Vec<u32> {
    use windows_sys::Win32::{
        Foundation::{CloseHandle, INVALID_HANDLE_VALUE},
        System::Diagnostics::ToolHelp::{
            CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
            TH32CS_SNAPPROCESS,
        },
    };
    let mut found = Vec::new();
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    if snapshot == INVALID_HANDLE_VALUE {
        return found;
    }
    let mut entry = PROCESSENTRY32W {
        dwSize: u32::try_from(std::mem::size_of::<PROCESSENTRY32W>()).unwrap_or(0),
        ..unsafe { std::mem::zeroed() }
    };
    let mut more = unsafe { Process32FirstW(snapshot, &mut entry) } != 0;
    while more {
        found.push(entry.th32ProcessID);
        more = unsafe { Process32NextW(snapshot, &mut entry) } != 0;
    }
    unsafe { CloseHandle(snapshot) };
    found
}

/// The executable path and, on 64-bit Windows, the current directory read from the process
/// parameters block of its PEB. Each read is bounded; any failure drops that place.
#[cfg(windows)]
fn places_of(pid: u32) -> Vec<PathBuf> {
    use std::os::windows::ffi::OsStringExt;
    use windows_sys::Win32::{
        Foundation::CloseHandle,
        System::Threading::{
            OpenProcess, PROCESS_QUERY_INFORMATION, PROCESS_QUERY_LIMITED_INFORMATION,
            PROCESS_VM_READ, QueryFullProcessImageNameW,
        },
    };
    let mut places = Vec::new();
    let mut handle = unsafe { OpenProcess(PROCESS_QUERY_INFORMATION | PROCESS_VM_READ, 0, pid) };
    let full = !handle.is_null();
    if !full {
        handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
        if handle.is_null() {
            return places;
        }
    }
    let mut buffer = [0u16; 1024];
    let mut len = u32::try_from(buffer.len()).unwrap_or(0);
    if unsafe { QueryFullProcessImageNameW(handle, 0, buffer.as_mut_ptr(), &mut len) } != 0 {
        let len = usize::try_from(len).unwrap_or(0).min(buffer.len());
        places.push(PathBuf::from(std::ffi::OsString::from_wide(&buffer[..len])));
    }
    #[cfg(target_pointer_width = "64")]
    if full && let Some(cwd) = current_directory(handle) {
        places.push(cwd);
    }
    unsafe { CloseHandle(handle) };
    places
}

#[cfg(all(windows, target_pointer_width = "64"))]
fn current_directory(handle: windows_sys::Win32::Foundation::HANDLE) -> Option<PathBuf> {
    use std::os::windows::ffi::OsStringExt;
    use windows_sys::Wdk::System::Threading::NtQueryInformationProcess;
    use windows_sys::Win32::System::Diagnostics::Debug::ReadProcessMemory;

    /// `PROCESS_BASIC_INFORMATION`, declared here so no kernel-type feature is needed.
    #[repr(C)]
    struct Basic {
        exit_status: i32,
        peb: usize,
        affinity: usize,
        priority: i32,
        pid: usize,
        parent: usize,
    }
    // x64 layout: PEB.ProcessParameters at 0x20; RTL_USER_PROCESS_PARAMETERS.CurrentDirectory
    // .DosPath (a UNICODE_STRING: u16 length in bytes, then the buffer pointer at +8) at 0x38.
    const PARAMETERS: usize = 0x20;
    const CURRENT_DIRECTORY: usize = 0x38;
    let read = |address: usize, out: *mut core::ffi::c_void, size: usize| -> bool {
        let mut got = 0usize;
        unsafe { ReadProcessMemory(handle, address as _, out, size, &mut got) != 0 && got == size }
    };
    let mut basic: Basic = unsafe { std::mem::zeroed() };
    let size = u32::try_from(std::mem::size_of::<Basic>()).ok()?;
    let status = unsafe {
        NtQueryInformationProcess(
            handle,
            0,
            (&raw mut basic).cast(),
            size,
            std::ptr::null_mut(),
        )
    };
    if status < 0 || basic.peb == 0 {
        return None;
    }
    let mut parameters = 0usize;
    if !read(basic.peb + PARAMETERS, (&raw mut parameters).cast(), 8) || parameters == 0 {
        return None;
    }
    let mut length = 0u16;
    let mut buffer = 0usize;
    if !read(parameters + CURRENT_DIRECTORY, (&raw mut length).cast(), 2)
        || !read(
            parameters + CURRENT_DIRECTORY + 8,
            (&raw mut buffer).cast(),
            8,
        )
        || buffer == 0
        || length == 0
    {
        return None;
    }
    let mut wide = vec![0u16; usize::from(length) / 2];
    if !read(buffer, wide.as_mut_ptr().cast(), usize::from(length)) {
        return None;
    }
    Some(PathBuf::from(std::ffi::OsString::from_wide(&wide)))
}

#[cfg(test)]
mod tests {
    use super::path_is_inside;
    use std::path::Path;

    #[test]
    fn a_sibling_with_a_longer_name_is_not_inside() {
        assert!(path_is_inside(Path::new("/a/wt/x"), Path::new("/a/wt")));
        assert!(path_is_inside(Path::new("/a/wt"), Path::new("/a/wt/")));
        assert!(!path_is_inside(Path::new("/a/wt-2/x"), Path::new("/a/wt")));
    }
}
