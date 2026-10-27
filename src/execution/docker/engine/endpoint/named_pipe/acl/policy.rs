use windows_sys::Win32::Foundation::{GENERIC_ALL, GENERIC_WRITE};
use windows_sys::Win32::Security::{
    EqualSid, IsWellKnownSid, PSID, WinAnonymousSid, WinAuthenticatedUserSid,
    WinBuiltinAdministratorsSid, WinBuiltinUsersSid, WinLocalSystemSid, WinWorldSid,
};
use windows_sys::Win32::Storage::FileSystem::{
    DELETE, FILE_APPEND_DATA, FILE_GENERIC_READ, FILE_GENERIC_WRITE, FILE_WRITE_ATTRIBUTES,
    FILE_WRITE_DATA, FILE_WRITE_EA, WRITE_DAC, WRITE_OWNER,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Trustee {
    CurrentUser,
    LocalSystem,
    Administrators,
    BroadReadOnly,
    Other,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct AclEntry {
    pub(super) trustee: Trustee,
    pub(super) mask: u32,
}

pub(super) fn classify_trustee(sid: PSID, current_user: &[u8]) -> Trustee {
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

pub(super) fn trusted_pipe_acl(
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
