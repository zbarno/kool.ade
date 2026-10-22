use super::super::super::super::super::{
    mcp_server::{BoundaryAccess, ServerConfig},
    session_bridge::SessionBinding,
};
use crate::harness::ToolAccess;
use serde_json::{Value, json};
use std::{
    fs,
    io::{BufRead, Write},
    os::unix::net::UnixListener,
    path::{Path, PathBuf},
    thread,
    time::{Duration, Instant},
};

pub(super) fn server_session(
    case_dir: &Path,
    access: ToolAccess,
    expected_command: &str,
    host_marker: &Path,
) -> (PathBuf, thread::JoinHandle<()>) {
    let listener = UnixListener::bind(case_dir.join("execution.sock")).unwrap();
    listener.set_nonblocking(true).unwrap();
    let task_id = uuid::Uuid::new_v4().hyphenated().to_string();
    let binding = SessionBinding {
        session_id: uuid::Uuid::new_v4().hyphenated().to_string(),
        capability_id: uuid::Uuid::new_v4().hyphenated().to_string(),
        task_id: task_id.clone(),
        workspace_id: format!(
            "{}:{}:{}",
            uuid::Uuid::new_v4().hyphenated(),
            task_id,
            "a".repeat(40)
        ),
        access: if access == ToolAccess::ReadOnly {
            "read_only".into()
        } else {
            "implementation".into()
        },
        target_id: "local".into(),
        target_arch: Some("x86_64".into()),
    };
    let config = if access == ToolAccess::ReadOnly {
        ServerConfig::read_only(&binding)
    } else {
        ServerConfig {
            protocol_version: 2,
            access: BoundaryAccess::Implementation,
            session_id: binding.session_id.clone(),
            task_id: binding.task_id.clone(),
            workspace_id: binding.workspace_id.clone(),
            capability_id: binding.capability_id.clone(),
            target_id: Some(binding.target_id.clone()),
            target_arch: binding.target_arch.clone(),
            resource_socket: Some(case_dir.join("resource.sock")),
            resource_cache: Some(case_dir.join("cache")),
        }
    };
    let path = case_dir.join("server.json");
    fs::write(&path, serde_json::to_vec(&config).unwrap()).unwrap();
    set_private(&path);
    let command = expected_command.to_owned();
    let command_capture = case_dir.join("bridge-command");
    let host_marker = host_marker.to_path_buf();
    let bridge = thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(5);
        let (stream, _) = loop {
            match listener.accept() {
                Ok(connection) => break connection,
                Err(error)
                    if error.kind() == std::io::ErrorKind::WouldBlock
                        && Instant::now() < deadline =>
                {
                    thread::sleep(Duration::from_millis(10));
                }
                Err(error) => panic!("provider MCP connection did not arrive: {error}"),
            }
        };
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let mut reader = std::io::BufReader::new(stream.try_clone().unwrap());
        let mut line = String::new();
        let bytes = reader
            .read_line(&mut line)
            .expect("provider MCP request should be readable");
        assert!(bytes > 0, "provider MCP request should not be empty");
        let request: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(request["access"], binding.access);
        assert_eq!(request["session_id"], binding.session_id);
        assert_eq!(request["capability_id"], binding.capability_id);
        assert_eq!(request["task_id"], binding.task_id);
        assert_eq!(request["workspace_id"], binding.workspace_id);
        assert_eq!(request["command"], command);
        fs::write(command_capture, request["command"].as_str().unwrap()).unwrap();
        assert!(
            !host_marker.exists(),
            "the session fixture records tool requests without executing them"
        );
        let mut stream = reader.into_inner();
        serde_json::to_writer(
            &mut stream,
            &json!({
                "stdout": if request["access"] == "read_only" { "read result" } else { "write request received" },
                "stderr":"", "exit_code":0, "is_error":false,
            }),
        )
        .unwrap();
        stream.write_all(b"\n").unwrap();
    });
    (path, bridge)
}

pub(super) fn current_application_binary() -> PathBuf {
    let current_test = std::env::current_exe().unwrap();
    let app_binary = current_test
        .parent()
        .and_then(Path::parent)
        .expect("test binary lives in target/debug/deps")
        .join("koolade");
    assert!(
        app_binary.is_file(),
        "all-targets must build the application binary for provider MCP tests"
    );
    app_binary
}

pub(super) fn read_response(path: &Path) -> Value {
    serde_json::from_slice(&fs::read(path).unwrap()).unwrap()
}

pub(super) fn make_executable(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
}

fn set_private(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600)).unwrap();
}
