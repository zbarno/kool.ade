use super::*;
use std::io;

#[path = "tests/screenshot.rs"]
mod screenshot;

#[test]
fn initialize_echoes_protocol_and_lists_only_boundary_tools() {
    let config = test_config(super::super::BoundaryAccess::Implementation);
    let input = concat!(
        "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"params\":{\"protocolVersion\":\"2025-03-26\"}}\n",
        "{\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\"}\n",
        "{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"tools/list\"}\n",
    );
    let mut output = Vec::new();
    serve_stream(
        Path::new("/tmp/mcp/server.json"),
        &config,
        &mut io::Cursor::new(input),
        &mut output,
    )
    .unwrap();
    let responses = output
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
        .map(|line| serde_json::from_slice::<Value>(line).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(responses.len(), 2);
    assert_eq!(responses[0]["result"]["protocolVersion"], "2025-03-26");
    let names = responses[1]["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|tool| tool["name"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(
        names,
        ["koolade_bash", "koolade_resource", "koolade_dependency"]
    );
}

#[test]
fn oversized_messages_are_rejected_before_the_line_is_fully_read() {
    let config = test_config(super::super::BoundaryAccess::ReadOnly);
    let input = vec![b'x'; MESSAGE_LIMIT + 1];
    let error = serve_stream(
        Path::new("/tmp/mcp/server.json"),
        &config,
        &mut io::Cursor::new(input),
        &mut Vec::new(),
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("size limit"));
}

#[test]
fn actual_read_tool_request_reaches_the_read_only_application_session() {
    let response = run_bash_tool_request(
        super::super::BoundaryAccess::ReadOnly,
        "cat README.md",
        "read result",
    );
    assert_eq!(response["result"]["isError"], false);
    assert_eq!(
        response["result"]["content"][0]["text"],
        "read result\nExit code: 0"
    );
}

#[test]
fn actual_write_tool_request_reaches_the_bounded_implementation_session() {
    let response = run_bash_tool_request(
        super::super::BoundaryAccess::Implementation,
        "printf change >> src/lib.rs",
        "write result",
    );
    assert_eq!(response["result"]["isError"], false);
    assert_eq!(
        response["result"]["content"][0]["text"],
        "write result\nExit code: 0"
    );
}

fn run_bash_tool_request(
    access: super::super::BoundaryAccess,
    command: &str,
    bridge_output: &str,
) -> Value {
    use std::{
        io::{BufRead, Write},
        os::unix::net::UnixListener,
        thread,
    };

    let directory = tempfile::tempdir().unwrap();
    let config_path = directory.path().join("mcp.json");
    let listener = UnixListener::bind(directory.path().join("execution.sock")).unwrap();
    let command = command.to_owned();
    let bridge_command = command.clone();
    let bridge_output = bridge_output.to_owned();
    let bridge = thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let mut reader = std::io::BufReader::new(stream.try_clone().unwrap());
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        let request: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(request["command"], bridge_command);
        assert_eq!(
            request["access"],
            match access {
                super::super::BoundaryAccess::ReadOnly => "read_only",
                super::super::BoundaryAccess::Implementation => "implementation",
                super::super::BoundaryAccess::RuntimeQaScreenshotReview =>
                    "runtime_qa_screenshot_review",
            }
        );
        let mut stream = reader.into_inner();
        serde_json::to_writer(
            &mut stream,
            &json!({
                "stdout": bridge_output,
                "stderr": "",
                "exit_code": 0,
                "is_error": false,
            }),
        )
        .unwrap();
        stream.write_all(b"\n").unwrap();
    });
    let mut config = test_config(access);
    if access == super::super::BoundaryAccess::Implementation {
        config.resource_socket = Some(directory.path().join("resource.sock"));
        config.resource_cache = Some(directory.path().join("cache"));
    }
    let request = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "tools/call",
        "params": {
            "name": "koolade_bash",
            "arguments": { "command": command },
        },
    });
    let mut output = Vec::new();
    serve_stream(
        &config_path,
        &config,
        &mut io::Cursor::new(format!("{request}\n")),
        &mut output,
    )
    .unwrap();
    bridge.join().unwrap();
    serde_json::from_slice(output.split(|byte| *byte == b'\n').next().unwrap()).unwrap()
}

fn test_config(access: super::super::BoundaryAccess) -> ServerConfig {
    ServerConfig {
        protocol_version: 2,
        access,
        session_id: "00000000-0000-4000-8000-000000000001".into(),
        task_id: "00000000-0000-4000-8000-000000000002".into(),
        workspace_id: format!(
            "00000000-0000-4000-8000-000000000003:00000000-0000-4000-8000-000000000002:{}",
            "a".repeat(40)
        ),
        capability_id: "00000000-0000-4000-8000-000000000004".into(),
        target_id: Some("local".into()),
        target_arch: Some("amd64".into()),
        resource_socket: None,
        resource_cache: None,
        screenshot_access: None,
    }
}
