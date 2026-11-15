#![cfg(windows)]
#![allow(dead_code)]

mod computer_use {
    pub(crate) mod windows_backend {
        #[path = "../../../../../src/computer_use/windows_backend/storage_policy.rs"]
        mod storage_policy;

        mod process {
            use std::io;

            #[path = "../../../../../../src/computer_use/windows_backend/process/identity.rs"]
            mod identity;

            pub(super) fn current_user_sid() -> io::Result<identity::AlignedSid> {
                use windows_sys::Win32::{
                    Foundation::CloseHandle,
                    Security::TOKEN_QUERY,
                    System::Threading::{GetCurrentProcess, OpenProcessToken},
                };
                let mut token = std::ptr::null_mut();
                // SAFETY: token is a writable output handle and the process pseudo-handle is valid.
                if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
                    return Err(io::Error::last_os_error());
                }
                let result = identity::token_user_sid(token);
                // SAFETY: token was returned as an owned handle by OpenProcessToken.
                unsafe { CloseHandle(token) };
                result
            }
        }

        #[path = "../../../../../src/computer_use/windows_backend/storage.rs"]
        mod storage;
    }
}
