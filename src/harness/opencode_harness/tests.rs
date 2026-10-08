use super::*;
use crate::harness::{AiHarness, ExecutionMode, LiveProgress, PlanningRequest};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, mpsc};
use std::time::Duration;

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
fn permissions_are_denied_by_default_and_mode_scoped() {
    let no_access: serde_json::Value =
        serde_json::from_str(&permission_policy(crate::harness::ToolAccess::None)).unwrap();
    assert_eq!(no_access["read"], "deny");
    assert_eq!(no_access["edit"], "deny");
    assert_eq!(no_access["bash"], "deny");

    let readonly: serde_json::Value =
        serde_json::from_str(&permission_policy(crate::harness::ToolAccess::ReadOnly)).unwrap();
    assert_eq!(readonly["*"], "deny");
    assert_eq!(readonly["read"], "allow");
    assert_eq!(readonly["edit"], "deny");
    assert_eq!(readonly["bash"], "deny");
    assert_eq!(readonly["task"], "deny");

    let implementation: serde_json::Value = serde_json::from_str(&permission_policy(
        crate::harness::ToolAccess::BoundedImplementation,
    ))
    .unwrap();
    assert_eq!(implementation["edit"], "allow");
    assert_eq!(implementation["bash"]["git *"], "deny");
    assert_eq!(implementation["bash"]["gh *"], "deny");
    assert_eq!(implementation["bash"]["curl *"], "deny");
    assert_eq!(implementation["external_directory"], "deny");
}

#[test]
fn version_parser_accepts_cli_prefix_and_rejects_malformed_versions() {
    assert_eq!(parse_version("1.2.3"), Some("1.2.3".into()));
    assert_eq!(parse_version("opencode 1.2.3"), Some("1.2.3".into()));
    assert_eq!(parse_version("unknown"), None);
}

#[cfg(unix)]
#[test]
fn probe_distinguishes_ready_invalid_and_unconfigured() {
    let root = temp_dir("probe");
    let ready = fake_cli(
        &root,
        "if [ \"$1\" = \"--version\" ]; then echo '1.2.3'; elif [ \"$2\" = \"models\" ]; then echo 'openai/gpt-test'; fi\nexit 0",
    );
    assert_eq!(
        probe(&ready).unwrap(),
        ("1.2.3".into(), vec!["openai/gpt-test".into()])
    );
    let empty = fake_cli(
        &root,
        "if [ \"$1\" = \"--version\" ]; then echo '1.2.3'; fi\nexit 0",
    );
    assert_eq!(
        probe(&empty).unwrap_err().0,
        OpenCodeReadiness::ConfigurationRequired
    );
    let invalid = fake_cli(&root, "echo invalid; exit 0");
    assert_eq!(
        probe(&invalid).unwrap_err().0,
        OpenCodeReadiness::InvalidInstallation
    );
    let auth = fake_cli(
        &root,
        "if [ \"$1\" = \"--version\" ]; then echo '1.2.3'; exit 0; fi; echo 'Authentication required' >&2; exit 1",
    );
    assert_eq!(
        probe(&auth).unwrap_err().0,
        OpenCodeReadiness::AuthenticationRequired
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[cfg(unix)]
#[test]
fn executes_structured_result_with_model_and_telemetry() {
    let root = temp_dir("execute");
    let binary = fake_cli(
        &root,
        &format!(
            "if [ \"$1\" = \"--pure\" ]; then cat > '{}'; test -s '{}' || exit 10; echo \"$OPENCODE_PERMISSION\" | grep -F '\"git *\":\"deny\"' >/dev/null || exit 11; case \" $* \" in *\"--model openai/gpt-test\"*) ;; *) exit 12 ;; esac; printf '%s\\n' '{{\"type\":\"tool_use\",\"part\":{{\"tool\":\"read\"}}}}' '{{\"type\":\"text\",\"part\":{{\"text\":\"{{\\\"ok\\\":true}}\"}}}}' '{{\"type\":\"step_finish\",\"part\":{{\"reason\":\"stop\",\"cost\":0.02,\"tokens\":{{\"input\":12,\"output\":4}}}}}}'; exit 0; fi\nif [ \"$1\" = \"--version\" ]; then echo '1.2.3'; exit 0; fi\nif [ \"$1\" = \"--pure\" ] && [ \"$2\" = \"models\" ]; then echo 'openai/gpt-test'; exit 0; fi\nexit 1",
            root.join("prompt.txt").display(),
            root.join("prompt.txt").display()
        ),
    );
    let _env = EnvOverride::set(&binary);
    let (tx, rx) = mpsc::channel();
    let result = OpenCodeHarness
        .execute_with_model(&request(root.clone(), tx), Some("openai/gpt-test"))
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
fn malformed_output_fails_and_cancellation_stops_the_child() {
    let root = temp_dir("cancel");
    let binary = fake_cli(&root, "cat >/dev/null; echo malformed; exit 0");
    let _env = EnvOverride::set(&binary);
    let (tx, _) = mpsc::channel();
    assert!(
        OpenCodeHarness
            .execute(&request(root.clone(), tx))
            .unwrap_err()
            .detail()
            .contains("completed final response")
    );

    let ready = root.join("started");
    let binary = fake_cli(&root, &format!("touch '{}'; sleep 30", ready.display()));
    _env.set_value(&binary);
    let cancel = Arc::new(AtomicBool::new(false));
    let worker_cancel = cancel.clone();
    let worker_root = root.clone();
    let worker = std::thread::spawn(move || {
        OpenCodeHarness.execute(&PlanningRequest {
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
        task_id: None,
        reasoning_level: "high".into(),
        telemetry_phase: None,
        repo_root: root,
        prompt_body: "fixture task".into(),
        system_instructions: "fixture policy".into(),
        timeout: Duration::from_secs(5),
        progress_tx,
        cancel: Arc::new(AtomicBool::new(false)),
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
struct EnvOverride(Option<std::ffi::OsString>);

#[cfg(unix)]
impl EnvOverride {
    fn set(path: &std::path::Path) -> Self {
        let prior = std::env::var_os(OPENCODE_BINARY_ENV);
        unsafe { std::env::set_var(OPENCODE_BINARY_ENV, path) };
        Self(prior)
    }

    fn set_value(&self, path: &std::path::Path) {
        unsafe { std::env::set_var(OPENCODE_BINARY_ENV, path) };
    }
}

#[cfg(unix)]
impl Drop for EnvOverride {
    fn drop(&mut self) {
        match self.0.take() {
            Some(value) => unsafe { std::env::set_var(OPENCODE_BINARY_ENV, value) },
            None => unsafe { std::env::remove_var(OPENCODE_BINARY_ENV) },
        }
    }
}
