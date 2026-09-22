use super::{AdoptionError, failed};
use std::fs::File;
/// Windows owner/group/DACL preservation, including inheritance protection. SACL audit
/// entries are not copied: reading them requires a privilege this editor does not request.
#[cfg(windows)]
#[derive(Clone, Debug)]
pub(crate) struct WindowsSecurity {
    // DWORD alignment for the self-relative security descriptor returned by Windows.
    words: Vec<u32>,
    protected: bool,
}

#[cfg(windows)]
impl PartialEq for WindowsSecurity {
    fn eq(&self, other: &Self) -> bool {
        match (serde_json::to_value(self), serde_json::to_value(other)) {
            (Ok(left), Ok(right)) => left == right,
            _ => false,
        }
    }
}

#[cfg(windows)]
impl Eq for WindowsSecurity {}

#[cfg(windows)]
impl WindowsSecurity {
    pub(crate) fn read(file: &File) -> Result<Self, AdoptionError> {
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
            return Err(failed());
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
            return Err(failed());
        }
        let mut control = 0u16;
        let mut revision = 0u32;
        // SAFETY: the descriptor was fully initialized by a successful GetFileSecurityW.
        if unsafe {
            GetSecurityDescriptorControl(words.as_mut_ptr().cast(), &mut control, &mut revision)
        } == 0
        {
            return Err(failed());
        }
        Ok(Self {
            words,
            protected: control & SE_DACL_PROTECTED != 0,
        })
    }

    pub(crate) fn apply(&self, file: &File) -> Result<(), AdoptionError> {
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
            return Err(failed());
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
            return Err(failed());
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
            return Err(failed());
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
            return Err(failed());
        }
        Ok(())
    }
}

// Persist the OS-parsed SDDL form, never deserialize raw pointers/offsets into a descriptor.
impl serde::Serialize for WindowsSecurity {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use windows_sys::Win32::Security::Authorization::ConvertSecurityDescriptorToStringSecurityDescriptorW;
        use windows_sys::Win32::Security::{
            DACL_SECURITY_INFORMATION, GROUP_SECURITY_INFORMATION, OWNER_SECURITY_INFORMATION,
        };
        let mut words = self.words.clone();
        // Historical inheritance provenance is not an access permission.
        words[0] &= !(u32::from(windows_sys::Win32::Security::SE_DACL_AUTO_INHERITED) << 16);
        let mut text = std::ptr::null_mut();
        let mut len = 0;
        if unsafe {
            ConvertSecurityDescriptorToStringSecurityDescriptorW(
                words.as_mut_ptr().cast(),
                1,
                OWNER_SECURITY_INFORMATION | GROUP_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
                &mut text,
                &mut len,
            )
        } == 0
        {
            return Err(serde::ser::Error::custom("access metadata unavailable"));
        }
        let result = String::from_utf16(unsafe {
            std::slice::from_raw_parts(text, len.saturating_sub(1) as usize)
        });
        unsafe { windows_sys::Win32::Foundation::LocalFree(text.cast()) };
        #[derive(serde::Serialize)]
        struct Wire<'a> {
            sddl: &'a str,
        }
        let result = result.map_err(|_| serde::ser::Error::custom("invalid access metadata"))?;
        // Windows may report reserved trailing UTF-16 NULs after the textual descriptor.
        Wire {
            sddl: result.trim_end_matches('\0'),
        }
        .serialize(serializer)
    }
}
impl<'de> serde::Deserialize<'de> for WindowsSecurity {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        use windows_sys::Win32::Security::Authorization::ConvertStringSecurityDescriptorToSecurityDescriptorW;
        use windows_sys::Win32::Security::{GetSecurityDescriptorControl, SE_DACL_PROTECTED};
        #[derive(serde::Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Wire {
            sddl: String,
        }
        let wire = Wire::deserialize(deserializer)?;
        if wire.sddl.len() > 65536 || wire.sddl.contains('\0') {
            return Err(serde::de::Error::custom("invalid access metadata"));
        }
        let text: Vec<u16> = wire.sddl.encode_utf16().chain(Some(0)).collect();
        let mut descriptor = std::ptr::null_mut();
        let mut length = 0;
        if unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                text.as_ptr(),
                1,
                &mut descriptor,
                &mut length,
            )
        } == 0
        {
            return Err(serde::de::Error::custom("invalid access metadata"));
        }
        let mut control = 0;
        let mut revision = 0;
        let valid = length > 0
            && length <= 65536
            && unsafe { GetSecurityDescriptorControl(descriptor, &mut control, &mut revision) }
                != 0;
        let mut words = vec![0u32; (length as usize).div_ceil(4)];
        if valid {
            unsafe {
                std::ptr::copy_nonoverlapping(
                    descriptor.cast::<u8>(),
                    words.as_mut_ptr().cast::<u8>(),
                    length as usize,
                )
            }
        }
        unsafe { windows_sys::Win32::Foundation::LocalFree(descriptor) };
        if !valid {
            return Err(serde::de::Error::custom("invalid access metadata"));
        }
        Ok(Self {
            words,
            protected: control & SE_DACL_PROTECTED != 0,
        })
    }
}
