pub(crate) fn scoped_capture_failure_reason(error: &anyhow::Error) -> &'static str {
    let reason = error.to_string();
    if reason.contains("hard link or special file") {
        "it contains a hard link"
    } else if reason.contains("special file") {
        "it contains a special filesystem entry"
    } else if reason.contains("symlink") {
        "it contains a symbolic link"
    } else if reason.contains("changed while staging") {
        "the workspace changed while being captured"
    } else if reason.contains("changed or is unsafe") {
        "a filesystem entry changed or could not be opened safely during capture"
    } else if reason.contains("too many entries") {
        "the workspace entry limit was exceeded"
    } else if reason.contains("exceeds staging budget")
        || reason.contains("exceeds its staging budget")
    {
        "the workspace exceeds its byte limit"
    } else if let Some(error) = error.downcast_ref::<std::io::Error>() {
        match error.kind() {
            std::io::ErrorKind::PermissionDenied => {
                "the operating system denied access to a filesystem entry"
            }
            std::io::ErrorKind::NotFound => "a filesystem entry disappeared during capture",
            std::io::ErrorKind::NotADirectory => "a filesystem entry changed type during capture",
            std::io::ErrorKind::WouldBlock => "a filesystem entry was busy during capture",
            _ => "an operating system filesystem operation failed",
        }
    } else {
        "a filesystem safety check failed"
    }
}
