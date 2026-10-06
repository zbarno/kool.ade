use super::*;
use crate::harness::{AiHarness, PlanningRequest};
use std::path::Path;
use std::sync::Mutex;

static ENV_LOCK: Mutex<()> = Mutex::new(());

#[test]
fn model_catalog_uses_only_slug_tokens() {
    assert_eq!(
        parse_models(
            "gemini-3.8-flash-high Gemini 3.8 Flash\ngemini-3.7-pro-low Gemini Pro\nNo models"
        ),
        vec!["gemini-3.7-pro-low", "gemini-3.8-flash-high"]
    );
}

#[test]
fn auth_configuration_and_permission_denials_need_operator_attention() {
    use super::execute::{
        execution_error, looks_like_denial, provider_attention, redact_unexpected, stderr_tail,
    };
    assert!(
        provider_attention("authentication required")
            .unwrap()
            .contains("authenticate")
    );
    assert!(
        provider_attention("modelProvider is not configured")
            .unwrap()
            .contains("configure")
    );
    assert!(matches!(
        execution_error(None, &["authentication required".into()], &[]),
        crate::error::AppError::Other(_)
    ));
    assert!(looks_like_denial(
        "Tool was soft-denied because approval was unavailable"
    ));
    assert!(!looks_like_denial("normal progress message"));
    let redacted =
        redact_unexpected("Bearer bearer-secret-value GEMINI_API_KEY=AIzaSuperSecret123");
    assert!(!redacted.contains("bearer-secret-value"));
    assert!(!redacted.contains("AIzaSuperSecret123"));
    assert!(redacted.contains("[REDACTED]"));
    let spaced_labels = redact_unexpected("API key: spaced-secret Access key=another-secret");
    assert!(!spaced_labels.contains("spaced-secret"));
    assert!(!spaced_labels.contains("another-secret"));
    let _lock = ENV_LOCK.lock().unwrap();
    let old = std::env::var_os("KOOLADE_TEST_TOKEN");
    unsafe {
        std::env::set_var("KOOLADE_TEST_TOKEN", "shrt");
    }
    let short_redacted = redact_unexpected("unexpected value shrt");
    let short_stderr = stderr_tail(&["diagnostic shrt".into()]);
    unsafe {
        if let Some(value) = old {
            std::env::set_var("KOOLADE_TEST_TOKEN", value);
        } else {
            std::env::remove_var("KOOLADE_TEST_TOKEN");
        }
    }
    assert!(!short_redacted.contains("shrt"));
    assert!(!short_stderr.contains("shrt"));
}

#[cfg(unix)]
#[test]
fn readiness_discovers_version_and_models_without_auth_probe() {
    let root = std::env::temp_dir().join(format!("agy-probe-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&root);
    let cli = root.join("agy");
    std::fs::write(&cli, "#!/bin/sh\nif [ \"$1\" = \"--version\" ]; then echo 'agy 1.2.3'; else echo 'gemini-3.8-flash-high Gemini Flash'; fi\n").unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&cli, std::fs::Permissions::from_mode(0o755)).unwrap();
    let (version, models) = probe(&cli).unwrap();
    assert_eq!(version, "agy 1.2.3");
    assert_eq!(models, vec!["gemini-3.8-flash-high"]);
    std::fs::remove_dir_all(root).unwrap();
}

#[cfg(unix)]
#[test]
fn executes_stdin_prompt_and_normalizes_usage_result() {
    use crate::harness::{ExecutionMode, PlanningRequest};
    use std::sync::{Arc, atomic::AtomicBool, mpsc};
    use std::time::Duration;
    let root = std::env::temp_dir().join(format!("agy-execute-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&root);
    let cli = root.join("agy");
    std::fs::write(&cli, "#!/bin/sh\ncat >/dev/null\nprintf '%s\\n' '{\"event\":\"result\",\"result\":{\"status\":\"SUCCESS\",\"response\":\"done\",\"usage\":{\"input_tokens\":11,\"output_tokens\":3,\"thinking_tokens\":2,\"cache_read_tokens\":4,\"total_tokens\":14}}}'\n").unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&cli, std::fs::Permissions::from_mode(0o755)).unwrap();
    let _env = set_binary(&cli);
    let (tx, rx) = mpsc::channel();
    let request = PlanningRequest {
        mode: ExecutionMode::Implementation,
        reasoning_level: "high".into(),
        telemetry_phase: None,
        repo_root: root.clone(),
        prompt_body: "task secret prompt".into(),
        system_instructions: "persona".into(),
        timeout: Duration::from_secs(5),
        progress_tx: tx,
        cancel: Arc::new(AtomicBool::new(false)),
    };
    let result = AntigravityHarness.execute(&request).unwrap();
    assert_eq!(result.final_text, "done");
    let call = rx.try_iter().flat_map(|p| p.model_calls).last().unwrap();
    assert_eq!(call.input_tokens, Some(11));
    assert_eq!(call.output_tokens, Some(3));
    assert_eq!(call.reasoning_tokens, Some(2));
    assert_eq!(call.cache_read_tokens, Some(4));
    std::fs::remove_dir_all(root).unwrap();
}

#[cfg(unix)]
#[test]
fn auth_errors_and_malformed_output_are_reported() {
    use crate::harness::AiHarness;
    let root = std::env::temp_dir().join(format!("agy-error-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&root);
    let cli = root.join("agy");
    fake_cli(
        &cli,
        "cat >/dev/null\nprintf '%s\\n' '{\"event\":\"result\",\"result\":{\"status\":\"ERROR\",\"error\":\"authentication required\"}}'\nexit 1\n",
    );
    let _env = set_binary(&cli);
    let error = AntigravityHarness
        .execute(&request(&root, std::sync::mpsc::channel().0))
        .unwrap_err();
    assert!(error.detail().to_ascii_lowercase().contains("authenticate"));
    fake_cli(
        &cli,
        "cat >/dev/null\necho 'not-json Bearer secretsupersecret'\nexit 0\n",
    );
    let error = AntigravityHarness
        .execute(&request(&root, std::sync::mpsc::channel().0))
        .unwrap_err();
    assert!(error.detail().contains("before a result"));
    assert!(error.detail().contains("not-json"));
    assert!(!error.detail().contains("secretsupersecret"));
    std::fs::remove_dir_all(root).unwrap();
}

#[cfg(unix)]
#[test]
fn cancellation_stops_the_active_cli_process() {
    use crate::harness::AiHarness;
    use std::sync::{Arc, atomic::AtomicBool};
    use std::time::{Duration, Instant};
    let root = std::env::temp_dir().join(format!("agy-cancel-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&root);
    let ready = root.join("started");
    let cli = root.join("agy");
    fake_cli(
        &cli,
        &format!("cat >/dev/null\ntouch '{}'\nsleep 30\n", ready.display()),
    );
    let _env = set_binary(&cli);
    let cancel = Arc::new(AtomicBool::new(false));
    let worker_cancel = cancel.clone();
    let worker_root = root.clone();
    let worker = std::thread::spawn(move || {
        AntigravityHarness.execute(&PlanningRequest {
            cancel: worker_cancel,
            ..request(&worker_root, std::sync::mpsc::channel().0)
        })
    });
    let deadline = Instant::now() + Duration::from_secs(3);
    while !ready.exists() && Instant::now() < deadline {
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
            .contains("cancelled")
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn stdin_prompt_keeps_task_text_out_of_arguments() {
    use crate::harness::{ExecutionMode, PlanningRequest};
    use std::sync::{Arc, atomic::AtomicBool, mpsc};
    use std::time::Duration;
    let request = PlanningRequest {
        mode: ExecutionMode::Implementation,
        reasoning_level: "medium".into(),
        telemetry_phase: None,
        repo_root: ".".into(),
        prompt_body: "never argv".into(),
        system_instructions: "instructions".into(),
        timeout: Duration::from_secs(1),
        progress_tx: mpsc::channel().0,
        cancel: Arc::new(AtomicBool::new(false)),
    };
    let args = execute::command(Path::new("agy"), &request, Some("gemini-3.8-flash-high"));
    assert!(args.iter().any(|arg| arg == "--sandbox"));
    assert!(
        args.windows(2)
            .any(|pair| pair == ["--model", "gemini-3.8-flash-high"])
    );
    assert!(
        !args
            .iter()
            .any(|arg| arg == "--dangerously-skip-permissions")
    );
    assert!(!args.iter().any(|arg| arg.contains("never argv")));
    let input: serde_json::Value = serde_json::from_str(&execute::prompt_input(&request)).unwrap();
    assert!(
        input["message"]["content"]
            .as_str()
            .unwrap()
            .contains("never argv")
    );
}

#[cfg(unix)]
fn fake_cli(path: &Path, script: &str) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::write(path, format!("#!/bin/sh\n{script}")).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

struct BinaryOverride {
    old: Option<std::ffi::OsString>,
    _guard: std::sync::MutexGuard<'static, ()>,
}
impl Drop for BinaryOverride {
    fn drop(&mut self) {
        unsafe {
            if let Some(value) = self.old.take() {
                std::env::set_var(ANTIGRAVITY_BINARY_ENV, value);
            } else {
                std::env::remove_var(ANTIGRAVITY_BINARY_ENV);
            }
        }
    }
}

fn set_binary(path: &Path) -> BinaryOverride {
    let _lock = ENV_LOCK.lock().unwrap();
    let old = std::env::var_os(ANTIGRAVITY_BINARY_ENV);
    unsafe {
        std::env::set_var(ANTIGRAVITY_BINARY_ENV, path);
    }
    BinaryOverride { old, _guard: _lock }
}

fn request(
    root: &Path,
    progress_tx: std::sync::mpsc::Sender<crate::harness::LiveProgress>,
) -> crate::harness::PlanningRequest {
    use crate::harness::ExecutionMode;
    use std::sync::{Arc, atomic::AtomicBool};
    use std::time::Duration;
    crate::harness::PlanningRequest {
        mode: ExecutionMode::Implementation,
        reasoning_level: "high".into(),
        telemetry_phase: None,
        repo_root: root.to_path_buf(),
        prompt_body: "task secret prompt".into(),
        system_instructions: "persona".into(),
        timeout: Duration::from_secs(5),
        progress_tx,
        cancel: Arc::new(AtomicBool::new(false)),
    }
}
