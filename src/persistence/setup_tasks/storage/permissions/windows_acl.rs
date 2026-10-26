mod acl;

use std::{mem::size_of, os::windows::ffi::OsStrExt, path::Path, ptr::null_mut};

use windows_sys::Win32::{
    Foundation::{CloseHandle, HANDLE, HLOCAL, LocalFree},
    Security::Authorization::{GetNamedSecurityInfoW, SE_FILE_OBJECT},
    Security::{
        DACL_SECURITY_INFORMATION, GetTokenInformation, OWNER_SECURITY_INFORMATION,
        PSECURITY_DESCRIPTOR, PSID, TOKEN_QUERY, TOKEN_USER, TokenUser,
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
