use super::*;
use crate::harness::{AiHarness, ExecutionMode, LiveProgress, PlanningRequest};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, mpsc};
use std::time::Duration;

#[test]
fn parses_claude_stream_events_and_usage() {
    assert_eq!(
        parse_event(r#"{"type":"system","subtype":"init","model":"sonnet-5"}"#),
        Some(ClaudeEvent::Usage {
            input: None,
            output: None,
            cost: None,
            model: Some("sonnet-5".into())
        })
    );
    assert_eq!(
        parse_event(
            r#"{"type":"assistant","message":{"content":[{"type":"tool_use","name":"Read"}]}}"#
        ),
        Some(ClaudeEvent::Tool("Read".into()))
    );
    assert_eq!(
        parse_event(
            r#"{"type":"result","subtype":"success","result":"done","usage":{"input_tokens":8,"output_tokens":2},"total_cost_usd":0.01}"#
        ),
        Some(ClaudeEvent::Completed {
            text: "done".into(),
            input: Some(8),
            output: Some(2),
            cost: Some(10_000)
        })
    );
    assert_eq!(
        parse_event(r#"{"type":"result","is_error":true,"result":"not authenticated"}"#),
        Some(ClaudeEvent::Failure("not authenticated".into()))
    );
    assert_eq!(parse_event("not json"), None);
}

#[test]
fn structured_prompt_uses_stream_json_input_without_arguments() {
    let value: serde_json::Value =
        serde_json::from_str(prompt_input("private task prompt").trim()).unwrap();
    assert_eq!(
        value["message"]["content"][0]["text"],
        "private task prompt"
    );
    let request = request(std::env::temp_dir(), mpsc::channel().0);
    let args = command(std::path::Path::new("claude"), &request);
    assert!(args.iter().any(|arg| arg == "--restricted"));
    assert!(args.iter().any(|arg| arg == "--no-session-persistence"));
    assert!(args.iter().any(|arg| arg == "--permission-prompts"));
    assert!(args.iter().any(|arg| arg == "Read,Glob,Grep,Edit,Write"));
    assert!(!args.iter().any(|arg| arg == "private task prompt"));
    let read_only = PlanningRequest {
        mode: ExecutionMode::Planning,
        ..request
    };
    let args = command(std::path::Path::new("claude"), &read_only);
    assert!(args.iter().any(|arg| arg == "Read,Glob,Grep"));
}

#[test]
fn version_parser_enforces_supported_version_shape() {
    assert_eq!(parse_version("2.1.268"), Some((2, 1, 268)));
    assert_eq!(parse_version("Claude Code v2.1.300"), Some((2, 1, 300)));
    assert_eq!(parse_version("unknown"), None);
}

#[cfg(unix)]
#[test]
fn probe_distinguishes_ready_unsupported_and_missing_auth() {
    let root = temp_dir("probe");
    let binary = fake_cli(
        &root,
        "if [ \"$1\" = \"--version\" ]; then echo '2.1.300'; else echo '{\"authMethod\":\"claude.ai\"}'; fi\nexit 0",
    );
    assert_eq!(probe(&binary).unwrap(), "2.1.300");
    let old = fake_cli(
        &root,
        "if [ \"$1\" = \"--version\" ]; then echo '2.1.100'; else echo '{}'; fi\nexit 0",
    );
    assert_eq!(probe(&old).unwrap_err().0, ClaudeReadiness::Unsupported);
    let unauth = fake_cli(
        &root,
        "if [ \"$1\" = \"--version\" ]; then echo '2.1.300'; else echo '{\"authMethod\":\"none\"}'; fi\nexit 0",
    );
    assert_eq!(
        probe(&unauth).unwrap_err().0,
        ClaudeReadiness::AuthenticationRequired
    );
    let broken_auth = fake_cli(
        &root,
        "if [ \"$1\" = \"--version\" ]; then echo '2.1.300'; else exit 2; fi\n",
    );
    let failure = probe(&broken_auth).unwrap_err();
    assert_eq!(failure.0, ClaudeReadiness::Unusable);
    assert_eq!(failure.2.as_deref(), Some("2.1.300"));
    let unauth_nonzero = fake_cli(
        &root,
        "if [ \"$1\" = \"--version\" ]; then echo '2.1.300'; else echo '{\"authMethod\":\"none\"}'; exit 1; fi\n",
    );
    let failure = probe(&unauth_nonzero).unwrap_err();
    assert_eq!(failure.0, ClaudeReadiness::AuthenticationRequired);
    assert_eq!(failure.2.as_deref(), Some("2.1.300"));
    let unknown_auth = fake_cli(
        &root,
        "if [ \"$1\" = \"--version\" ]; then echo '2.1.300'; else echo '{\"authMethod\":\"mystery\"}'; fi\nexit 0",
    );
    let failure = probe(&unknown_auth).unwrap_err();
    assert_eq!(failure.0, ClaudeReadiness::Unusable);
    assert_eq!(failure.2.as_deref(), Some("2.1.300"));
    std::fs::remove_dir_all(root).unwrap();
}

#[cfg(unix)]
#[test]
fn executes_normalized_result_and_usage_from_fake_claude() {
    let root = temp_dir("execute");
    let binary = fake_cli(
        &root,
        "cat >/dev/null\nprintf '%s\\n' '{\"type\":\"system\",\"subtype\":\"init\",\"model\":\"sonnet-5\"}' '{\"type\":\"result\",\"subtype\":\"success\",\"is_error\":false,\"result\":\"{\\\"ok\\\":true}\",\"usage\":{\"input_tokens\":12,\"output_tokens\":4},\"total_cost_usd\":0.02}'\nexit 0",
    );
    let _env = EnvOverride::set(&binary);
    let (tx, rx) = mpsc::channel();
    let result = ClaudeHarness.execute(&request(root.clone(), tx)).unwrap();
    assert_eq!(result.final_text, r#"{"ok":true}"#);
    let activity = rx.try_iter().last().unwrap();
    assert_eq!(activity.model_calls.len(), 1);
    assert_eq!(activity.model_calls[0].model.as_deref(), Some("sonnet-5"));
    assert_eq!(
        activity.model_calls[0].provider.as_deref(),
        Some("anthropic")
    );
    assert_eq!(activity.model_calls[0].input_tokens, Some(12));
    assert_eq!(activity.model_calls[0].output_tokens, Some(4));
    assert_eq!(
        activity.model_calls[0].estimated_cost_usd_micros,
        Some(20_000)
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[cfg(unix)]
#[test]
fn malformed_output_is_rejected_and_cancellation_kills_the_child() {
    let root = temp_dir("cancel");
    let binary = fake_cli(&root, "echo malformed\nexit 0");
    let _env = EnvOverride::set(&binary);
    let (tx, _) = mpsc::channel();
    let error = ClaudeHarness
        .execute(&request(root.clone(), tx))
        .unwrap_err();
    assert!(error.detail().contains("without a final result"));

    let ready = root.join("started");
    let binary = fake_cli(&root, &format!("touch '{}'\nsleep 30", ready.display()));
    _env.set_value(&binary);
    let cancel = Arc::new(AtomicBool::new(false));
    let worker_cancel = cancel.clone();
    let worker_root = root.clone();
    let worker = std::thread::spawn(move || {
        ClaudeHarness.execute(&PlanningRequest {
            cancel: worker_cancel,
            ..request(worker_root, mpsc::channel().0)
        })
    });
    let deadline = std::time::Instant::now() + Duration::from_secs(3);
    while !ready.exists() && std::time::Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(ready.exists());
    cancel.store(true, std::sync::atomic::Ordering::Relaxed);
    assert!(
        worker
            .join()
            .unwrap()
            .unwrap_err()
            .detail()
            .contains("cancelled by user")
    );
    std::fs::remove_dir_all(root).unwrap();
}

fn request(root: std::path::PathBuf, progress_tx: mpsc::Sender<LiveProgress>) -> PlanningRequest {
    PlanningRequest {
        mode: ExecutionMode::Implementation,
        reasoning_level: "high".into(),
        telemetry_phase: None,
        repo_root: root,
        runtime_config_source: None,
        prompt_body: "fixture task".into(),
        system_instructions: "fixture policy".into(),
        timeout: Duration::from_secs(5),
        progress_tx,
        cancel: Arc::new(AtomicBool::new(false)),
    }
}

#[cfg(unix)]
fn temp_dir(name: &str) -> std::path::PathBuf {
    let path = std::env::temp_dir().join(format!("koolade-claude-{name}-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&path).unwrap();
    path
}

#[cfg(unix)]
fn fake_cli(root: &std::path::Path, body: &str) -> std::path::PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let path = root.join(format!("claude-{}.sh", uuid::Uuid::new_v4()));
    std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    path
}

#[cfg(unix)]
struct EnvOverride(Option<std::ffi::OsString>);

#[cfg(unix)]
impl EnvOverride {
    fn set(path: &std::path::Path) -> Self {
        let prior = std::env::var_os(CLAUDE_BINARY_ENV);
        unsafe {
            std::env::set_var(CLAUDE_BINARY_ENV, path);
        }
        Self(prior)
    }

    fn set_value(&self, path: &std::path::Path) {
        unsafe {
            std::env::set_var(CLAUDE_BINARY_ENV, path);
        }
    }
}

#[cfg(unix)]
impl Drop for EnvOverride {
    fn drop(&mut self) {
        match self.0.take() {
            Some(value) => unsafe { std::env::set_var(CLAUDE_BINARY_ENV, value) },
            None => unsafe { std::env::remove_var(CLAUDE_BINARY_ENV) },
        }
    }
}
