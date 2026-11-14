use std::{io, ptr::null_mut};

use windows_sys::Win32::{
    Foundation::{CloseHandle, HANDLE},
    Security::{
        CopySid, EqualSid, GetLengthSid, GetTokenInformation, TOKEN_QUERY, TOKEN_USER, TokenUser,
    },
    System::Threading::{GetCurrentProcess, OpenProcessToken},
};

pub(crate) struct AlignedSid {
    words: Vec<usize>,
    byte_len: usize,
}

impl AlignedSid {
    fn zeroed(byte_len: usize) -> Self {
        Self {
            words: vec![0; byte_len.div_ceil(std::mem::size_of::<usize>())],
            byte_len,
        }
    }

    fn as_ptr(&self) -> *const u8 {
        self.words.as_ptr().cast()
    }

    fn as_mut_ptr(&mut self) -> *mut u8 {
        self.words.as_mut_ptr().cast()
    }

    fn as_bytes(&self) -> &[u8] {
        // SAFETY: the backing words contain at least byte_len initialized bytes.
        unsafe { std::slice::from_raw_parts(self.as_ptr(), self.byte_len) }
    }
}

impl std::ops::Deref for AlignedSid {
    type Target = [u8];

    fn deref(&self) -> &Self::Target {
        self.as_bytes()
    }
}

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

pub(super) fn token_user_sid(token: HANDLE) -> io::Result<AlignedSid> {
    let mut required = 0_u32;
    // SAFETY: the first call requests the required size and provides no buffer.
    unsafe { GetTokenInformation(token, TokenUser, null_mut(), 0, &mut required) };
    if required == 0 || required > 64 * 1024 {
        return Err(io::Error::last_os_error());
    }
    let word_count = (required as usize).div_ceil(std::mem::size_of::<usize>());
    let mut info = vec![0_usize; word_count];
    let info_size = info.len() * std::mem::size_of::<usize>();
    // SAFETY: the aligned `usize` allocation is writable and has at least
    // `required` bytes, as requested from GetTokenInformation.
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
    if sid_length == 0 || sid_length as usize > info_size {
        return Err(io::Error::last_os_error());
    }
    let mut sid = AlignedSid::zeroed(sid_length as usize);
    // SAFETY: source SID is inside aligned TOKEN_USER storage; the destination
    // is an aligned writable allocation with the exact reported size.
    if unsafe { CopySid(sid_length, sid.as_mut_ptr().cast(), user.User.Sid) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(sid)
}
