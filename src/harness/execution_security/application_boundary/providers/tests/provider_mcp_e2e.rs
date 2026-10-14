use super::super::super::super::{
    mcp_server::{BoundaryAccess, ServerConfig},
    session_bridge::SessionBinding,
};
use super::super::super::CliProvider;
use super::test_boundary;
use crate::harness::{
    ExecutionMode, ToolAccess,
    pi_proc::{PollState, StreamEvt, spawn_with_input_provider_env},
};
use serde_json::{Value, json};
use std::{
    fs,
    io::{BufRead, Write},
    os::unix::net::UnixListener,
    thread,
    time::Duration,
};

#[test]
fn every_fake_provider_launches_the_application_mcp_tool_for_each_tool_mode() {
    let current_test = std::env::current_exe().unwrap();
    let app_binary = current_test
        .parent()
        .and_then(std::path::Path::parent)
        .expect("test binary lives in target/debug/deps")
        .join("koolade");
    if !app_binary.is_file() {
        eprintln!("skipping provider MCP process test; build target/debug/koolade first");
        return;
    }

    let root = std::env::temp_dir().join(format!("koolade-provider-mcp-{}", uuid::Uuid::new_v4()));
    fs::create_dir(&root).unwrap();
    let fake_cli = root.join("fake provider cli");
    fs::write(
        &fake_cli,
        "#!/bin/sh\nset -eu\ncapture=\"$1\"\nif [ \"$KOOLADE_TEST_TOOL_ACCESS\" != none ]; then\n  printf '%s\\n' \"$KOOLADE_TEST_TOOL_REQUEST\" | \"$KOOLADE_TEST_APP_BINARY\" --internal-mcp-server \"$KOOLADE_TEST_SERVER_CONFIG\" > \"$capture.response\"\nfi\nprintf 'launched\\n' > \"$capture\"\n",
    )
    .unwrap();
    make_executable(&fake_cli);

    let providers = [
        CliProvider::Pi,
        CliProvider::Codex,
        CliProvider::Claude,
        CliProvider::OpenCode,
        CliProvider::Copilot,
        CliProvider::Antigravity,
    ];
    for mode in ExecutionMode::ALL {
        for provider in providers {
            let access = mode.tool_access();
            let mut boundary = test_boundary(access);
            boundary.app_binary = Some(app_binary.clone());
            let capture = root.join(format!("{provider:?}-{mode:?}.capture"));
            let case_dir = root.join(format!("{provider:?}-{mode:?}"));
            fs::create_dir(&case_dir).unwrap();
            let (server_config, bridge) = if access == ToolAccess::None {
                (None, None)
            } else {
                let socket = case_dir.join("execution.sock");
                let listener = UnixListener::bind(&socket).unwrap();
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
                let access_name = binding.access.clone();
                let bridge = thread::spawn(move || {
                    let (stream, _) = listener.accept().unwrap();
                    let mut reader = std::io::BufReader::new(stream.try_clone().unwrap());
                    let mut line = String::new();
                    reader.read_line(&mut line).unwrap();
                    let request: Value = serde_json::from_str(&line).unwrap();
                    assert_eq!(request["access"], access_name);
                    assert_eq!(request["task_id"], binding.task_id);
                    assert_eq!(request["command"], "printf provider-tool-ok");
                    let mut stream = reader.into_inner();
                    serde_json::to_writer(
                        &mut stream,
                        &json!({"stdout":"provider-tool-ok","stderr":"","exit_code":0,"is_error":false}),
                    )
                    .unwrap();
                    stream.write_all(b"\n").unwrap();
                });
                (Some(path), Some(bridge))
            };
            boundary.server_config = server_config.clone();
            let mut args = vec![fake_cli.to_string_lossy().into_owned()];
            args.push(capture.to_string_lossy().into_owned());
            let mut env = Vec::new();
            boundary.configure(provider, &mut args, &mut env).unwrap();
            env.extend([
                ("KOOLADE_TEST_APP_BINARY".into(), app_binary.to_string_lossy().into_owned()),
                (
                    "KOOLADE_TEST_SERVER_CONFIG".into(),
                    server_config
                        .as_ref()
                        .map(|path| path.to_string_lossy().into_owned())
                        .unwrap_or_default(),
                ),
                (
                    "KOOLADE_TEST_TOOL_ACCESS".into(),
                    if access == ToolAccess::None { "none" } else { "enabled" }.into(),
                ),
                (
                    "KOOLADE_TEST_TOOL_REQUEST".into(),
                    json!({
                        "jsonrpc":"2.0", "id":1, "method":"tools/call",
                        "params":{"name":"koolade_bash","arguments":{"command":"printf provider-tool-ok"}}
                    })
                    .to_string(),
                ),
            ]);
            let task = spawn_with_input_provider_env(
                &args,
                boundary.working_directory(),
                None,
                &env,
                provider,
            )
            .unwrap();
            loop {
                match task.poll_next(Duration::from_millis(100)) {
                    Ok(StreamEvt::Exited(true)) => break,
                    Ok(StreamEvt::Exited(false)) => panic!("{provider:?} failed in {mode:?}"),
                    Ok(StreamEvt::Stdout(_) | StreamEvt::Stderr(_)) | Err(PollState::Pending) => {}
                    Err(PollState::Closed) => panic!("{provider:?} {mode:?} closed early"),
                }
            }
            assert!(capture.is_file());
            if let Some(bridge) = bridge {
                bridge.join().unwrap();
                let response: Value = serde_json::from_slice(
                    &fs::read(capture.with_extension("capture.response")).unwrap(),
                )
                .unwrap();
                assert_eq!(response["result"]["isError"], false);
                assert_eq!(
                    response["result"]["content"][0]["text"],
                    "provider-tool-ok\nExit code: 0"
                );
            } else {
                assert!(boundary.mcp_tool_names().is_empty());
            }
        }
    }
    fs::remove_dir_all(root).unwrap();
}

fn make_executable(path: &std::path::Path) {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
}

fn set_private(path: &std::path::Path) {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600)).unwrap();
}
