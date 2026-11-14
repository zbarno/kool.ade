use std::{io, ptr::null_mut};

use windows_sys::Win32::{
    Foundation::{CloseHandle, HANDLE},
    Security::{
        CopySid, EqualSid, GetLengthSid, GetTokenInformation, TOKEN_QUERY, TOKEN_USER, TokenUser,
    },
    System::Threading::{GetCurrentProcess, OpenProcessToken},
};

pub(super) fn verify_current_user_owner(process: HANDLE) -> io::Result<()> {
    let mut current_token = null_mut();
    let mut child_token = null_mut();
    // SAFETY: token handles are out-pointers; GetCurrentProcess is a valid pseudo-handle.
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut current_token) } == 0 {
        return Err(io::Error::last_os_error());
    }
    if unsafe { OpenProcessToken(process, TOKEN_QUERY, &mut child_token) } == 0 {
        unsafe { CloseHandle(current_token) };
        return Err(io::Error::last_os_error());
    }
    let current = token_user_sid(current_token);
    let child = token_user_sid(child_token);
    unsafe {
        CloseHandle(current_token);
        CloseHandle(child_token);
    }
    let (current, child) = (current?, child?);
    // SAFETY: each owned vector contains a copied SID structure.
    if unsafe {
        EqualSid(
            current.as_ptr().cast_mut().cast(),
            child.as_ptr().cast_mut().cast(),
        )
    } != 0
    {
        Ok(())
    } else {
        Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "QEMU process token does not match the current user",
        ))
    }
}

fn token_user_sid(token: HANDLE) -> io::Result<Vec<u8>> {
    let mut required = 0_u32;
    // SAFETY: the first call requests the required size and provides no buffer.
    unsafe { GetTokenInformation(token, TokenUser, null_mut(), 0, &mut required) };
    if required == 0 || required > 64 * 1024 {
        return Err(io::Error::last_os_error());
    }
    let mut info = vec![0_u8; required as usize];
    // SAFETY: `info` is a writable buffer of the required size.
    if unsafe {
        GetTokenInformation(
            token,
            TokenUser,
            info.as_mut_ptr().cast(),
            required,
            &mut required,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: GetTokenInformation populated a TOKEN_USER at the start of `info`.
    let user = unsafe { &*info.as_ptr().cast::<TOKEN_USER>() };
    let sid_length = unsafe { GetLengthSid(user.User.Sid) };
    if sid_length == 0 || sid_length as usize > info.len() {
        return Err(io::Error::last_os_error());
    }
    let mut sid = vec![0_u8; sid_length as usize];
    // SAFETY: source SID is inside `info`; destination has the exact reported size.
    if unsafe { CopySid(sid_length, sid.as_mut_ptr().cast(), user.User.Sid) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(sid)
}
