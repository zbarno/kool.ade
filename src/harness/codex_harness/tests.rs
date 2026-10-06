use super::*;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, mpsc};
use std::time::Duration;

use crate::harness::{AiHarness, ExecutionMode, LiveProgress, PlanningRequest};

#[test]
fn parses_completed_message_activity_and_usage_events() {
    assert_eq!(
        parse_event(r#"{"type":"item.completed","item":{"type":"agent_message","text":"final"}}"#),
        Some(CodexEvent::Message("final".into()))
    );
    assert_eq!(
        parse_event(r#"{"type":"item.started","item":{"type":"command_execution"}}"#),
        Some(CodexEvent::Activity("command_execution".into()))
    );
    assert_eq!(
        parse_event(r#"{"type":"turn.completed","usage":{"input_tokens":12,"output_tokens":7}}"#),
        Some(CodexEvent::Usage {
            call_id: None,
            input: Some(12),
            output: Some(7),
            cached_input: None,
            cache_write_input: None,
        })
    );
}

#[test]
fn live_progress_retains_harness_usage_metadata() {
    let mut progress = LiveProgress::default();
    progress.update(LiveProgress {
        model_calls: vec![crate::harness::ModelCallUsage {
            call_id: "codex:test".into(),
            provider: Some("openai".into()),
            model: Some("codex-model".into()),
            input_tokens: Some(11),
            output_tokens: Some(5),
            ..Default::default()
        }],
        ..LiveProgress::default()
    });
    assert_eq!(progress.model_calls[0].input_tokens, Some(11));
    assert_eq!(progress.model_calls[0].output_tokens, Some(5));
    assert_eq!(
        progress.model_calls[0].model.as_deref(),
        Some("codex-model")
    );
}

#[test]
fn command_uses_model_reasoning_and_operation_scoped_sandbox() {
    let (tx, _rx) = mpsc::channel();
    let request = PlanningRequest {
        mode: ExecutionMode::Implementation,
        reasoning_level: "high".into(),
        telemetry_phase: None,
        repo_root: std::env::temp_dir(),
        prompt_body: "do task".into(),
        system_instructions: "system".into(),
        timeout: Duration::from_secs(2),
        progress_tx: tx,
        cancel: Arc::new(AtomicBool::new(false)),
    };
    let (argv, prompt) = command(Path::new("codex"), &request, Some("selected-model".into()));
    assert!(
        argv.windows(2)
            .any(|pair| pair == ["--model", "selected-model"])
    );
    assert!(
        argv.windows(2)
            .any(|pair| pair == ["--sandbox", "workspace-write"])
    );
    assert!(
        argv.iter()
            .any(|value| value == "model_reasoning_effort=\"high\"")
    );
    assert_eq!(prompt, "do task");
    assert!(
        argv.iter()
            .any(|value| value.contains("developer_instructions="))
    );
    assert!(argv.iter().any(|value| value == "--ephemeral"));

    let read_only = PlanningRequest {
        mode: ExecutionMode::Planning,
        ..request
    };
    let (argv, _) = command(Path::new("codex"), &read_only, None);
    assert!(
        argv.windows(2)
            .any(|pair| pair == ["--sandbox", "read-only"])
    );
}

#[test]
fn malformed_events_are_ignored_and_failures_are_normalized() {
    assert_eq!(parse_event("not json"), None);
    assert_eq!(parse_event("{}"), None);
    assert_eq!(
        parse_event(r#"{"type":"turn.failed","error":{"message":"login required"}}"#),
        Some(CodexEvent::Failure("login required".into()))
    );
    assert_eq!(normalize_effort("XHIGH"), "xhigh");
    assert_eq!(normalize_effort("unsupported"), "medium");
}

#[cfg(unix)]
#[test]
fn executable_override_is_validated_without_leaking_override_value() {
    use std::os::unix::fs::PermissionsExt;
    let root = std::env::temp_dir().join(format!("koolade-codex-probe-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let binary = root.join("codex");
    std::fs::write(&binary, "#!/bin/sh\nexit 0\n").unwrap();
    std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert!(executable(&binary));
    assert!(!executable(&root));
    std::fs::remove_dir_all(root).unwrap();
}

#[cfg(unix)]
#[test]
fn installed_codex_without_authentication_is_reported_as_setup_required() {
    let root = std::env::temp_dir().join(format!("koolade-codex-auth-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let binary = root.join("codex");
    std::fs::write(
        &binary,
        "#!/bin/sh\nif [ \"$1\" = \"--version\" ]; then echo 'codex-cli 1.2.3'; else echo 'Not logged in'; fi\n",
    ).unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o755)).unwrap();
    let error = CodexHarness::check_binary(&binary).unwrap_err();
    assert!(error.detail().contains("codex login"));
    std::fs::remove_dir_all(root).unwrap();
}

#[cfg(unix)]
#[test]
fn executes_structured_events_and_normalizes_usage() {
    let root = std::env::temp_dir().join(format!("koolade-codex-run-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let binary = fake_cli(
        &root,
        "printf '%s\\n' '{\"type\":\"item.completed\",\"item\":{\"type\":\"agent_message\",\"text\":\"{\\\"ok\\\":true}\"}}' '{\"type\":\"turn.completed\",\"turn_id\":\"turn-1\",\"usage\":{\"input_tokens\":3,\"output_tokens\":4,\"cached_input_tokens\":2,\"cache_write_input_tokens\":1}}'\nexit 0",
    );
    let _env = EnvOverride::set(CODEX_BINARY_ENV, &binary);
    let (tx, rx) = mpsc::channel();
    let outcome = CodexHarness.execute(&request(root.clone(), tx)).unwrap();
    assert_eq!(outcome.final_text, r#"{"ok":true}"#);
    let progress = rx.try_iter().last().unwrap();
    assert_eq!(progress.model_calls.len(), 1);
    assert_eq!(progress.model_calls[0].call_id, "codex:turn-1");
    assert_eq!(progress.model_calls[0].provider.as_deref(), Some("openai"));
    assert_eq!(progress.model_calls[0].model, None);
    assert_eq!(
        progress.model_calls[0].api.as_deref(),
        Some("codex-cli-turn-aggregate")
    );
    assert_eq!(progress.model_calls[0].input_tokens, Some(3));
    assert_eq!(progress.model_calls[0].output_tokens, Some(4));
    assert_eq!(progress.model_calls[0].cache_read_tokens, Some(2));
    assert_eq!(progress.model_calls[0].cache_write_tokens, Some(1));
    std::fs::remove_dir_all(root).unwrap();
}

#[cfg(unix)]
#[test]
fn failure_and_malformed_completion_are_reported_as_harness_failures() {
    let root = std::env::temp_dir().join(format!("koolade-codex-fail-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let binary = fake_cli(
        &root,
        "printf '%s\\n' '{\"type\":\"turn.failed\",\"error\":{\"message\":\"approval required\"}}'\nexit 0",
    );
    let _env = EnvOverride::set(CODEX_BINARY_ENV, &binary);
    let (tx, _) = mpsc::channel();
    let error = CodexHarness
        .execute(&request(root.clone(), tx))
        .unwrap_err();
    assert!(error.detail().contains("approval required"));

    let binary = fake_cli(&root, "echo not-json\nexit 0");
    _env.set_value(&binary);
    let (tx, _) = mpsc::channel();
    let error = CodexHarness
        .execute(&request(root.clone(), tx))
        .unwrap_err();
    assert!(error.detail().contains("without a final assistant message"));
    std::fs::remove_dir_all(root).unwrap();
}

#[cfg(unix)]
#[test]
fn cancellation_terminates_the_active_codex_process() {
    let root = std::env::temp_dir().join(format!("koolade-codex-cancel-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let ready = root.join("started");
    let binary = fake_cli(&root, &format!("touch '{}'\nsleep 30", ready.display()));
    let _env = EnvOverride::set(CODEX_BINARY_ENV, &binary);
    let cancel = Arc::new(AtomicBool::new(false));
    let worker_cancel = cancel.clone();
    let worker_root = root.clone();
    let worker = std::thread::spawn(move || {
        let (tx, _) = mpsc::channel();
        CodexHarness.execute(&PlanningRequest {
            cancel: worker_cancel,
            ..request(worker_root, tx)
        })
    });
    let deadline = std::time::Instant::now() + Duration::from_secs(3);
    while !ready.exists() && std::time::Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(ready.exists(), "fake Codex process did not start");
    cancel.store(true, std::sync::atomic::Ordering::Relaxed);
    let error = worker.join().unwrap().unwrap_err();
    assert!(error.detail().contains("cancelled by user"));
    std::fs::remove_dir_all(root).unwrap();
}

#[cfg(unix)]
fn request(root: std::path::PathBuf, progress_tx: mpsc::Sender<LiveProgress>) -> PlanningRequest {
    PlanningRequest {
        mode: ExecutionMode::Implementation,
        reasoning_level: "medium".into(),
        telemetry_phase: None,
        repo_root: root,
        prompt_body: "fixture task".into(),
        system_instructions: "fixture instructions".into(),
        timeout: Duration::from_secs(5),
        progress_tx,
        cancel: Arc::new(AtomicBool::new(false)),
    }
}

#[cfg(unix)]
fn fake_cli(root: &std::path::Path, body: &str) -> std::path::PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let path = root.join(format!("codex-{}.sh", uuid::Uuid::new_v4()));
    std::fs::write(&path, format!("#!/bin/sh\ncat >/dev/null\n{body}\n")).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    path
}

#[cfg(unix)]
struct EnvOverride {
    name: &'static str,
    prior: Option<std::ffi::OsString>,
}

#[cfg(unix)]
impl EnvOverride {
    fn set(name: &'static str, value: &std::path::Path) -> Self {
        let prior = std::env::var_os(name);
        unsafe {
            std::env::set_var(name, value);
        }
        Self { name, prior }
    }

    fn set_value(&self, value: &std::path::Path) {
        unsafe {
            std::env::set_var(self.name, value);
        }
    }
}

#[cfg(unix)]
impl Drop for EnvOverride {
    fn drop(&mut self) {
        match self.prior.take() {
            Some(value) => unsafe { std::env::set_var(self.name, value) },
            None => unsafe { std::env::remove_var(self.name) },
        }
    }
}
