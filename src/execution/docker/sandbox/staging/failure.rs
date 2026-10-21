//! Sanitized reasons for scoped workspace capture failures.

use std::borrow::Cow;

pub(crate) fn scoped_capture_failure_reason(error: &anyhow::Error) -> Cow<'static, str> {
    let reason = error.to_string();
    let io_kind = error.chain().find_map(|cause| {
        cause
            .downcast_ref::<std::io::Error>()
            .map(std::io::Error::kind)
    });
    if reason.contains("hard link or special file") {
        Cow::Borrowed("it contains a hard link")
    } else if reason.contains("special file") {
        Cow::Borrowed("it contains a special filesystem entry")
    } else if reason.contains("symlink") {
        Cow::Borrowed("it contains a symbolic link")
    } else if reason.contains("changed while staging") {
        Cow::Borrowed("the workspace changed while being captured")
    } else if reason.contains("too many entries") {
        Cow::Borrowed("the workspace entry limit was exceeded")
    } else if reason.contains("PermissionDenied")
        || io_kind == Some(std::io::ErrorKind::PermissionDenied)
    {
        Cow::Borrowed("a filesystem entry is not accessible to the controller")
    } else if reason.contains("NotFound") || io_kind == Some(std::io::ErrorKind::NotFound) {
        Cow::Borrowed("a filesystem entry disappeared while being captured")
    } else if reason.contains("workspace directory could not be duplicated") {
        Cow::Borrowed("a workspace directory could not be inspected")
    } else if reason.contains("workspace directory could not be read") {
        Cow::Borrowed("a workspace directory could not be read")
    } else if reason.contains("workspace root contains a symlink") {
        Cow::Borrowed("a workspace path could not be opened safely")
    } else if let Some(reason) = formatted_io_failure_reason(&reason).or_else(|| {
        io_kind
            .map(|kind| format!("{kind:?}"))
            .and_then(|kind| formatted_io_failure_reason(&kind))
    }) {
        Cow::Borrowed(reason)
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
                    kind => {
                        return Cow::Owned(format!(
                            "an operating system filesystem operation failed (error kind {kind:?})"
                        ));
                    }
                }
            }
        };
        Cow::Borrowed(reason)
    } else {
        Cow::Borrowed("a filesystem safety check failed")
    }
}

fn formatted_io_failure_reason(reason: &str) -> Option<&'static str> {
    if reason.ends_with("FilesystemLoop") {
        Some("a symbolic link or loop was encountered")
    } else if reason.ends_with("NotADirectory") {
        Some("a workspace path component is not a directory")
    } else if reason.ends_with("InvalidInput") || reason.ends_with("InvalidData") {
        Some("a workspace entry could not be inspected")
    } else if reason.ends_with("UnexpectedEof") {
        Some("a workspace file changed while being read")
    } else {
        None
    }
}
