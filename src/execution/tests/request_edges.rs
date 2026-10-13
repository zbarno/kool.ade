use super::*;

#[test]
fn request_validation_rejects_invalid_ids_host_paths_and_shell_command_strings() {
    let mut invalid_id = request();
    invalid_id.execution_id = "exec-1".into();
    assert!(invalid_id.validate_at(1).is_err());

    let mut host_path = request();
    host_path.argv.push("/home/operator-a/project".into());
    assert!(host_path.validate_at(1).is_err());

    let mut shell = request();
    shell.argv = vec!["bash".into(), "-c".into(), "git status".into()];
    assert!(shell.validate_at(1).is_err());

    let mut combined_shell = request();
    combined_shell.argv = vec!["bash".into(), "-lc".into(), "git status".into()];
    assert!(combined_shell.validate_at(1).is_err());

    let mut unicode_shell = request();
    unicode_shell.argv = vec!["bash".into(), "-éclair".into()];
    assert!(unicode_shell.validate_at(1).is_err());

    let mut bounded_script = request();
    bounded_script.argv = vec!["sh".into(), "-s".into()];
    bounded_script.stdin = Some("git status --short\n".into());
    assert!(bounded_script.validate_at(1).is_ok());

    bounded_script.stdin = Some("x".repeat(super::super::contract::MAX_SCRIPT_BYTES + 1));
    assert!(bounded_script.validate_at(1).is_err());

    let mut shell_without_script = request();
    shell_without_script.argv = vec!["sh".into(), "-s".into()];
    assert!(shell_without_script.validate_at(1).is_err());

    let mut secret_name = request();
    secret_name
        .environment_allowlist
        .insert("GITHUB_TOKEN".into());
    assert!(secret_name.validate_at(1).is_err());
}

#[test]
fn cancelled_and_expired_requests_are_reported_without_starting() {
    let request = request();
    assert_eq!(
        request.validate_control(2, 2, false),
        Err(ExecutionErrorKind::Cancelled)
    );
    assert_eq!(
        request.validate_control(10_000, 1, false),
        Err(ExecutionErrorKind::DeadlineExceeded)
    );
    assert!(request.validate_control(2, 1, false).is_ok());
}
