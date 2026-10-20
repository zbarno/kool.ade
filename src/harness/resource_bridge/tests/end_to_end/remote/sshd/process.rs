use std::{
    fs,
    os::fd::{AsRawFd, FromRawFd},
    path::Path,
};

pub(super) fn terminate_worker_pid(
    pid: libc::pid_t,
    worker_binary: &Path,
    worker_marker: &str,
) -> std::io::Result<bool> {
    let pidfd = unsafe { libc::syscall(libc::SYS_pidfd_open, pid, 0) };
    if pidfd < 0 {
        let error = std::io::Error::last_os_error();
        return if error.raw_os_error() == Some(libc::ESRCH) {
            Ok(false)
        } else {
            Err(error)
        };
    }
    let pidfd = unsafe { fs::File::from_raw_fd(pidfd as std::os::fd::RawFd) };
    let command_line = match fs::read(format!("/proc/{pid}/cmdline")) {
        Ok(command_line) => command_line,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error),
    };
    let arguments = command_line
        .split(|byte| *byte == 0)
        .filter(|argument| !argument.is_empty())
        .collect::<Vec<_>>();
    if arguments.len() != 4
        || arguments[0] != worker_binary.as_os_str().as_encoded_bytes()
        || arguments[1] != b"worker"
        || arguments[2] != b"serve"
        || arguments[3] != b"--stdio"
    {
        return Ok(false);
    }
    let environment = match fs::read(format!("/proc/{pid}/environ")) {
        Ok(environment) => environment,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error),
    };
    let marker = format!("KOOLADE_TEST_FIXTURE_WORKER_ID={worker_marker}");
    if !environment
        .split(|byte| *byte == 0)
        .any(|value| value == marker.as_bytes())
    {
        return Ok(false);
    }
    let result = unsafe {
        libc::syscall(
            libc::SYS_pidfd_send_signal,
            pidfd.as_raw_fd(),
            libc::SIGKILL,
            std::ptr::null::<libc::siginfo_t>(),
            0,
        )
    };
    if result == 0 {
        Ok(true)
    } else {
        let error = std::io::Error::last_os_error();
        if error.raw_os_error() == Some(libc::ESRCH) {
            Ok(false)
        } else {
            Err(error)
        }
    }
}
