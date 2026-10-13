#[cfg(unix)]
pub(super) fn terminate_group(pid: u32) {
    unsafe extern "C" {
        fn kill(pid: i32, signal: i32) -> i32;
    }
    if let Ok(pid) = i32::try_from(pid) {
        unsafe {
            kill(-pid, 9);
        }
    }
}

#[cfg(not(unix))]
pub(super) fn terminate_group(_pid: u32) {}
