use super::{AdoptionError, failed};
use std::fs::File;
/// `security.*` extended attributes as `(name, value)` pairs.
#[cfg(target_os = "linux")]
type SecurityXattrs = Vec<(Vec<u8>, Vec<u8>)>;

#[cfg(target_os = "linux")]
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct UnixSecurity {
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

    pub(crate) fn from_file(file: &File) -> Result<Self, AdoptionError> {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};

        let metadata = file.metadata().map_err(|_| failed())?;
        if !metadata.is_file() {
            return Err(failed());
        }
        Ok(Self {
            uid: metadata.uid(),
            gid: metadata.gid(),
            mode: metadata.permissions().mode() & 0o7777,
            acl: Self::read_acl(file)?,
            security_xattrs: Self::read_security_xattrs(file)?,
        })
    }

    fn read_security_xattrs(file: &File) -> Result<SecurityXattrs, AdoptionError> {
        use std::os::unix::io::AsRawFd;

        const MAX_LIST_BYTES: usize = 64 * 1024;
        let size = unsafe { libc::flistxattr(file.as_raw_fd(), std::ptr::null_mut(), 0) };
        if size < 0 {
            let error = std::io::Error::last_os_error().raw_os_error();
            return if error == Some(libc::ENOTSUP) || error == Some(libc::ENODATA) {
                Ok(Vec::new())
            } else {
                Err(failed())
            };
        }
        let size = usize::try_from(size).map_err(|_| failed())?;
        if size > MAX_LIST_BYTES {
            return Err(failed());
        }
        let mut names = vec![0; size];
        let read =
            unsafe { libc::flistxattr(file.as_raw_fd(), names.as_mut_ptr().cast(), names.len()) };
        if read < 0 || usize::try_from(read).ok() != Some(size) {
            return Err(failed());
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
                return Err(failed());
            }
            let value_size = usize::try_from(value_size).map_err(|_| failed())?;
            total_value_bytes = total_value_bytes.checked_add(value_size).ok_or(failed())?;
            if value_size > Self::MAX_ACL_BYTES || total_value_bytes > MAX_LIST_BYTES {
                return Err(failed());
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
                return Err(failed());
            }
            result.push((name, value));
        }
        result.sort_by(|left, right| left.0.cmp(&right.0));
        Ok(result)
    }

    fn read_acl(file: &File) -> Result<Option<Vec<u8>>, AdoptionError> {
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
                Err(failed())
            };
        }
        let size = usize::try_from(size).map_err(|_| failed())?;
        if size > Self::MAX_ACL_BYTES {
            return Err(failed());
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
            return Err(failed());
        }
        Ok(Some(acl))
    }

    pub(crate) fn apply(&self, file: &File) -> Result<(), AdoptionError> {
        use std::os::unix::fs::PermissionsExt;
        use std::os::unix::io::AsRawFd;

        let fd = file.as_raw_fd();
        // SAFETY: fd belongs to the live temporary File, and the uid/gid values came from
        // fstat on the existing regular file. The temporary is not published yet.
        if unsafe { libc::fchown(fd, self.uid, self.gid) } != 0 {
            return Err(failed());
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
                    return Err(failed());
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
                        return Err(failed());
                    }
                }
            }
        }
        file.set_permissions(std::fs::Permissions::from_mode(self.mode))
            .map_err(|_| failed())?;
        if Self::from_file(file)? != *self {
            return Err(failed());
        }
        Ok(())
    }

    fn apply_security_xattrs(
        file: &File,
        expected: &[(Vec<u8>, Vec<u8>)],
    ) -> Result<(), AdoptionError> {
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
                return Err(failed());
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
                return Err(failed());
            }
        }
        if Self::read_security_xattrs(file)? != expected {
            return Err(failed());
        }
        Ok(())
    }
}
