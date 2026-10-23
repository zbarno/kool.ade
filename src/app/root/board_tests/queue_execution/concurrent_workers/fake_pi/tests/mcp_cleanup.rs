use std::{
    io::Write,
    os::unix::fs::PermissionsExt,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

#[test]
fn provider_drains_mcp_output_and_waits_for_child_before_returning() {
    // Boundary: fake provider -> app-owned MCP child process. Invariant: the
    // provider must drain child output and reap it before a queue retry can need
    // the task execution state. A Rust unit cannot prove child exit. This uses
    // temporary scripts and config only; no Docker or vendor API, with low cost.
    let root = tempfile::tempdir().unwrap();
    let provider = super::super::create(root.path());
    let config = root.path().join("mcp-config.json");
    std::fs::write(
        &config,
        r#"{"access":"implementation","task_id":"00000000-0000-4000-8000-000000000001"}"#,
    )
    .unwrap();
    let exit_marker = root.path().join("mcp-exit-observed");
    let release_marker = root.path().join("release-mcp-server");
    let pid_file = root.path().join("mcp-server.pid");
    let server = root.path().join("mcp-server.py");
    std::fs::write(
        &server,
        r##"#!/usr/bin/python3
import json, os, sys, time
from pathlib import Path
Path(os.environ['MCP_PID_FILE']).write_text(str(os.getpid()))
for line in sys.stdin:
    request = json.loads(line)
    method = request.get('method')
    if method == 'exit':
        Path(os.environ['MCP_EXIT_MARKER']).write_text('received')
        sys.stdout.write('x' * (1024 * 1024))
        sys.stdout.flush()
        release = Path(os.environ['MCP_RELEASE_MARKER'])
        deadline = time.monotonic() + 10
        while not release.exists() and time.monotonic() < deadline:
            time.sleep(0.01)
        break
    if 'id' in request:
        result = {'content': [], 'isError': False}
        if method == 'tools/call':
            result = {'content': [{'type': 'text', 'text': 'write cwd=/workspace\nwrote=/workspace/src/task-1.rs\nExit code: 0'}], 'isError': False}
        print(json.dumps({'jsonrpc': '2.0', 'id': request['id'], 'result': result}), flush=True)
"##,
    )
    .unwrap();
    std::fs::set_permissions(&server, std::fs::Permissions::from_mode(0o700)).unwrap();

    let prompt = "TICKET PATH: 001-task.md\nTICKET CONTENT:\n## Affected files and components\n\n- src/task-1.rs\n\n## Acceptance Criteria\n\n- File exists.\n\nAPPROVED SPECIFICATION:\nSynthetic fixture specification.";
    let mut child = Command::new(provider)
        .arg("--tools")
        .arg("koolade_bash")
        .env("KOOLADE_MCP_COMMAND", &server)
        .env("KOOLADE_MCP_ARGS", serde_json::json!([config]).to_string())
        .env("MCP_EXIT_MARKER", &exit_marker)
        .env("MCP_RELEASE_MARKER", &release_marker)
        .env("MCP_PID_FILE", &pid_file)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(prompt.as_bytes())
        .unwrap();

    let exit_deadline = Instant::now() + Duration::from_secs(5);
    while !exit_marker.is_file() && Instant::now() < exit_deadline {
        if child.try_wait().unwrap().is_some() {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let cleanup_requested_exit = exit_marker.is_file();
    // The old provider called wait(timeout=3) without draining stdout. Keep the
    // child alive beyond that deadline and flood its pipe to exercise both gaps.
    let verification_deadline = Instant::now() + Duration::from_millis(3500);
    let mut provider_exited_before_release = false;
    while Instant::now() < verification_deadline {
        if child.try_wait().unwrap().is_some() {
            provider_exited_before_release = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    std::fs::write(&release_marker, "release").unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(cleanup_requested_exit, "MCP child did not receive exit");
    assert!(
        !provider_exited_before_release,
        "provider returned before its MCP child exited"
    );

    #[cfg(target_os = "linux")]
    {
        let pid = std::fs::read_to_string(pid_file).unwrap();
        let process = std::path::PathBuf::from("/proc").join(pid.trim());
        let deadline = Instant::now() + Duration::from_secs(2);
        while process.exists() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(!process.exists(), "MCP child process was not reaped");
    }
}
