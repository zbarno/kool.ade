//! Win32 security descriptors for app-owned VM storage.

use std::{io, mem::size_of, ptr::null_mut};

use windows_sys::Win32::{
    Foundation::{HANDLE, LocalFree},
    Security::Authorization::{
        ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW,
        ConvertStringSidToSidW, GetSecurityInfo, SDDL_REVISION_1, SE_FILE_OBJECT, SetSecurityInfo,
    },
    Security::{
        ACCESS_ALLOWED_ACE, ACE_HEADER, ACL_SIZE_INFORMATION, AclSizeInformation,
        CONTAINER_INHERIT_ACE, DACL_SECURITY_INFORMATION, GetAce, GetAclInformation,
        GetSecurityDescriptorControl, GetSecurityDescriptorDacl, INHERITED_ACE, IsValidAcl,
        OBJECT_INHERIT_ACE, PROTECTED_DACL_SECURITY_INFORMATION, SE_DACL_PROTECTED,
        SECURITY_ATTRIBUTES,
    },
    Storage::FileSystem::FILE_ALL_ACCESS,
};

pub(super) struct PrivateDirectorySecurity {
    descriptor: *mut core::ffi::c_void,
    pub(super) attributes: SECURITY_ATTRIBUTES,
}

impl PrivateDirectorySecurity {
    pub(super) fn new(current_sid: &[u8]) -> io::Result<Self> {
        let sid_text = sid_string(current_sid)?;
        let sddl = format!("D:P(A;OICI;FA;;;{sid_text})(A;OICI;FA;;;SY)");
        let wide = sddl.encode_utf16().chain(Some(0)).collect::<Vec<_>>();
        let mut descriptor = null_mut();
        // SAFETY: SDDL is NUL-terminated and descriptor is a writable output pointer.
        if unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                wide.as_ptr(),
                SDDL_REVISION_1,
                &mut descriptor,
                null_mut(),
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        let attributes = SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: descriptor,
            bInheritHandle: 0,
        };
        Ok(Self {
            descriptor,
            attributes,
        })
    }
}

impl Drop for PrivateDirectorySecurity {
    fn drop(&mut self) {
        // SAFETY: the descriptor was allocated by ConvertStringSecurityDescriptorToSecurityDescriptorW.
        unsafe { LocalFree(self.descriptor.cast()) };
    }
}

pub(super) fn set_private_dacl(handle: HANDLE, current_sid: &[u8]) -> io::Result<()> {
    // The handle was opened with WRITE_DAC and points at the directory itself,
    // not a reparse target. Recreate the exact per-user ACL on existing roots.
    let security = PrivateDirectorySecurity::new(current_sid)?;
    let mut present = 0;
    let mut dacl = null_mut();
    let mut defaulted = 0;
    // SAFETY: the descriptor is valid and all outputs are writable.
    if unsafe {
        GetSecurityDescriptorDacl(security.descriptor, &mut present, &mut dacl, &mut defaulted)
    } == 0
        || present == 0
        || dacl.is_null()
    {
        return Err(io::Error::last_os_error());
    }
    let flags = DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION;
    // SAFETY: DACL remains live in `security` for the duration of the call.
    let status = unsafe {
        SetSecurityInfo(
            handle,
            SE_FILE_OBJECT,
            flags,
            null_mut(),
            null_mut(),
            dacl,
            null_mut(),
        )
    };
    if status == 0 {
        Ok(())
    } else {
        Err(io::Error::from_raw_os_error(status as i32))
    }
}

pub(super) fn private_acl_matches_current_user_and_system(
    handle: HANDLE,
    current_sid: &[u8],
) -> io::Result<bool> {
    acl_matches_current_user_and_system(handle, current_sid, true)
}

pub(super) fn preexisting_acl_is_safe_to_lock(
    handle: HANDLE,
    current_sid: &[u8],
) -> io::Result<bool> {
    acl_matches_current_user_and_system(handle, current_sid, false)
}

fn acl_matches_current_user_and_system(
    handle: HANDLE,
    current_sid: &[u8],
    require_protected: bool,
) -> io::Result<bool> {
    let mut dacl = null_mut();
    let mut descriptor = null_mut();
    // SAFETY: outputs are writable and the handle remains valid for the query.
    let status = unsafe {
        GetSecurityInfo(
            handle,
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION,
            null_mut(),
            null_mut(),
            &mut dacl,
            null_mut(),
            &mut descriptor,
        )
    };
    if status != 0 {
        return Err(io::Error::from_raw_os_error(status as i32));
    }
    let mut present = 0;
    let mut descriptor_dacl = null_mut();
    let mut defaulted = 0;
    let mut control = 0_u16;
    let mut revision = 0_u32;
    // SAFETY: descriptor came from GetSecurityInfo and outputs are writable.
    let protected = !descriptor.is_null()
        && unsafe { GetSecurityDescriptorControl(descriptor, &mut control, &mut revision) != 0 }
        && (!require_protected || control & SE_DACL_PROTECTED != 0);
    // SAFETY: descriptor came from GetSecurityInfo and outputs are writable.
    let valid_descriptor = !descriptor.is_null()
        && unsafe {
            GetSecurityDescriptorDacl(
                descriptor,
                &mut present,
                &mut descriptor_dacl,
                &mut defaulted,
            ) != 0
        };
    let safe = if protected && valid_descriptor && present != 0 && !descriptor_dacl.is_null() {
        // SAFETY: ACL remains owned by the live descriptor from GetSecurityInfo.
        unsafe { acl_has_only_user_and_system_full_access(descriptor_dacl, current_sid) }
    } else {
        false
    };
    // SAFETY: descriptor was allocated by GetSecurityInfo.
    unsafe { LocalFree(descriptor.cast()) };
    Ok(safe)
}

unsafe fn acl_has_only_user_and_system_full_access(
    dacl: *mut windows_sys::Win32::Security::ACL,
    current_sid: &[u8],
) -> bool {
    // SAFETY: caller supplies the DACL pointer extracted from a live descriptor.
    if unsafe { IsValidAcl(dacl) } == 0 {
        return false;
    }
    let mut acl_info = ACL_SIZE_INFORMATION::default();
    // SAFETY: dacl is the non-null ACL returned by the live descriptor; the
    // output is correctly sized for AclSizeInformation.
    if unsafe {
        GetAclInformation(
            dacl,
            (&mut acl_info as *mut ACL_SIZE_INFORMATION).cast(),
            size_of::<ACL_SIZE_INFORMATION>() as u32,
            AclSizeInformation,
        )
    } == 0
        || acl_info.AceCount != 2
        || acl_info.AclBytesInUse < size_of::<windows_sys::Win32::Security::ACL>() as u32
    {
        return false;
    }
    let system_sid_text = "S-1-5-18".encode_utf16().chain(Some(0)).collect::<Vec<_>>();
    let mut system_sid = null_mut();
    // SAFETY: the constant SID string is NUL-terminated and system_sid is writable.
    if unsafe { ConvertStringSidToSidW(system_sid_text.as_ptr(), &mut system_sid) } == 0 {
        return false;
    }
    let system_sid = LocalSid(system_sid);
    let mut saw_user = false;
    let mut saw_system = false;
    let acl_start = dacl as usize;
    let Some(acl_end) = acl_start.checked_add(acl_info.AclBytesInUse as usize) else {
        return false;
    };
    for index in 0..2 {
        let mut ace = null_mut();
        // SAFETY: index is within the validated two-ACE ACL.
        if unsafe { GetAce(dacl, index, &mut ace) } == 0 || ace.is_null() {
            return false;
        }
        let ace_start = ace as usize;
        let Some(header_end) = ace_start.checked_add(size_of::<ACE_HEADER>()) else {
            return false;
        };
        if ace_start < acl_start || header_end > acl_end {
            return false;
        }
        // SAFETY: GetAce succeeded and the complete header was checked to lie
        // within AclBytesInUse before this potentially unaligned copy.
        let header = unsafe { ace.cast::<ACE_HEADER>().read_unaligned() };
        let ace_size = usize::from(header.AceSize);
        let Some(ace_end) = ace_start.checked_add(ace_size) else {
            return false;
        };
        if ace_size < size_of::<ACCESS_ALLOWED_ACE>() || ace_end > acl_end {
            return false;
        }
        let explicit_flags = (OBJECT_INHERIT_ACE | CONTAINER_INHERIT_ACE) as u8;
        let inherited_flags = explicit_flags | INHERITED_ACE as u8;
        if header.AceType != 0
            || (header.AceFlags != explicit_flags && header.AceFlags != inherited_flags)
        {
            return false;
        }
        // SAFETY: the ACE was range-checked and its type is ACCESS_ALLOWED_ACE.
        let allowed = unsafe { ace.cast::<ACCESS_ALLOWED_ACE>().read_unaligned() };
        if allowed.Mask != FILE_ALL_ACCESS {
            return false;
        }
        let sid_offset = size_of::<ACE_HEADER>() + size_of::<u32>();
        if sid_offset + 8 > ace_size {
            return false;
        }
        // SAFETY: sid_offset and the minimum SID header are within the
        // range-checked ACE above.
        let ace_sid = unsafe { ace.cast::<u8>().add(sid_offset).cast::<core::ffi::c_void>() };
        // SAFETY: the SID header lies within the already range-checked ACE.
        let sid_header = unsafe { std::slice::from_raw_parts(ace_sid.cast::<u8>(), 8) };
        let sub_authority_count = usize::from(sid_header[1]);
        if sid_header[0] != 1
            || sub_authority_count > 15
            || sid_offset + 8 + sub_authority_count * size_of::<u32>() > ace_size
        {
            return false;
        }
        // SAFETY: ACE SID storage is valid and both comparison SIDs remain live.
        if unsafe {
            windows_sys::Win32::Security::EqualSid(ace_sid, current_sid.as_ptr().cast_mut().cast())
        } != 0
        {
            if saw_user {
                return false;
            }
            saw_user = true;
        } else if unsafe { windows_sys::Win32::Security::EqualSid(ace_sid, system_sid.0) } != 0 {
            if saw_system {
                return false;
            }
            saw_system = true;
        } else {
            return false;
        }
    }
    saw_user && saw_system
}

struct LocalSid(*mut core::ffi::c_void);

impl Drop for LocalSid {
    fn drop(&mut self) {
        // SAFETY: SID was allocated by ConvertStringSidToSidW.
        unsafe { LocalFree(self.0.cast()) };
    }
}

fn sid_string(sid: &[u8]) -> io::Result<String> {
    let mut string_sid = null_mut();
    // SAFETY: `sid` is copied from the current process TOKEN_USER record.
    if unsafe { ConvertSidToStringSidW(sid.as_ptr().cast_mut().cast(), &mut string_sid) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let mut length = 0;
    let value = unsafe {
        while *string_sid.add(length) != 0 {
            length += 1;
        }
        String::from_utf16_lossy(std::slice::from_raw_parts(string_sid, length))
    };
    // SAFETY: ConvertSidToStringSidW allocates with LocalAlloc.
    unsafe { LocalFree(string_sid.cast()) };
    Ok(value)
}
