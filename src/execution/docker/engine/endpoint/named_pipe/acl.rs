use std::io;
use std::mem::size_of;

use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, LocalFree};
use windows_sys::Win32::Security::Authorization::{GetSecurityInfo, SE_FILE_OBJECT};
use windows_sys::Win32::Security::{
    ACCESS_ALLOWED_ACE, ACE_HEADER, ACL_SIZE_INFORMATION, AclSizeInformation,
    DACL_SECURITY_INFORMATION, EqualSid, GetAclInformation, GetLengthSid, GetTokenInformation,
    IsValidAcl, IsValidSid, IsWellKnownSid, OWNER_SECURITY_INFORMATION, PSID, TOKEN_QUERY,
    TOKEN_USER, TokenUser, WinBuiltinAdministratorsSid, WinLocalSystemSid,
};
use windows_sys::Win32::System::SystemServices::ACCESS_ALLOWED_ACE_TYPE;
use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

mod policy;
#[cfg(test)]
use policy::Trustee;
use policy::{AclEntry, classify_trustee, trusted_pipe_acl};

pub(super) fn validate_handle(pipe: HANDLE) -> io::Result<()> {
    let current_user = current_user_sid()?;
    let mut owner: PSID = std::ptr::null_mut();
    let mut dacl = std::ptr::null_mut();
    let mut descriptor = std::ptr::null_mut();
    let status = unsafe {
        GetSecurityInfo(
            pipe,
            SE_FILE_OBJECT,
            OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
            &mut owner,
            std::ptr::null_mut(),
            &mut dacl,
            std::ptr::null_mut(),
            &mut descriptor,
        )
    };
    if status != 0 {
        return Err(io::Error::from_raw_os_error(status as i32));
    }

    let result = if owner.is_null() || dacl.is_null() || descriptor.is_null() {
        Err(untrusted_acl())
    } else {
        validate_security_descriptor(owner, dacl, &current_user)
    };
    if !descriptor.is_null() {
        unsafe { LocalFree(descriptor) };
    }
    result
}

fn validate_security_descriptor(
    owner: PSID,
    dacl: *mut windows_sys::Win32::Security::ACL,
    current_user: &[u8],
) -> io::Result<()> {
    if unsafe { IsValidAcl(dacl) } == 0
        || unsafe { IsValidSid(owner) } == 0
        || unsafe { IsValidSid(current_user.as_ptr() as PSID) } == 0
    {
        return Err(untrusted_acl());
    }
    let owner_is_trusted = unsafe {
        EqualSid(owner, current_user.as_ptr() as PSID) != 0
            || IsWellKnownSid(owner, WinLocalSystemSid) != 0
            || IsWellKnownSid(owner, WinBuiltinAdministratorsSid) != 0
    };
    if !owner_is_trusted {
        return Err(untrusted_acl());
    }

    let mut acl_info = ACL_SIZE_INFORMATION::default();
    if unsafe {
        GetAclInformation(
            dacl,
            (&mut acl_info as *mut ACL_SIZE_INFORMATION).cast(),
            size_of::<ACL_SIZE_INFORMATION>() as u32,
            AclSizeInformation,
        )
    } == 0
    {
        return Err(untrusted_acl());
    }
    let acl_start = dacl as usize;
    let acl_bytes = acl_info.AclBytesInUse as usize;
    if acl_bytes < size_of::<windows_sys::Win32::Security::ACL>() {
        return Err(untrusted_acl());
    }
    let acl_end = acl_start.checked_add(acl_bytes).ok_or_else(untrusted_acl)?;
    let ace_start_min = acl_start
        .checked_add(size_of::<windows_sys::Win32::Security::ACL>())
        .ok_or_else(untrusted_acl)?;

    let mut entries = Vec::new();
    for index in 0..acl_info.AceCount {
        let mut ace_ptr = std::ptr::null_mut();
        if unsafe { windows_sys::Win32::Security::GetAce(dacl, index, &mut ace_ptr) } == 0
            || ace_ptr.is_null()
        {
            return Err(untrusted_acl());
        }
        let ace_start = ace_ptr as usize;
        if ace_start < ace_start_min || ace_start >= acl_end {
            return Err(untrusted_acl());
        }
        let remaining_len = acl_end.checked_sub(ace_start).ok_or_else(untrusted_acl)?;
        let remaining = unsafe { std::slice::from_raw_parts(ace_ptr.cast::<u8>(), remaining_len) };
        let header_len = size_of::<ACE_HEADER>();
        if remaining.len() < header_len {
            return Err(untrusted_acl());
        }
        let ace_type = remaining[0];
        let ace_size = usize::from(u16::from_le_bytes([remaining[2], remaining[3]]));
        if ace_type != ACCESS_ALLOWED_ACE_TYPE as u8
            || ace_size < size_of::<ACCESS_ALLOWED_ACE>()
            || ace_size > remaining.len()
        {
            return Err(untrusted_acl());
        }
        let ace = &remaining[..ace_size];
        let sid_offset = header_len + size_of::<u32>();
        // SID's fixed header is eight bytes: revision, subauthority count, and identifier
        // authority. Bound that header before reading its count, then bound the full SID before
        // calling Windows SID helpers that inspect it.
        const SID_HEADER_SIZE: usize = 8;
        if ace_size < sid_offset + SID_HEADER_SIZE {
            return Err(untrusted_acl());
        }
        let subauthority_count = usize::from(ace[sid_offset + 1]);
        let subauthority_bytes = subauthority_count
            .checked_mul(size_of::<u32>())
            .ok_or_else(untrusted_acl)?;
        let sid_len = SID_HEADER_SIZE
            .checked_add(subauthority_bytes)
            .ok_or_else(untrusted_acl)?;
        if sid_len > ace.len() - sid_offset {
            return Err(untrusted_acl());
        }
        let sid = unsafe {
            ace_ptr
                .cast::<u8>()
                .add(sid_offset)
                .cast::<std::ffi::c_void>()
        };
        if unsafe { IsValidSid(sid) } == 0 || unsafe { GetLengthSid(sid) } as usize != sid_len {
            return Err(untrusted_acl());
        }
        let trustee = classify_trustee(sid, current_user);
        entries.push(AclEntry {
            trustee,
            mask: u32::from_le_bytes([
                ace[header_len],
                ace[header_len + 1],
                ace[header_len + 2],
                ace[header_len + 3],
            ]),
        });
    }

    if trusted_pipe_acl(
        true,
        unsafe { EqualSid(owner, current_user.as_ptr() as PSID) != 0 },
        unsafe {
            IsWellKnownSid(owner, WinLocalSystemSid) != 0
                || IsWellKnownSid(owner, WinBuiltinAdministratorsSid) != 0
        },
        &entries,
    ) {
        Ok(())
    } else {
        Err(untrusted_acl())
    }
}

pub(super) fn current_user_sid() -> io::Result<Vec<u8>> {
    let mut token: HANDLE = std::ptr::null_mut();
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
        return Err(io::Error::last_os_error());
    }

    let result = (|| {
        let mut required = 0;
        unsafe {
            GetTokenInformation(token, TokenUser, std::ptr::null_mut(), 0, &mut required);
        }
        if required < size_of::<TOKEN_USER>() as u32 {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "current Windows user identity is unavailable",
            ));
        }
        let byte_len = required as usize;
        let word_len = byte_len.div_ceil(size_of::<usize>());
        // TOKEN_USER contains pointers and must be read from suitably aligned storage.
        let mut buffer = vec![0_usize; word_len];
        if unsafe {
            GetTokenInformation(
                token,
                TokenUser,
                buffer.as_mut_ptr().cast(),
                required,
                &mut required,
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        let user = unsafe { &*(buffer.as_ptr().cast::<TOKEN_USER>()) };
        if user.User.Sid.is_null() || unsafe { IsValidSid(user.User.Sid) } == 0 {
            return Err(untrusted_acl());
        }
        let length = unsafe { GetLengthSid(user.User.Sid) } as usize;
        if length == 0 {
            return Err(untrusted_acl());
        }
        Ok(unsafe { std::slice::from_raw_parts(user.User.Sid.cast::<u8>(), length) }.to_vec())
    })();
    unsafe { CloseHandle(token) };
    result
}

fn untrusted_acl() -> io::Error {
    io::Error::new(
        io::ErrorKind::PermissionDenied,
        "Docker named-pipe access control is not trusted",
    )
}

#[cfg(test)]
mod tests;
