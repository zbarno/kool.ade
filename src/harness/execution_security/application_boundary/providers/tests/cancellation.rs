use super::super::{CliProvider, configure};
use super::test_boundary;
use crate::harness::pi_proc::{ChildTask, PollState, StreamEvt};
use crate::harness::{ToolAccess, pi_proc::spawn_with_input_provider_env};
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::Path,
    thread,
    time::{Duration, Instant},
};

/// Production boundary: concurrent provider processes use the application-owned
/// clean environment and process-group cancellation. The invariant is that each
/// task sees only its own private OpenCode home/config, and cancelling one task
/// kills its descendants without cancelling the other. Config-only unit tests
/// cannot prove the OS process-group boundary. Dependency: Unix shell/process
/// groups only; no Docker, network, or vendor API. Setup: two fake CLI processes.
#[test]
fn concurrent_fake_provider_tasks_isolate_private_state_and_cancel_independently() {
    let root = tempfile::tempdir().unwrap();
    let cli = root.path().join("fake-opencode");
    fs::write(
        &cli,
        "#!/bin/sh\nset -eu\ntask=\"$1\"\nroot=\"$2\"\nprintf 'HOME=%s\\nXDG_CONFIG_HOME=%s\\nOPENCODE_CONFIG_CONTENT=%s\\n' \"$HOME\" \"$XDG_CONFIG_HOME\" \"$OPENCODE_CONFIG_CONTENT\" > \"$root/$task.capture\"\n( : > \"$root/$task.descendant_started\"; while [ ! -f \"$root/$task.finish\" ]; do sleep 0.01; done; : > \"$root/$task.descendant_finished\" ) &\nwhile [ ! -f \"$root/$task.descendant_started\" ]; do sleep 0.01; done\n: > \"$root/$task.started\"\nwhile [ ! -f \"$root/$task.finish\" ]; do sleep 0.01; done\nwait\n",
    )
    .unwrap();
    fs::set_permissions(&cli, fs::Permissions::from_mode(0o700)).unwrap();

    let mut first = test_boundary(ToolAccess::BoundedImplementation);
    first.server_name = format!("koolade_test_{}", uuid::Uuid::new_v4().simple());
    let mut second = test_boundary(ToolAccess::BoundedImplementation);
    second.server_name = format!("koolade_test_{}", uuid::Uuid::new_v4().simple());
    let first_private_directory = first.scratch.path.clone();
    let second_private_directory = second.scratch.path.clone();
    let first_task = spawn_fake_opencode(&cli, root.path(), "first", &first);
    let second_task = spawn_fake_opencode(&cli, root.path(), "second", &second);

    wait_for_file(&root.path().join("first.started"));
    wait_for_file(&root.path().join("second.started"));
    let first_capture = fs::read_to_string(root.path().join("first.capture")).unwrap();
    let second_capture = fs::read_to_string(root.path().join("second.capture")).unwrap();

    let first_home = captured_value(&first_capture, "HOME");
    let second_home = captured_value(&second_capture, "HOME");
    assert_ne!(first_home, second_home);
    assert!(first_home.starts_with(first.scratch.path.to_str().unwrap()));
    assert!(second_home.starts_with(second.scratch.path.to_str().unwrap()));
    assert_eq!(
        captured_value(&first_capture, "XDG_CONFIG_HOME"),
        format!("{first_home}/.config")
    );
    assert_eq!(
        captured_value(&second_capture, "XDG_CONFIG_HOME"),
        format!("{second_home}/.config")
    );

    let first_config = captured_value(&first_capture, "OPENCODE_CONFIG_CONTENT");
    let second_config = captured_value(&second_capture, "OPENCODE_CONFIG_CONTENT");
    assert!(first_config.contains(&first.server_name));
    assert!(!first_config.contains(&second.server_name));
    assert!(second_config.contains(&second.server_name));
    assert!(!second_config.contains(&first.server_name));

    first_task.kill();
    wait_for_exit(&first_task, false);
    assert!(!root.path().join("first.descendant_finished").exists());

    fs::write(root.path().join("first.finish"), "release cancelled task").unwrap();
    let observation_deadline = Instant::now() + Duration::from_secs(1);
    while !root.path().join("first.descendant_finished").exists()
        && Instant::now() < observation_deadline
    {
        thread::sleep(Duration::from_millis(10));
    }
    assert!(
        !root.path().join("first.descendant_finished").exists(),
        "a cancelled provider descendant must not survive to consume later state"
    );

    assert!(
        !root.path().join("second.descendant_finished").exists(),
        "cancelling one provider task must not release the other task's state"
    );
    fs::write(root.path().join("second.finish"), "complete surviving task").unwrap();
    wait_for_exit(&second_task, true);
    assert!(root.path().join("second.descendant_finished").exists());

    drop(first_task);
    drop(second_task);
    drop(first);
    drop(second);
    assert!(!first_private_directory.exists());
    assert!(!second_private_directory.exists());
}

fn spawn_fake_opencode(
    cli: &Path,
    root: &Path,
    task_id: &str,
    boundary: &super::super::super::ApplicationBoundary,
) -> ChildTask {
    let mut argv = vec![
        cli.to_string_lossy().into_owned(),
        task_id.into(),
        root.to_string_lossy().into_owned(),
    ];
    let mut env = Vec::new();
    configure(boundary, CliProvider::OpenCode, &mut argv, &mut env).unwrap();
    env.push(("PATH".into(), "/usr/bin:/bin".into()));
    spawn_with_input_provider_env(
        &argv,
        boundary.working_directory(),
        None,
        &env,
        CliProvider::OpenCode,
        root,
    )
    .unwrap()
}

fn wait_for_file(path: &Path) {
    let deadline = Instant::now() + Duration::from_secs(3);
    while Instant::now() < deadline {
        if path.exists() {
            return;
        }
        thread::sleep(Duration::from_millis(10));
    }
    panic!("timed out waiting for {}", path.display());
}

fn wait_for_exit(task: &ChildTask, expected_success: bool) {
    let deadline = Instant::now() + Duration::from_secs(3);
    while Instant::now() < deadline {
        match task.poll_next(Duration::from_millis(50)) {
            Ok(StreamEvt::Exited(success)) => {
                assert_eq!(success, expected_success);
                return;
            }
            Ok(StreamEvt::Stdout(_) | StreamEvt::Stderr(_)) | Err(PollState::Pending) => {}
            Err(PollState::Closed) => panic!("provider process stream closed before exit"),
        }
    }
    panic!("provider process did not exit before the deadline");
}

fn captured_value<'a>(capture: &'a str, key: &str) -> &'a str {
    capture
        .lines()
        .find_map(|line| line.strip_prefix(&format!("{key}=")))
        .unwrap_or_else(|| panic!("fake provider did not capture {key}"))
}
