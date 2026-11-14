//! Checked copies from Windows ACL buffers returned by Win32.

use std::{
    ffi::c_void,
    mem::{MaybeUninit, size_of},
};

use windows_sys::Win32::{
    Foundation::HANDLE,
    System::{Diagnostics::Debug::ReadProcessMemory, Threading::GetCurrentProcess},
};

/// Copy an ACL value from the current process using Win32's checked memory API.
pub(super) fn read_current_process_value<T: Copy>(address: *const c_void) -> Option<T> {
    let mut value = MaybeUninit::<T>::uninit();
    let mut bytes_read = 0;
    // SAFETY: the process handle is the current process pseudo-handle; output
    // points to uninitialized storage of exactly the requested size.
    let copied = unsafe {
        ReadProcessMemory(
            current_process(),
            address,
            value.as_mut_ptr().cast(),
            size_of::<T>(),
            &mut bytes_read,
        )
    };
    if copied == 0 || bytes_read != size_of::<T>() {
        return None;
    }
    // SAFETY: ReadProcessMemory reported a full copy into `value`.
    Some(unsafe { value.assume_init() })
}

/// Copy a bounded byte prefix from an ACL address without dereferencing it.
pub(super) fn read_current_process_bytes<const N: usize>(
    address: *const c_void,
) -> Option<[u8; N]> {
    let mut bytes = [0; N];
    let mut bytes_read = 0;
    // SAFETY: the process handle is the current process pseudo-handle; output
    // points to a writable fixed-size array.
    let copied = unsafe {
        ReadProcessMemory(
            current_process(),
            address,
            bytes.as_mut_ptr().cast(),
            N,
            &mut bytes_read,
        )
    };
    (copied != 0 && bytes_read == N).then_some(bytes)
}

fn current_process() -> HANDLE {
    // SAFETY: GetCurrentProcess returns a pseudo-handle valid for this process.
    unsafe { GetCurrentProcess() }
}
