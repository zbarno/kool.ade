use std::borrow::Cow;

pub(crate) fn scoped_capture_failure_reason(error: &anyhow::Error) -> Cow<'static, str> {
    let reason = error.to_string();
    if reason.contains("hard link or special file") {
        Cow::Borrowed("it contains a hard link")
    } else if reason.contains("special file") {
        Cow::Borrowed("it contains a special filesystem entry")
    } else if reason.contains("symlink") {
        Cow::Borrowed("it contains a symbolic link")
    } else if reason.contains("changed while staging") {
        Cow::Borrowed("the workspace changed while being captured")
    } else if reason.contains("changed or is unsafe") {
        Cow::Borrowed("a filesystem entry changed or could not be opened safely during capture")
    } else if reason.contains("PermissionDenied") {
        Cow::Borrowed("a filesystem entry is not accessible to the controller")
    } else if reason.contains("NotFound") {
        Cow::Borrowed("a filesystem entry disappeared while being captured")
    } else if reason.contains("too many entries") {
        Cow::Borrowed("the workspace entry limit was exceeded")
    } else if reason.contains("exceeds staging budget")
        || reason.contains("exceeds its staging budget")
    {
        Cow::Borrowed("the workspace exceeds its byte limit")
    } else if let Some(error) = error
        .chain()
        .find_map(|cause| cause.downcast_ref::<std::io::Error>())
    {
        let reason = match error.raw_os_error() {
            #[cfg(unix)]
            Some(code) if code == libc::EACCES || code == libc::EPERM => {
                "a filesystem entry is not accessible to the controller"
            }
            #[cfg(unix)]
            Some(code) if code == libc::EMFILE => {
                "the process file descriptor limit was reached during capture"
            }
            #[cfg(unix)]
            Some(code) if code == libc::ENFILE => {
                "the system file descriptor limit was reached during capture"
            }
            #[cfg(unix)]
            Some(code) if code == libc::EIO => {
                "the filesystem reported an input/output error during capture"
            }
            _ => {
                if let Some(code) = error.raw_os_error() {
                    return Cow::Owned(format!(
                        "an operating system filesystem operation failed (OS error {code})"
                    ));
                }
                match error.kind() {
                    std::io::ErrorKind::PermissionDenied => {
                        "a filesystem entry is not accessible to the controller"
                    }
                    std::io::ErrorKind::NotFound => {
                        "a filesystem entry disappeared while being captured"
                    }
                    std::io::ErrorKind::NotADirectory => {
                        "a filesystem entry changed type during capture"
                    }
                    std::io::ErrorKind::WouldBlock => "a filesystem entry was busy during capture",
                    _ => "an operating system filesystem operation failed",
                }
            }
        };
        Cow::Borrowed(reason)
    } else {
        Cow::Borrowed("a filesystem safety check failed")
    }
}
