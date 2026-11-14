//! Ensure path components cannot be replaced by another Windows account.

use std::{io, mem::size_of, ptr::null_mut};

use windows_sys::Win32::{
    Foundation::{HANDLE, LocalFree},
    Security::Authorization::{ConvertStringSidToSidW, GetSecurityInfo, SE_FILE_OBJECT},
    Security::{
        ACCESS_ALLOWED_ACE, ACE_HEADER, ACL_SIZE_INFORMATION, AclSizeInformation,
        DACL_SECURITY_INFORMATION, GetAce, GetAclInformation, GetSecurityDescriptorDacl,
        IsValidAcl, OWNER_SECURITY_INFORMATION,
    },
};

use super::acl_memory::{read_current_process_bytes, read_current_process_value};

/// Reject any parent DACL that grants mutation or replacement rights to an
/// identity outside the current user, SYSTEM, Administrators, or Creator Owner.
pub(super) fn directory_parent_is_safe(handle: HANDLE, current_sid: &[u8]) -> io::Result<bool> {
    let mut dacl = null_mut();
    let mut owner = null_mut();
    let mut descriptor = null_mut();
    // SAFETY: output pointers are writable and the handle is an open directory.
    let status = unsafe {
        GetSecurityInfo(
            handle,
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION | OWNER_SECURITY_INFORMATION,
            &mut owner,
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
    // SAFETY: descriptor came from GetSecurityInfo and outputs are writable.
    let valid = !descriptor.is_null()
        && unsafe {
            GetSecurityDescriptorDacl(
                descriptor,
                &mut present,
                &mut descriptor_dacl,
                &mut defaulted,
            ) != 0
        }
        && present != 0
        && !descriptor_dacl.is_null();
    let result = if valid {
        // SAFETY: the ACL remains owned by the live descriptor.
        unsafe { parent_descriptor_is_safe(descriptor_dacl, owner, current_sid) }
    } else {
        false
    };
    // SAFETY: descriptor was allocated by GetSecurityInfo.
    unsafe { LocalFree(descriptor.cast()) };
    Ok(result)
}

unsafe fn parent_descriptor_is_safe(
    dacl: *mut windows_sys::Win32::Security::ACL,
    owner: *mut core::ffi::c_void,
    current_sid: &[u8],
) -> bool {
    // SAFETY: caller supplies a DACL extracted from a live descriptor.
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
        || acl_info.AclBytesInUse < size_of::<windows_sys::Win32::Security::ACL>() as u32
    {
        return false;
    }
    let system_sid = match sid_from_string("S-1-5-18") {
        Ok(sid) => sid,
        Err(_) => return false,
    };
    let admins_sid = match sid_from_string("S-1-5-32-544") {
        Ok(sid) => sid,
        Err(_) => return false,
    };
    let creator_sid = match sid_from_string("S-1-3-0") {
        Ok(sid) => sid,
        Err(_) => return false,
    };
    let installer_sid =
        match sid_from_string("S-1-5-80-956008885-3418522649-1831038044-1853292631-2271478464") {
            Ok(sid) => sid,
            Err(_) => return false,
        };
    if owner.is_null()
        || unsafe {
            windows_sys::Win32::Security::EqualSid(owner, current_sid.as_ptr().cast_mut().cast())
                == 0
                && windows_sys::Win32::Security::EqualSid(owner, system_sid.0) == 0
                && windows_sys::Win32::Security::EqualSid(owner, admins_sid.0) == 0
                && windows_sys::Win32::Security::EqualSid(owner, installer_sid.0) == 0
        }
    {
        return false;
    }
    const MUTATION_RIGHTS: u32 = 0x0000_0156 // write data, append, write EA, delete-child
        | 0x0001_0000 // delete
        | 0x0004_0000 // write DACL
        | 0x0008_0000 // write owner
        | 0x0100_0000 // access system security
        | 0x1000_0000 // generic all
        | 0x4000_0000; // generic write

    let acl_start = dacl as usize;
    let Some(acl_end) = acl_start.checked_add(acl_info.AclBytesInUse as usize) else {
        return false;
    };
    for index in 0..acl_info.AceCount {
        let mut ace = null_mut();
        // SAFETY: index is within the validated ACL.
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
        let Some(header) = read_current_process_value::<ACE_HEADER>(ace.cast()) else {
            return false;
        };
        let ace_size = usize::from(header.AceSize);
        let Some(ace_end) = ace_start.checked_add(ace_size) else {
            return false;
        };
        if ace_size < size_of::<ACE_HEADER>() || ace_end > acl_end {
            return false;
        }
        match header.AceType {
            1 => continue, // ACCESS_DENIED_ACE grants no mutation rights.
            0 => {}
            _ => return false,
        }
        if ace_size < size_of::<ACCESS_ALLOWED_ACE>() {
            return false;
        }
        // SAFETY: the ACE was range-checked and its type is ACCESS_ALLOWED_ACE.
        let Some(allowed) = read_current_process_value::<ACCESS_ALLOWED_ACE>(ace.cast()) else {
            return false;
        };
        if allowed.Mask & MUTATION_RIGHTS == 0 {
            continue;
        }
        let sid_offset = size_of::<ACE_HEADER>() + size_of::<u32>();
        // Validate the SID's variable-length body against the ACE before
        // passing the pointer to EqualSid.
        if sid_offset + 8 > ace_size {
            return false;
        }
        // SAFETY: sid_offset and the minimum SID header are within the
        // range-checked ACE above.
        let sid = unsafe { ace.cast::<u8>().add(sid_offset).cast::<core::ffi::c_void>() };
        // SAFETY: the SID header lies within the already range-checked ACE.
        let Some(sid_header) = read_current_process_bytes::<8>(sid) else {
            return false;
        };
        let sub_authority_count = usize::from(sid_header[1]);
        if sid_header[0] != 1 || sub_authority_count > 15 {
            return false;
        }
        let sid_size = 8 + sub_authority_count * size_of::<u32>();
        if sid_offset + sid_size > ace_size {
            return false;
        }
        // SAFETY: the SID is in the validated ACE; comparison SIDs are live.
        let trusted = unsafe {
            windows_sys::Win32::Security::EqualSid(sid, current_sid.as_ptr().cast_mut().cast()) != 0
                || windows_sys::Win32::Security::EqualSid(sid, system_sid.0) != 0
                || windows_sys::Win32::Security::EqualSid(sid, admins_sid.0) != 0
                || windows_sys::Win32::Security::EqualSid(sid, creator_sid.0) != 0
                || windows_sys::Win32::Security::EqualSid(sid, installer_sid.0) != 0
        };
        if !trusted {
            return false;
        }
    }
    true
}

fn sid_from_string(value: &str) -> io::Result<LocalSid> {
    let wide = value.encode_utf16().chain(Some(0)).collect::<Vec<_>>();
    let mut sid = null_mut();
    // SAFETY: value is NUL-terminated and `sid` is a writable output pointer.
    if unsafe { ConvertStringSidToSidW(wide.as_ptr(), &mut sid) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(LocalSid(sid))
}

struct LocalSid(*mut core::ffi::c_void);

impl Drop for LocalSid {
    fn drop(&mut self) {
        // SAFETY: SID was allocated by ConvertStringSidToSidW.
        unsafe { LocalFree(self.0.cast()) };
    }
}
