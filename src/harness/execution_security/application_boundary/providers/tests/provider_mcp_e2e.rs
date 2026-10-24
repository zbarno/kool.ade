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
    time::{Duration, Instant},
};

mod registration;
mod support;

use support::{current_application_binary, make_executable, read_response, server_session};

const PROVIDERS: [CliProvider; 6] = [
    CliProvider::Codex,
    CliProvider::Claude,
    CliProvider::Pi,
    CliProvider::Antigravity,
    CliProvider::OpenCode,
    CliProvider::Copilot,
];

/// Production boundary: tool-enabled provider config through a spawned fake
/// CLI into the app-owned MCP server and a controlled Unix-socket fixture.
/// Invariant: each tool-enabled config registers only the app server, the fake
/// CLI uses its parsed executable and config path with the fixed internal-server
/// flag, role-bound tools are enforced, and requests carry app-owned session
/// identity. Unit tests cannot prove config values reach the app protocol
/// endpoint through a child process. The fixture records requests but does not
/// execute commands. Setup: Unix shell/sockets and the
/// all-targets-built app binary; no Docker, network, vendor CLI, or credentials.
/// It starts one fake child per provider/role pair and a bridge fixture wherever
/// that role permits MCP tools. Roles without tools must register no server.
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
                ToolAccess::RuntimeQaScreenshots => (None, None, None),
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
            let registered_server =
                registration::assert_server_registration(provider, &boundary, &args, &env);
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
                    registered_server
                        .as_ref()
                        .map(|args| args[0].clone())
                        .unwrap_or_default(),
                ),
                (
                    "KOOLADE_TEST_SERVER_CONFIG".into(),
                    registered_server
                        .as_ref()
                        .and_then(|args| args.get(2))
                        .cloned()
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
            let deadline = Instant::now() + Duration::from_secs(20);
            loop {
                if Instant::now() >= deadline {
                    task.kill();
                    panic!("{provider:?} timed out for role {role:?}");
                }
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
                "the controlled provider fixture executed a command locally for {provider:?} {role:?}"
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
