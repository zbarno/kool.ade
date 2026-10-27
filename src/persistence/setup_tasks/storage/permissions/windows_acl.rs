mod acl;

use std::{mem::size_of, os::windows::ffi::OsStrExt, path::Path, ptr::null_mut};

use windows_sys::Win32::{
    Foundation::{CloseHandle, HANDLE, HLOCAL, LocalFree},
    Security::Authorization::{
        ConvertStringSidToSidW, GetNamedSecurityInfoW, GetSecurityInfo, SE_FILE_OBJECT,
    },
    Security::{
        DACL_SECURITY_INFORMATION, EqualSid, GetTokenInformation, IsWellKnownSid,
        OWNER_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR, PSID, TOKEN_QUERY, TOKEN_USER, TokenUser,
        WinBuiltinAdministratorsSid, WinLocalSystemSid,
    },
    System::Threading::{GetCurrentProcess, OpenProcessToken},
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
    let mut dacl = null_mut();
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
        !descriptor.is_null() && !owner.is_null() && !dacl.is_null(),
        "machine setup state ACL is missing its owner or DACL"
    );

    let current_user_storage = current_user_sid_storage()?;
    let current_user = unsafe { &*(current_user_storage.as_ptr() as *const TOKEN_USER) }
        .User
        .Sid;
    acl::verify_private_acl(descriptor, owner, dacl, current_user)
}

pub(super) fn verify_private_acl_handle(handle: HANDLE) -> anyhow::Result<()> {
    let mut owner: PSID = null_mut();
    let mut dacl = null_mut();
    let mut descriptor: PSECURITY_DESCRIPTOR = null_mut();
    let status = unsafe {
        GetSecurityInfo(
            handle,
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
        "could not inspect machine setup handle ACL (Windows error {status})"
    );
    anyhow::ensure!(
        !descriptor.is_null() && !owner.is_null() && !dacl.is_null(),
        "machine setup handle ACL is missing its owner or DACL"
    );
    let current_user_storage = current_user_sid_storage()?;
    let current_user = unsafe { &*(current_user_storage.as_ptr() as *const TOKEN_USER) }
        .User
        .Sid;
    acl::verify_private_acl(descriptor, owner, dacl, current_user)
}

pub(super) fn verify_no_untrusted_delete_rights(handle: HANDLE) -> anyhow::Result<()> {
    let mut owner: PSID = null_mut();
    let mut dacl = null_mut();
    let mut descriptor: PSECURITY_DESCRIPTOR = null_mut();
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
    let _descriptor = Descriptor(descriptor);
    anyhow::ensure!(
        status == 0 && !descriptor.is_null() && !owner.is_null() && !dacl.is_null(),
        "could not inspect machine setup ancestor ACL (Windows error {status})"
    );
    let current_user_storage = current_user_sid_storage()?;
    let current_user = unsafe { &*(current_user_storage.as_ptr() as *const TOKEN_USER) }
        .User
        .Sid;
    anyhow::ensure!(
        is_trusted_ancestor_owner(owner, current_user)?,
        "machine setup ancestor has an untrusted owner"
    );
    acl::verify_no_untrusted_delete_rights(descriptor, dacl, current_user)
}

fn is_trusted_ancestor_owner(owner: PSID, current_user: PSID) -> anyhow::Result<bool> {
    if unsafe { EqualSid(owner, current_user) } != 0
        || unsafe { IsWellKnownSid(owner, WinLocalSystemSid) } != 0
        || unsafe { IsWellKnownSid(owner, WinBuiltinAdministratorsSid) } != 0
    {
        return Ok(true);
    }
    const TRUSTED_INSTALLER_SID: &str =
        "S-1-5-80-956008885-3418522649-1831038044-1853292631-2271478464";
    let mut sid_text = TRUSTED_INSTALLER_SID.encode_utf16().collect::<Vec<_>>();
    sid_text.push(0);
    let mut trusted_installer: PSID = null_mut();
    if unsafe { ConvertStringSidToSidW(sid_text.as_ptr(), &mut trusted_installer) } == 0 {
        anyhow::bail!("could not validate the trusted Windows ancestor owner")
    }
    let trusted = unsafe { EqualSid(owner, trusted_installer) } != 0;
    unsafe { LocalFree(trusted_installer as HLOCAL) };
    Ok(trusted)
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
