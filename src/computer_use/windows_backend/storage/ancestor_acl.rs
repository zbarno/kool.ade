//! Ensure path components cannot be replaced by another Windows account.

use std::{io, ptr::null_mut};

use windows_sys::Win32::{
    Foundation::{HANDLE, LocalFree},
    Security::Authorization::{ConvertStringSidToSidW, GetSecurityInfo, SE_FILE_OBJECT},
    Security::{
        ACCESS_ALLOWED_ACE, DACL_SECURITY_INFORMATION, GetAce, GetSecurityDescriptorDacl,
        IsValidAcl, OWNER_SECURITY_INFORMATION,
    },
};

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

    for index in 0..u32::from(unsafe { (*dacl).AceCount }) {
        let mut ace = null_mut();
        // SAFETY: index is within the validated ACL.
        if unsafe { GetAce(dacl, index, &mut ace) } == 0 || ace.is_null() {
            return false;
        }
        // SAFETY: the ACE pointer belongs to the validated ACL.
        let header = unsafe { &*ace.cast::<windows_sys::Win32::Security::ACE_HEADER>() };
        match header.AceType {
            1 => continue, // ACCESS_DENIED_ACE grants no mutation rights.
            0 => {}
            _ => return false,
        }
        // SAFETY: type zero is ACCESS_ALLOWED_ACE.
        let allowed = unsafe { &*ace.cast::<ACCESS_ALLOWED_ACE>() };
        if allowed.Mask & MUTATION_RIGHTS == 0 {
            continue;
        }
        let sid = std::ptr::addr_of!(allowed.SidStart).cast_mut().cast();
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
