use std::{process::Child, time::Duration};

#[cfg(unix)]
pub(super) fn cleanup_descendants(pid: u32) {
    signal_group(pid, libc::SIGTERM);
    std::thread::sleep(Duration::from_millis(100));
    signal_group(pid, libc::SIGKILL);
}

#[cfg(not(unix))]
pub(super) fn cleanup_descendants(_pid: u32) {}

pub(super) fn terminate(pid: u32, child: &mut Child) {
    #[cfg(unix)]
    {
        signal_group(pid, libc::SIGTERM);
        std::thread::sleep(Duration::from_millis(150));
        signal_group(pid, libc::SIGKILL);
    }
    let _ = child.kill();
    let _ = child.wait();
    #[cfg(windows)]
    let _ = pid;
}

#[cfg(unix)]
fn signal_group(pid: u32, signal: i32) {
    if let Ok(pid) = i32::try_from(pid) {
        unsafe {
            libc::kill(-pid, signal);
        }
    }
}
