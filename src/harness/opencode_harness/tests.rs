use super::readiness::{parse_models, parse_version};
use super::*;
use crate::harness::{AiHarness, ExecutionMode, LiveProgress, PlanningRequest};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, mpsc};
use std::time::Duration;

#[path = "tests/catalog_failures.rs"]
mod catalog_failures;

#[test]
fn parses_json_events_and_usage() {
    assert_eq!(
        parse_event(r#"{"type":"text","part":{"text":"done"}}"#),
        Some(OpenCodeEvent::Text("done".into()))
    );
    assert_eq!(
        parse_event(r#"{"type":"tool_use","part":{"tool":"read"}}"#),
        Some(OpenCodeEvent::Tool("read".into()))
    );
    assert_eq!(
        parse_event(
            r#"{"type":"step_finish","sessionID":"s1","part":{"messageID":"m1","providerID":"openai","modelID":"gpt-test","reason":"stop","cost":0.01,"tokens":{"input":8,"output":2}}}"#
        ),
        Some(OpenCodeEvent::StepFinished {
            call_id: Some("opencode:s1:m1".into()),
            provider: Some("openai".into()),
            model: Some("gpt-test".into()),
            input: Some(8),
            output: Some(2),
            cost_microusd: Some(10_000),
            stop_reason: Some("stop".into()),
        })
    );
    assert!(matches!(
        parse_event(r#"{"type":"error","error":{"data":{"message":"auth required"}}}"#),
        Some(OpenCodeEvent::Failure(message)) if message == "auth required"
    ));
    assert_eq!(parse_event("invalid json"), None);
}

#[test]
fn version_parser_accepts_cli_prefix_and_rejects_malformed_versions() {
    assert_eq!(parse_version("1.2.3"), Some("1.2.3".into()));
    assert_eq!(parse_version("opencode 1.2.3"), Some("1.2.3".into()));
    assert_eq!(parse_version("unknown"), None);
}

#[test]
fn model_catalog_keeps_only_identifiers_and_drops_embedded_cli_output() {
    assert_eq!(
        parse_models("openai/gpt-5 token=synthetic-secret\nollama/llama3.2"),
        ["ollama/llama3.2"]
    );
}

#[cfg(unix)]
#[test]
fn probe_distinguishes_ready_invalid_and_unconfigured() {
    let root = temp_dir("probe");
    let ready = fake_cli(
        &root,
        "if [ \"$1\" = \"--version\" ]; then echo '1.2.3'; elif [ \"$2\" = \"models\" ]; then echo 'openai/gpt-test'; fi\nexit 0",
    );
    let ready_report = probe(&ready, Duration::from_secs(1)).unwrap();
    assert_eq!(ready_report.version, "1.2.3");
    assert_eq!(ready_report.models, vec!["openai/gpt-test"]);
    assert_eq!(ready_report.readiness, OpenCodeReadiness::Ready);
    let empty = fake_cli(
        &root,
        "if [ \"$1\" = \"--version\" ]; then echo '1.2.3'; fi\nexit 0",
    );
    let unconfigured = probe(&empty, Duration::from_secs(1)).unwrap();
    assert_eq!(
        unconfigured.readiness,
        OpenCodeReadiness::ConfigurationRequired
    );
    assert_eq!(unconfigured.version, "1.2.3");
    assert!(unconfigured.diagnostic.contains("opencode auth login"));
    assert!(unconfigured.diagnostic.contains("set up a local model"));
    let invalid = fake_cli(&root, "echo invalid; exit 0");
    assert_eq!(
        probe(&invalid, Duration::from_secs(1)).unwrap_err().0,
        OpenCodeReadiness::InvalidInstallation
    );
    let auth = fake_cli(
        &root,
        "if [ \"$1\" = \"--version\" ]; then echo '1.2.3'; exit 0; fi; echo 'Authentication required' >&2; exit 1",
    );
    let report = probe(&auth, Duration::from_secs(1)).unwrap();
    assert_eq!(report.readiness, OpenCodeReadiness::CatalogUnavailable);
    assert!(report.diagnostic.contains("model discovery failed"));
    assert!(!report.diagnostic.contains("Authentication required"));
    std::fs::remove_dir_all(root).unwrap();
}

#[cfg(unix)]
#[test]
fn executes_structured_result_with_model_and_telemetry() {
    let root = temp_dir("execute");
    let binary = fake_execution_cli(
        &root,
        &format!(
            "if [ \"$1\" = \"--pure\" ]; then cat > '{}'; test -s '{}' || exit 10; echo \"$OPENCODE_CONFIG_CONTENT\" | grep -F '\"*\":\"deny\"' >/dev/null; if [ $? -ne 0 ]; then echo missing-config-deny >&2; exit 11; fi; printf '%s\\n' \"$*\" | grep -F -- '--agent koolade_' >/dev/null; if [ $? -ne 0 ]; then echo missing-isolated-agent >&2; exit 12; fi; printf '%s\\n' \"$*\" | grep -F -- '--model openai/gpt-test' >/dev/null; if [ $? -ne 0 ]; then echo missing-model >&2; exit 13; fi; printf '%s\\n' '{{\"type\":\"tool_use\",\"part\":{{\"tool\":\"read\"}}}}' '{{\"type\":\"text\",\"part\":{{\"text\":\"{{\\\"ok\\\":true}}\"}}}}' '{{\"type\":\"step_finish\",\"part\":{{\"reason\":\"stop\",\"cost\":0.02,\"tokens\":{{\"input\":12,\"output\":4}}}}}}'; exit 0; fi\nif [ \"$1\" = \"--version\" ]; then echo '1.2.3'; exit 0; fi\nif [ \"$1\" = \"--pure\" ] && [ \"$2\" = \"models\" ]; then echo 'openai/gpt-test'; exit 0; fi\nexit 1",
            root.join("prompt.txt").display(),
            root.join("prompt.txt").display()
        ),
    );
    let _env = EnvOverride::set(&binary);
    let (tx, rx) = mpsc::channel();
    let result = crate::harness::with_uncontained_provider_test_execution(|| {
        OpenCodeHarness.execute_with_model(&request(root.clone(), tx), Some("openai/gpt-test"))
    })
    .unwrap();
    assert_eq!(result.final_text, r#"{"ok":true}"#);
    let activity = rx.try_iter().last().unwrap();
    assert_eq!(activity.model_calls.len(), 1);
    assert_eq!(activity.model_calls[0].provider.as_deref(), Some("openai"));
    assert_eq!(activity.model_calls[0].model.as_deref(), Some("gpt-test"));
    assert_eq!(
        activity.model_calls[0].requested_model.as_deref(),
        Some("openai/gpt-test")
    );
    assert_eq!(activity.model_calls[0].input_tokens, Some(12));
    assert_eq!(activity.model_calls[0].output_tokens, Some(4));
    assert_eq!(
        activity.model_calls[0].estimated_cost_usd_micros,
        Some(20_000)
    );
    assert_eq!(activity.activity.as_deref(), Some("read"));
    let prompt = std::fs::read_to_string(root.join("prompt.txt")).unwrap();
    assert!(prompt.contains("fixture policy"));
    assert!(prompt.contains("fixture task"));
    std::fs::remove_dir_all(root).unwrap();
}

#[cfg(unix)]
#[test]
fn sequential_steps_use_non_overlapping_model_call_intervals() {
    let root = temp_dir("step-timing");
    let binary = fake_execution_cli(
        &root,
        r#"if [ "$1" = "--pure" ]; then
cat >/dev/null
sleep 0.08
printf '%s\n' '{"type":"text","part":{"text":"done"}}' '{"type":"step_finish","sessionID":"s","part":{"messageID":"m1","providerID":"openai","modelID":"gpt-test","tokens":{"input":3,"output":2}}}'
sleep 0.08
printf '%s\n' '{"type":"step_finish","sessionID":"s","part":{"messageID":"m2","providerID":"openai","modelID":"gpt-test","tokens":{"input":4,"output":2}}}'
exit 0
fi
exit 1"#,
    );
    let _env = EnvOverride::set(&binary);
    let (tx, rx) = mpsc::channel();
    crate::harness::with_uncontained_provider_test_execution(|| {
        OpenCodeHarness.execute(&request(root.clone(), tx))
    })
    .unwrap();
    let updates = rx.try_iter().last().unwrap();
    let calls = updates.model_calls;
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0].call_id, "opencode:s:m1");
    assert_eq!(calls[1].call_id, "opencode:s:m2");
    assert_eq!(calls[0].ended_at, calls[1].started_at);
    assert_eq!(calls[0].total_tokens, Some(5));
    assert_eq!(calls[1].total_tokens, Some(6));
    assert!(calls.iter().all(|call| call.duration_millis.is_some()));
    std::fs::remove_dir_all(root).unwrap();
}

#[cfg(unix)]
#[test]
fn malformed_output_fails_and_cancellation_stops_the_child() {
    let root = temp_dir("cancel");
    let binary = fake_execution_cli(&root, "cat >/dev/null; echo malformed; exit 0");
    let mut _env = EnvOverride::set(&binary);
    let (tx, _) = mpsc::channel();
    assert!(crate::harness::with_uncontained_provider_test_execution(
        || {
            OpenCodeHarness
                .execute(&request(root.clone(), tx))
                .unwrap_err()
                .detail()
                .contains("completed final response")
        }
    ));

    let ready = root.join("started");
    let binary = fake_execution_cli(&root, &format!("touch '{}'; sleep 30", ready.display()));
    _env.set_value(&binary);
    let cancel = Arc::new(AtomicBool::new(false));
    let worker_cancel = cancel.clone();
    let worker_root = root.clone();
    let worker = std::thread::spawn(move || {
        crate::harness::with_uncontained_provider_test_execution(|| {
            OpenCodeHarness.execute(&PlanningRequest {
                prompt_manifest: None,
                cancel: worker_cancel,
                ..request(worker_root, mpsc::channel().0)
            })
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
        prompt_manifest: None,
        mode: ExecutionMode::Implementation,
        task_id: None,
        reasoning_level: "high".into(),
        telemetry_phase: None,
        repo_root: root,
        runtime_config_source: None,
        prompt_body: "fixture task".into(),
        system_instructions: "fixture policy".into(),
        timeout: Duration::from_secs(5),
        progress_tx,
        cancel: Arc::new(AtomicBool::new(false)),
        execution_workspace: None,

        authorization_verifier: None,
        review_identity: None,
    }
}

#[cfg(unix)]
fn temp_dir(name: &str) -> std::path::PathBuf {
    let path =
        std::env::temp_dir().join(format!("koolade-opencode-{name}-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&path).unwrap();
    path
}

#[cfg(unix)]
fn fake_cli(root: &std::path::Path, body: &str) -> std::path::PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let path = root.join(format!("opencode-{}.sh", uuid::Uuid::new_v4()));
    std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    path
}

#[cfg(unix)]
fn fake_execution_cli(root: &std::path::Path, body: &str) -> std::path::PathBuf {
    fake_cli(
        root,
        &format!(
            "if [ \"$1\" = \"--version\" ]; then echo '1.2.3'; exit 0; fi\nif [ \"$1\" = \"--pure\" ] && [ \"$2\" = \"models\" ]; then echo 'openai/gpt-test'; exit 0; fi\n{body}"
        ),
    )
}

#[cfg(unix)]
struct EnvOverride {
    environment: crate::test_support::ProcessEnvironmentGuard,
}

#[cfg(unix)]
impl EnvOverride {
    fn set(path: &std::path::Path) -> Self {
        let mut environment = crate::test_support::ProcessEnvironmentGuard::new();
        environment.set(OPENCODE_BINARY_ENV, path.as_os_str());
        Self { environment }
    }

    fn set_value(&mut self, path: &std::path::Path) {
        self.environment.set(OPENCODE_BINARY_ENV, path.as_os_str());
    }
}
