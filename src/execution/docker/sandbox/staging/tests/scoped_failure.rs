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
        scoped_capture_failure_reason(&anyhow!(
            "workspace file could not be opened safely: PermissionDenied"
        )),
        "a filesystem entry is not accessible to the controller"
    );
    assert_eq!(
        scoped_capture_failure_reason(&anyhow!(
            "workspace directory could not be opened safely: NotFound"
        )),
        "a filesystem entry disappeared while being captured"
    );
    assert_eq!(
        scoped_capture_failure_reason(&anyhow!(
            "workspace directory could not be opened safely: NotADirectory"
        )),
        "a workspace path component is not a directory"
    );
    assert_eq!(
        scoped_capture_failure_reason(&anyhow!(
            "workspace file could not be opened safely: FilesystemLoop"
        )),
        "a symbolic link or loop was encountered"
    );
    assert_eq!(
        scoped_capture_failure_reason(&anyhow!("host path /secret/customer/data was rejected")),
        "a filesystem safety check failed"
    );
}

#[cfg(unix)]
#[test]
fn scoped_capture_sanitizes_permission_denied_from_a_real_file_open() {
    use std::os::unix::fs::PermissionsExt;

    if unsafe { libc::geteuid() } == 0 {
        eprintln!("skipping permission-denied staging test when running as root");
        return;
    }

    let fixture = super::Fixture::new();
    let root = fixture.0.join("apps/web");
    std::fs::create_dir_all(&root).unwrap();
    let file = root.join("generated-output");
    std::fs::write(&file, b"generated").unwrap();
    let mut permissions = std::fs::metadata(&file).unwrap().permissions();
    permissions.set_mode(0o0);
    std::fs::set_permissions(&file, permissions).unwrap();

    let workspace =
        crate::execution::docker::sandbox::ValidatedWorkspace::inspect(&fixture.0).unwrap();
    let error = super::capture_scoped(&workspace, "apps/web", 1024 * 1024).unwrap_err();
    let message = error.to_string();

    assert!(message.contains("a filesystem entry is not accessible"));
    assert!(!message.contains(file.to_str().unwrap()));
}

#[cfg(unix)]
#[test]
fn nested_unclassified_os_error_includes_only_the_numeric_code() {
    let error = anyhow::Error::new(std::io::Error::from_raw_os_error(12345))
        .context("capture failed for /private/workspace/path");
    assert_eq!(
        scoped_capture_failure_reason(&error),
        "an operating system filesystem operation failed (OS error 12345)"
    );
    assert!(!scoped_capture_failure_reason(&error).contains("/private/workspace/path"));
}

#[test]
fn error_without_raw_os_code_reports_only_its_safe_error_kind() {
    let error = anyhow::Error::new(std::io::Error::from(std::io::ErrorKind::Other));
    assert_eq!(
        scoped_capture_failure_reason(&error),
        "an operating system filesystem operation failed (error kind Other)"
    );
}
