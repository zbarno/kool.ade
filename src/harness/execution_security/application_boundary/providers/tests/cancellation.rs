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
/// clean environment and process-group cancellation. Each child must receive
/// only its own private OpenCode home, file-backed config paths, and app-server
/// identity; cancelling one task kills its descendants without cancelling the
/// other. Config-only unit tests cannot prove these values reach the spawned
/// process or prove the OS process-group boundary. Dependency: Unix shell/process
/// groups only; no Docker, network, or vendor API. Setup: two fake CLI processes.
#[test]
fn concurrent_fake_provider_tasks_isolate_private_state_and_cancel_independently() {
    let root = tempfile::tempdir().unwrap();
    let cli = root.path().join("fake-opencode");
    fs::write(
        &cli,
        "#!/bin/sh\nset -eu\ntask=\"$1\"\nroot=\"$2\"\nprintf 'HOME=%s\\nXDG_CONFIG_HOME=%s\\nXDG_DATA_HOME=%s\\nOPENCODE_CONFIG=%s\\nOPENCODE_CONFIG_DIR=%s\\nOPENCODE_CONFIG_CONTENT=%s\\n' \"$HOME\" \"$XDG_CONFIG_HOME\" \"$XDG_DATA_HOME\" \"$OPENCODE_CONFIG\" \"$OPENCODE_CONFIG_DIR\" \"$OPENCODE_CONFIG_CONTENT\" > \"$root/$task.capture\"\n( : > \"$root/$task.descendant_started\"; while [ ! -f \"$root/$task.finish\" ]; do sleep 0.01; done; : > \"$root/$task.descendant_finished\" ) &\nwhile [ ! -f \"$root/$task.descendant_started\" ]; do sleep 0.01; done\n: > \"$root/$task.started\"\nwhile [ ! -f \"$root/$task.finish\" ]; do sleep 0.01; done\nwait\n",
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
    let first_expected_home = first_private_directory.join("opencode-home");
    let second_expected_home = second_private_directory.join("opencode-home");
    assert_eq!(first_home, first_expected_home.to_str().unwrap());
    assert_eq!(second_home, second_expected_home.to_str().unwrap());
    assert_ne!(first_home, second_home);
    assert_eq!(
        captured_value(&first_capture, "XDG_CONFIG_HOME"),
        format!("{first_home}/.config")
    );
    assert_eq!(
        captured_value(&second_capture, "XDG_CONFIG_HOME"),
        format!("{second_home}/.config")
    );
    assert_eq!(
        captured_value(&first_capture, "XDG_DATA_HOME"),
        format!("{first_home}/.local/share")
    );
    assert_eq!(
        captured_value(&second_capture, "XDG_DATA_HOME"),
        format!("{second_home}/.local/share")
    );

    let first_config_path = first_private_directory.join("opencode.json");
    let second_config_path = second_private_directory.join("opencode.json");
    assert_eq!(
        captured_value(&first_capture, "OPENCODE_CONFIG"),
        first_config_path.to_str().unwrap()
    );
    assert_eq!(
        captured_value(&second_capture, "OPENCODE_CONFIG"),
        second_config_path.to_str().unwrap()
    );
    assert_eq!(
        captured_value(&first_capture, "OPENCODE_CONFIG_DIR"),
        first_expected_home.join("config").to_str().unwrap()
    );
    assert_eq!(
        captured_value(&second_capture, "OPENCODE_CONFIG_DIR"),
        second_expected_home.join("config").to_str().unwrap()
    );

    let first_config = captured_value(&first_capture, "OPENCODE_CONFIG_CONTENT");
    let second_config = captured_value(&second_capture, "OPENCODE_CONFIG_CONTENT");
    assert!(first_config.contains(&first.server_name));
    assert!(!first_config.contains(&second.server_name));
    assert!(second_config.contains(&second.server_name));
    assert!(!second_config.contains(&first.server_name));
    let first_file_config: serde_json::Value =
        serde_json::from_slice(&fs::read(first_config_path).unwrap()).unwrap();
    let second_file_config: serde_json::Value =
        serde_json::from_slice(&fs::read(second_config_path).unwrap()).unwrap();
    let first_runtime_config: serde_json::Value = serde_json::from_str(first_config).unwrap();
    let second_runtime_config: serde_json::Value = serde_json::from_str(second_config).unwrap();
    assert_eq!(first_file_config, first_runtime_config);
    assert_eq!(second_file_config, second_runtime_config);
    assert_eq!(first_file_config["mcp"].as_object().unwrap().len(), 1);
    assert_eq!(second_file_config["mcp"].as_object().unwrap().len(), 1);
    assert!(
        first_file_config["mcp"]
            .get(first.server_name.as_str())
            .is_some()
    );
    assert!(
        first_file_config["mcp"]
            .get(second.server_name.as_str())
            .is_none()
    );
    assert!(
        second_file_config["mcp"]
            .get(second.server_name.as_str())
            .is_some()
    );
    assert!(
        second_file_config["mcp"]
            .get(first.server_name.as_str())
            .is_none()
    );

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
