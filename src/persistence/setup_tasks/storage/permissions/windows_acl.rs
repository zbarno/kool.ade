use std::{
    mem::{size_of, zeroed},
    os::windows::ffi::OsStrExt,
    path::Path,
    ptr::null_mut,
};

use windows_sys::Win32::{
    Foundation::{CloseHandle, HANDLE, HLOCAL, LocalFree},
    Security::Authorization::{GetNamedSecurityInfoW, SE_FILE_OBJECT},
    Security::{
        ACCESS_ALLOWED_ACE, ACE_HEADER, ACL, ACL_SIZE_INFORMATION, AclSizeInformation,
        DACL_SECURITY_INFORMATION, GetAce, GetAclInformation, GetTokenInformation, IsWellKnownSid,
        OWNER_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR, PSID, TOKEN_QUERY, TOKEN_USER, TokenUser,
        WinBuiltinAdministratorsSid, WinCreatorOwnerSid, WinLocalSystemSid,
    },
    System::{
        SystemServices::{
            ACCESS_ALLOWED_ACE_TYPE, ACCESS_ALLOWED_CALLBACK_ACE_TYPE,
            ACCESS_ALLOWED_CALLBACK_OBJECT_ACE_TYPE, ACCESS_ALLOWED_COMPOUND_ACE_TYPE,
            ACCESS_ALLOWED_OBJECT_ACE_TYPE, SYSTEM_ACCESS_FILTER_ACE_TYPE,
        },
        Threading::{GetCurrentProcess, OpenProcessToken},
    },
};

struct Token(HANDLE);

impl Drop for Token {
    fn drop(&mut self) {
        unsafe { CloseHandle(self.0) };
    }
}

struct Descriptor(PSECURITY_DESCRIPTOR);

impl Drop for Descriptor {
    fn drop(&mut self) {
        unsafe { LocalFree(self.0 as HLOCAL) };
    }
}

pub(super) fn verify_private_acl(path: &Path) -> anyhow::Result<()> {
    let mut wide_path = path.as_os_str().encode_wide().collect::<Vec<_>>();
    anyhow::ensure!(
        !wide_path.contains(&0),
        "machine setup state path contains a null character"
    );
    wide_path.push(0);
    let mut owner: PSID = null_mut();
    let mut dacl: *mut ACL = null_mut();
    let mut descriptor: PSECURITY_DESCRIPTOR = null_mut();
    let status = unsafe {
        GetNamedSecurityInfoW(
            wide_path.as_ptr(),
            SE_FILE_OBJECT,
            OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
            &mut owner,
            null_mut(),
            &mut dacl,
            null_mut(),
            &mut descriptor,
        )
    };
    let _descriptor = Descriptor(descriptor);
    anyhow::ensure!(
        status == 0,
        "could not inspect machine setup state ACL (Windows error {status})"
    );
    anyhow::ensure!(
        !owner.is_null() && !dacl.is_null(),
        "machine setup state ACL is missing its owner or DACL"
    );

    let current_user_storage = current_user_sid_storage()?;
    let current_user = unsafe { &*(current_user_storage.as_ptr() as *const TOKEN_USER) }
        .User
        .Sid;
    anyhow::ensure!(
        unsafe { windows_sys::Win32::Security::EqualSid(owner, current_user) } != 0,
        "machine setup state is not owned by the current Windows user"
    );

    let mut info: ACL_SIZE_INFORMATION = unsafe { zeroed() };
    let acl_info_ok = unsafe {
        GetAclInformation(
            dacl,
            &mut info as *mut _ as *mut core::ffi::c_void,
            size_of::<ACL_SIZE_INFORMATION>() as u32,
            AclSizeInformation,
        )
    };
    anyhow::ensure!(
        acl_info_ok != 0,
        "could not inspect machine setup state DACL"
    );
    // The returned DACL includes inherited ACEs. Check them alongside explicit
    // entries so a permissive parent cannot make this path public.
    for index in 0..info.AceCount {
        let mut raw_ace: *mut core::ffi::c_void = null_mut();
        anyhow::ensure!(
            unsafe { GetAce(dacl, index, &mut raw_ace) } != 0 && !raw_ace.is_null(),
            "could not read a machine setup state ACL entry"
        );
        let header = unsafe { *(raw_ace as *const ACE_HEADER) };
        let ace_type = u32::from(header.AceType);
        if ace_type != ACCESS_ALLOWED_ACE_TYPE {
            anyhow::ensure!(
                !matches!(
                    ace_type,
                    ACCESS_ALLOWED_COMPOUND_ACE_TYPE
                        | ACCESS_ALLOWED_OBJECT_ACE_TYPE
                        | ACCESS_ALLOWED_CALLBACK_ACE_TYPE
                        | ACCESS_ALLOWED_CALLBACK_OBJECT_ACE_TYPE
                ),
                "machine setup state DACL contains an unsupported allow ACE"
            );
            anyhow::ensure!(
                ace_type <= SYSTEM_ACCESS_FILTER_ACE_TYPE,
                "machine setup state DACL contains an unknown ACE type"
            );
            continue;
        }
        anyhow::ensure!(
            usize::from(header.AceSize) >= size_of::<ACCESS_ALLOWED_ACE>(),
            "machine setup state DACL contains a truncated allow ACE"
        );
        let ace = unsafe { &*(raw_ace as *const ACCESS_ALLOWED_ACE) };
        let ace_sid = &ace.SidStart as *const u32 as PSID;
        let is_current_user =
            unsafe { windows_sys::Win32::Security::EqualSid(ace_sid, current_user) } != 0;
        let is_system_or_admin = unsafe {
            IsWellKnownSid(ace_sid, WinLocalSystemSid) != 0
                || IsWellKnownSid(ace_sid, WinBuiltinAdministratorsSid) != 0
        };
        let is_creator_owner_inherit_only =
            u32::from(header.AceFlags) & windows_sys::Win32::Security::INHERIT_ONLY_ACE != 0
                && unsafe { IsWellKnownSid(ace_sid, WinCreatorOwnerSid) } != 0;
        anyhow::ensure!(
            is_current_user || is_system_or_admin || is_creator_owner_inherit_only,
            "machine setup state DACL grants access to another Windows principal"
        );
    }
    Ok(())
}

fn current_user_sid_storage() -> anyhow::Result<Vec<usize>> {
    let mut token_handle = null_mut();
    anyhow::ensure!(
        unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token_handle) } != 0,
        "could not read the current Windows user token"
    );
    let _token = Token(token_handle);

    let mut size = 0;
    unsafe {
        GetTokenInformation(token_handle, TokenUser, null_mut(), 0, &mut size);
    }
    anyhow::ensure!(
        size >= size_of::<TOKEN_USER>() as u32,
        "current Windows user token is invalid"
    );
    let words = (size as usize).div_ceil(size_of::<usize>());
    let mut buffer = vec![0usize; words];
    anyhow::ensure!(
        unsafe {
            GetTokenInformation(
                token_handle,
                TokenUser,
                buffer.as_mut_ptr() as *mut core::ffi::c_void,
                size,
                &mut size,
            )
        } != 0,
        "could not read the current Windows user SID"
    );
    let user = unsafe { &*(buffer.as_ptr() as *const TOKEN_USER) };
    anyhow::ensure!(
        !user.User.Sid.is_null(),
        "current Windows user SID is missing"
    );
    Ok(buffer)
}
