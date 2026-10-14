use super::*;

#[test]
fn persistent_runtime_is_retained_until_stopped_and_keeps_bounded_startup_logs() {
    let backend = backend();
    let session = session(backend.clone());
    let (output, process) = session
        .start_runtime_process("echo readiness passed", std::time::Duration::from_secs(2))
        .unwrap();

    assert_eq!(output.stdout, "hello\n");
    let process = process.expect("successful startup returns a process handle");
    assert!(process.is_active());
    assert!(process.logs().len() <= 1024 * 1024);
    assert!(!backend.released.load(Ordering::Acquire));

    process.stop().unwrap();
    assert!(!process.is_active());
    assert!(backend.released.load(Ordering::Acquire));
    assert!(backend.workspace_released.load(Ordering::Acquire));
}

#[test]
fn cancelling_the_session_releases_retained_runtime_processes() {
    let backend = backend();
    let session = session(backend.clone());
    let (_, process) = session
        .start_runtime_process("echo readiness passed", std::time::Duration::from_secs(2))
        .unwrap();
    drop(process);
    assert!(!backend.released.load(Ordering::Acquire));

    session.cancel().unwrap();
    assert!(backend.released.load(Ordering::Acquire));
    assert!(backend.workspace_released.load(Ordering::Acquire));
}

#[test]
fn dropping_the_session_releases_retained_runtime_processes() {
    let backend = backend();
    let session = session(backend.clone());
    let (_, process) = session
        .start_runtime_process("echo readiness passed", std::time::Duration::from_secs(2))
        .unwrap();
    drop(process);

    drop(session);
    assert!(backend.released.load(Ordering::Acquire));
    assert!(backend.workspace_released.load(Ordering::Acquire));
}
