use super::*;

#[test]
fn script_session_uses_stdin_collects_validated_streams_and_releases() {
    let backend = backend();
    let session = session(backend.clone());
    let output = session
        .run_bounded_script("printf hello", Duration::from_secs(10))
        .unwrap();
    assert_eq!(output.stdout, "hello\n");
    assert_eq!(output.stderr, "warning\n");
    assert_eq!(output.result.exit_code, Some(0));
    let request = backend.request.lock().unwrap().clone().unwrap();
    assert_eq!(request.argv, ["sh", "-s"]);
    assert_eq!(request.stdin.as_deref(), Some("printf hello"));
    assert!(backend.released.load(Ordering::Acquire));
    assert!(backend.workspace_released.load(Ordering::Acquire));
}

#[test]
fn script_session_rejects_oversized_or_unbounded_requests() {
    let backend = backend();
    let session = session(backend.clone());
    assert!(
        session
            .run_bounded_script(
                &"x".repeat(super::super::super::contract::MAX_SCRIPT_BYTES + 1),
                Duration::from_secs(10)
            )
            .is_err()
    );
    assert!(backend.request.lock().unwrap().is_none());
}
