use std::io;
use std::mem::size_of;

use windows_sys::Win32::Foundation::{CloseHandle, GENERIC_ALL, GENERIC_WRITE, HANDLE, LocalFree};
use windows_sys::Win32::Security::Authorization::{GetSecurityInfo, SE_FILE_OBJECT};
use windows_sys::Win32::Security::{
    ACCESS_ALLOWED_ACE, ACE_HEADER, DACL_SECURITY_INFORMATION, EqualSid, GetLengthSid,
    GetTokenInformation, IsValidAcl, IsValidSid, IsWellKnownSid, OWNER_SECURITY_INFORMATION, PSID,
    TOKEN_QUERY, TOKEN_USER, TokenUser, WinAnonymousSid, WinAuthenticatedUserSid,
    WinBuiltinAdministratorsSid, WinBuiltinUsersSid, WinLocalSystemSid, WinWorldSid,
};
use windows_sys::Win32::Storage::FileSystem::{
    DELETE, FILE_APPEND_DATA, FILE_GENERIC_READ, FILE_GENERIC_WRITE, FILE_WRITE_ATTRIBUTES,
    FILE_WRITE_DATA, FILE_WRITE_EA, WRITE_DAC, WRITE_OWNER,
};
use windows_sys::Win32::System::SystemServices::ACCESS_ALLOWED_ACE_TYPE;
use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

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

    let mut entries = Vec::new();
    let ace_count = unsafe { (*dacl).AceCount };
    for index in 0..u32::from(ace_count) {
        let mut ace_ptr = std::ptr::null_mut();
        if unsafe { windows_sys::Win32::Security::GetAce(dacl, index, &mut ace_ptr) } == 0
            || ace_ptr.is_null()
        {
            return Err(untrusted_acl());
        }
        let header = unsafe { &*(ace_ptr as *const ACE_HEADER) };
        if header.AceType != ACCESS_ALLOWED_ACE_TYPE as u8
            || usize::from(header.AceSize) < size_of::<ACCESS_ALLOWED_ACE>()
        {
            return Err(untrusted_acl());
        }
        let ace = unsafe { &*(ace_ptr as *const ACCESS_ALLOWED_ACE) };
        let sid = std::ptr::addr_of!(ace.SidStart) as PSID;
        let sid_offset = size_of::<ACE_HEADER>() + size_of::<u32>();
        let ace_size = usize::from(header.AceSize);
        // SID's fixed header is eight bytes: revision, subauthority count, and identifier
        // authority. Bound that header before reading its count, then bound the full SID before
        // calling Windows SID helpers that inspect it.
        const SID_HEADER_SIZE: usize = 8;
        if ace_size < sid_offset + SID_HEADER_SIZE {
            return Err(untrusted_acl());
        }
        let subauthority_count = unsafe { *sid.cast::<u8>().add(1) } as usize;
        let subauthority_bytes = subauthority_count
            .checked_mul(size_of::<u32>())
            .ok_or_else(untrusted_acl)?;
        let sid_len = SID_HEADER_SIZE
            .checked_add(subauthority_bytes)
            .ok_or_else(untrusted_acl)?;
        if sid_len > ace_size - sid_offset
            || unsafe { IsValidSid(sid) } == 0
            || unsafe { GetLengthSid(sid) } as usize != sid_len
        {
            return Err(untrusted_acl());
        }
        let trustee = classify_trustee(sid, current_user);
        entries.push(AclEntry {
            trustee,
            mask: ace.Mask,
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Trustee {
    CurrentUser,
    LocalSystem,
    Administrators,
    BroadReadOnly,
    Other,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct AclEntry {
    trustee: Trustee,
    mask: u32,
}

fn classify_trustee(sid: PSID, current_user: &[u8]) -> Trustee {
    unsafe {
        if EqualSid(sid, current_user.as_ptr() as PSID) != 0 {
            Trustee::CurrentUser
        } else if IsWellKnownSid(sid, WinLocalSystemSid) != 0 {
            Trustee::LocalSystem
        } else if IsWellKnownSid(sid, WinBuiltinAdministratorsSid) != 0 {
            Trustee::Administrators
        } else if IsWellKnownSid(sid, WinWorldSid) != 0
            || IsWellKnownSid(sid, WinAnonymousSid) != 0
            || IsWellKnownSid(sid, WinAuthenticatedUserSid) != 0
            || IsWellKnownSid(sid, WinBuiltinUsersSid) != 0
        {
            Trustee::BroadReadOnly
        } else {
            Trustee::Other
        }
    }
}

fn trusted_pipe_acl(
    dacl_present: bool,
    owner_is_user: bool,
    owner_is_system_or_admin: bool,
    entries: &[AclEntry],
) -> bool {
    if !dacl_present || !(owner_is_user || owner_is_system_or_admin) || entries.is_empty() {
        return false;
    }

    let mut caller_has_read_write = false;
    for entry in entries {
        match entry.trustee {
            Trustee::CurrentUser => {
                caller_has_read_write |= entry.mask & FILE_GENERIC_READ == FILE_GENERIC_READ
                    && entry.mask & FILE_GENERIC_WRITE == FILE_GENERIC_WRITE;
            }
            Trustee::LocalSystem | Trustee::Administrators => {}
            Trustee::BroadReadOnly => {
                if entry.mask
                    & (FILE_WRITE_DATA
                        | FILE_APPEND_DATA
                        | FILE_WRITE_EA
                        | FILE_WRITE_ATTRIBUTES
                        | WRITE_DAC
                        | WRITE_OWNER
                        | DELETE
                        | GENERIC_WRITE
                        | GENERIC_ALL)
                    != 0
                {
                    return false;
                }
            }
            Trustee::Other => return false,
        }
    }
    caller_has_read_write
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
