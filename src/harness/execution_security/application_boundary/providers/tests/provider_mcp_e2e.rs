use super::super::super::super::{
    mcp_server::{BoundaryAccess, ServerConfig},
    session_bridge::SessionBinding,
};
use super::super::super::CliProvider;
use super::test_boundary;
use crate::{
    execution::contract::HarnessRole,
    harness::{
        ToolAccess,
        pi_proc::{PollState, StreamEvt, spawn_with_input_provider_env},
    },
};
use serde_json::{Value, json};
use std::{
    fs,
    io::{BufRead, Write},
    os::unix::net::UnixListener,
    thread,
    time::Duration,
};

const PROVIDERS: [CliProvider; 6] = [
    CliProvider::Codex,
    CliProvider::Claude,
    CliProvider::Pi,
    CliProvider::Antigravity,
    CliProvider::OpenCode,
    CliProvider::Copilot,
];

#[test]
fn every_provider_role_calls_only_its_application_owned_mcp_authority() {
    let app_binary = current_application_binary();
    let root = std::env::temp_dir().join(format!("koolade-provider-mcp-{}", uuid::Uuid::new_v4()));
    fs::create_dir(&root).unwrap();
    let fake_cli = root.join("fake provider cli");
    fs::write(
        &fake_cli,
        "#!/bin/sh\nset -eu\ncapture=\"$1\"\nif [ -n \"$KOOLADE_TEST_SERVER_CONFIG\" ]; then\n  printf '%s\\n' \"$KOOLADE_TEST_TOOL_REQUEST\" | \"$KOOLADE_TEST_APP_BINARY\" --internal-mcp-server \"$KOOLADE_TEST_SERVER_CONFIG\" > \"$capture.response\"\n  if [ \"$KOOLADE_TEST_DENIED_REQUEST\" != none ]; then\n    printf '%s\\n' \"$KOOLADE_TEST_DENIED_REQUEST\" | \"$KOOLADE_TEST_APP_BINARY\" --internal-mcp-server \"$KOOLADE_TEST_SERVER_CONFIG\" > \"$capture.denied\"\n  fi\nfi\nprintf 'launched\\n' > \"$capture\"\n",
    )
    .unwrap();
    make_executable(&fake_cli);

    let host_marker = root.join("host-command-ran");
    let mut exercised = 0usize;
    for role in HarnessRole::ALL {
        let access = role.harness_mode().tool_access();
        for provider in PROVIDERS {
            let case_dir = root.join(format!("{provider:?}-{role:?}"));
            fs::create_dir(&case_dir).unwrap();
            let capture = case_dir.join("capture");
            let (server_config, bridge, command) = match access {
                ToolAccess::None => (None, None, None),
                ToolAccess::ReadOnly => {
                    let command = "cat README.md".to_owned();
                    let (path, bridge) = server_session(&case_dir, access, &command, &host_marker);
                    (Some(path), Some(bridge), Some(command))
                }
                ToolAccess::BoundedImplementation => {
                    let command = format!("printf host-executed > '{}'", host_marker.display());
                    let (path, bridge) = server_session(&case_dir, access, &command, &host_marker);
                    (Some(path), Some(bridge), Some(command))
                }
            };

            let mut boundary = test_boundary(access);
            boundary.app_binary = Some(app_binary.clone());
            boundary.server_config = server_config.clone();
            let mut args = vec![
                fake_cli.to_string_lossy().into_owned(),
                capture.to_string_lossy().into_owned(),
            ];
            let mut env = Vec::new();
            boundary.configure(provider, &mut args, &mut env).unwrap();
            let tool_request = command.as_ref().map_or(Value::Null, |command| {
                json!({
                    "jsonrpc":"2.0", "id":1, "method":"tools/call",
                    "params":{"name":"koolade_bash","arguments":{"command":command}}
                })
            });
            let denied_request = if access == ToolAccess::ReadOnly {
                json!({
                    "jsonrpc":"2.0", "id":2, "method":"tools/call",
                    "params":{"name":"koolade_resource","arguments":{"url":"https://example.invalid","purpose":"negative permission test"}}
                })
                .to_string()
            } else {
                "none".into()
            };
            env.extend([
                (
                    "KOOLADE_TEST_APP_BINARY".into(),
                    app_binary.to_string_lossy().into_owned(),
                ),
                (
                    "KOOLADE_TEST_SERVER_CONFIG".into(),
                    server_config
                        .as_ref()
                        .map(|p| p.to_string_lossy().into_owned())
                        .unwrap_or_default(),
                ),
                ("KOOLADE_TEST_TOOL_REQUEST".into(), tool_request.to_string()),
                ("KOOLADE_TEST_DENIED_REQUEST".into(), denied_request),
            ]);
            let task = spawn_with_input_provider_env(
                &args,
                boundary.working_directory(),
                None,
                &env,
                provider,
                &case_dir,
            )
            .unwrap();
            loop {
                match task.poll_next(Duration::from_millis(100)) {
                    Ok(StreamEvt::Exited(true)) => break,
                    Ok(StreamEvt::Exited(false)) => panic!("{provider:?} failed for role {role:?}"),
                    Ok(StreamEvt::Stdout(_) | StreamEvt::Stderr(_)) | Err(PollState::Pending) => {}
                    Err(PollState::Closed) => panic!("{provider:?} closed for role {role:?}"),
                }
            }
            assert!(
                capture.is_file(),
                "{provider:?} did not launch for {role:?}"
            );
            if let Some(bridge) = bridge {
                bridge.join().unwrap();
                let response = read_response(&capture.with_extension("response"));
                assert_eq!(
                    response["result"]["isError"], false,
                    "{provider:?} {role:?}"
                );
                let expected = if access == ToolAccess::ReadOnly {
                    "read result"
                } else {
                    "write request received"
                };
                assert_eq!(
                    response["result"]["content"][0]["text"],
                    format!("{expected}\nExit code: 0")
                );
                assert_eq!(
                    fs::read_to_string(case_dir.join("bridge-command")).unwrap(),
                    command.unwrap()
                );
                exercised += 1;
                if access == ToolAccess::ReadOnly {
                    let denied = read_response(&capture.with_extension("denied"));
                    assert_eq!(
                        denied["result"]["isError"], true,
                        "{provider:?} {role:?} exposed an implementation-only tool"
                    );
                    assert!(
                        denied["result"]["content"][0]["text"]
                            .as_str()
                            .unwrap()
                            .contains("not available")
                    );
                }
            } else {
                assert!(
                    boundary.mcp_tool_names().is_empty(),
                    "{provider:?} {role:?} received MCP tools"
                );
            }
            assert!(
                !host_marker.exists(),
                "provider command escaped the application bridge for {provider:?} {role:?}"
            );
        }
    }
    assert_eq!(
        exercised,
        PROVIDERS.len()
            * HarnessRole::ALL
                .iter()
                .filter(|role| role.harness_mode().tool_access() != ToolAccess::None)
                .count()
    );
    fs::remove_dir_all(root).unwrap();
}

fn server_session(
    case_dir: &std::path::Path,
    access: ToolAccess,
    expected_command: &str,
    host_marker: &std::path::Path,
) -> (std::path::PathBuf, thread::JoinHandle<()>) {
    let listener = UnixListener::bind(case_dir.join("execution.sock")).unwrap();
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
        let (stream, _) = listener.accept().unwrap();
        let mut reader = std::io::BufReader::new(stream.try_clone().unwrap());
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
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
            "the MCP layer executed the command on the host"
        );
        let mut stream = reader.into_inner();
        serde_json::to_writer(&mut stream, &json!({
            "stdout": if request["access"] == "read_only" { "read result" } else { "write request received" },
            "stderr":"", "exit_code":0, "is_error":false,
        })).unwrap();
        stream.write_all(b"\n").unwrap();
    });
    (path, bridge)
}

fn current_application_binary() -> std::path::PathBuf {
    let current_test = std::env::current_exe().unwrap();
    let app_binary = current_test
        .parent()
        .and_then(std::path::Path::parent)
        .expect("test binary lives in target/debug/deps")
        .join("koolade");
    assert!(
        app_binary.is_file(),
        "all-targets must build the application binary for provider MCP tests"
    );
    app_binary
}

fn read_response(path: &std::path::Path) -> Value {
    serde_json::from_slice(&fs::read(path).unwrap()).unwrap()
}

fn make_executable(path: &std::path::Path) {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
}

fn set_private(path: &std::path::Path) {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600)).unwrap();
}
