use anyhow::anyhow;

use crate::execution::docker::sandbox::staging::scoped_capture_failure_reason;

#[test]
fn scoped_capture_failure_categories_are_safe_and_specific() {
    assert_eq!(
        scoped_capture_failure_reason(&anyhow!("workspace contains a hard link or special file")),
        "it contains a hard link"
    );
    assert_eq!(
        scoped_capture_failure_reason(&anyhow!("workspace contains a special file")),
        "it contains a special filesystem entry"
    );
    assert_eq!(
        scoped_capture_failure_reason(&anyhow!("workspace file changed while staging")),
        "the workspace changed while being captured"
    );
    assert_eq!(
        scoped_capture_failure_reason(&anyhow!("workspace file changed or is unsafe")),
        "a filesystem entry changed or could not be opened safely during capture"
    );
    assert_eq!(
        scoped_capture_failure_reason(
            &std::io::Error::from(std::io::ErrorKind::PermissionDenied).into()
        ),
        "the operating system denied access to a filesystem entry"
    );
    assert_eq!(
        scoped_capture_failure_reason(&std::io::Error::from(std::io::ErrorKind::NotFound).into()),
        "a filesystem entry disappeared during capture"
    );
    assert_eq!(
        scoped_capture_failure_reason(&anyhow!("host path /secret/customer/data was rejected")),
        "a filesystem safety check failed"
    );
}
