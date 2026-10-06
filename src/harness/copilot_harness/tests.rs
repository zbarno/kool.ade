use super::execute::{
    UsageFile, needs_attention, normalize_output, parse_usage, supports_usage_output,
};
use super::*;
use crate::harness::{ExecutionMode, PlanningRequest};
use std::{
    sync::{Arc, atomic::AtomicBool, mpsc},
    time::Duration,
};

fn request(mode: ExecutionMode) -> PlanningRequest {
    PlanningRequest {
        mode,
        reasoning_level: "medium".into(),
        telemetry_phase: None,
        repo_root: std::env::temp_dir(),
        prompt_body: "task prompt".into(),
        system_instructions: "system context".into(),
        timeout: Duration::from_secs(2),
        progress_tx: mpsc::channel().0,
        cancel: Arc::new(AtomicBool::new(false)),
    }
}

#[test]
fn command_routes_model_and_respects_tool_access() {
    let args = super::execute::command(
        Path::new("copilot"),
        &request(ExecutionMode::Implementation),
        Some("claude-sonnet-4.5"),
        Some(Path::new("usage.json")),
    );
    assert!(
        args.windows(2)
            .any(|pair| pair == ["--model", "claude-sonnet-4.5"])
    );
    assert!(args.iter().any(|arg| arg == "--available-tools=read,edit"));
    assert!(args.iter().any(|arg| arg == "--allow-tool=write"));
    assert!(!args.iter().any(|arg| arg == "shell"));
    let read = super::execute::command(
        Path::new("copilot"),
        &request(ExecutionMode::Planning),
        None,
        None,
    );
    assert!(read.iter().any(|arg| arg == "--available-tools=read"));
    let no_tools = super::execute::command(
        Path::new("copilot"),
        &request(ExecutionMode::Reconciliation),
        None,
        None,
    );
    assert!(no_tools.iter().any(|arg| arg == "--available-tools="));
    assert!(args.iter().any(|arg| arg == "--disable-builtin-mcps"));
    assert!(
        args.windows(2)
            .any(|pair| pair == ["--usage-output-file", "usage.json"])
    );
}

#[test]
fn usage_metadata_is_normalized_when_present_and_ignored_when_malformed() {
    let value = serde_json::json!({
        "model": "gpt-5.4",
        "usage": { "inputTokens": 17, "outputTokens": 8, "totalTokens": 25 }
    });
    let usage = parse_usage(&value);
    assert_eq!(usage.model.as_deref(), Some("gpt-5.4"));
    assert_eq!(usage.input_tokens, Some(17));
    assert_eq!(usage.output_tokens, Some(8));
    assert_eq!(usage.total_tokens, Some(25));
    assert_eq!(parse_usage(&serde_json::json!({})).input_tokens, None);
}

#[cfg(unix)]
#[test]
fn usage_file_is_consumed_when_the_cli_supports_it() {
    let root = temp_dir("usage-output");
    let script = r#"while [ "$#" -gt 0 ]; do
if [ "$1" = "--usage-output-file" ]; then
  printf '{"model":"gpt-5.4","inputTokens":12,"outputTokens":6}' > "$2"
  shift 2
else
  shift
fi
done
cat >/dev/null
echo done
"#;
    let binary = fake_cli(&root, script);
    let usage_file = UsageFile::new();
    let request = request(ExecutionMode::Implementation);
    let args = super::execute::command(&binary, &request, None, Some(&usage_file.path));
    let output = super::execute::run_process(&request, &args, "prompt".into()).unwrap();
    assert_eq!(output.success, Some(true));
    assert_eq!(normalize_output(&output.stdout).as_deref(), Some("done"));
    let usage = usage_file.read();
    assert_eq!(usage.model.as_deref(), Some("gpt-5.4"));
    assert_eq!(usage.input_tokens, Some(12));
    assert_eq!(usage.output_tokens, Some(6));
}

#[cfg(unix)]
#[test]
fn execution_captures_success_and_classifies_failure_and_empty_output() {
    let root = temp_dir("execute");
    let success = fake_cli(&root, "cat >/dev/null\nprintf 'task complete\\n'\n");
    let request = request(ExecutionMode::Implementation);
    let args = super::execute::command(&success, &request, Some("model-test"), None);
    let output = super::execute::run_process(&request, &args, "system and task".into()).unwrap();
    assert_eq!(
        normalize_output(&output.stdout).as_deref(),
        Some("task complete")
    );
    assert_eq!(output.success, Some(true));
    assert!(
        args.windows(2)
            .any(|pair| pair == ["--model", "model-test"])
    );

    let failure = fake_cli(
        &root,
        "cat >/dev/null\necho 'authentication required' >&2\nexit 1\n",
    );
    let args = super::execute::command(&failure, &request, None, None);
    let output = super::execute::run_process(&request, &args, "context".into()).unwrap();
    assert_eq!(output.success, Some(false));
    let diagnostic = output
        .stdout
        .iter()
        .chain(output.stderr.iter())
        .collect::<Vec<_>>();
    assert!(needs_attention(&diagnostic));
    assert!(!needs_attention(&[&"general failure".to_owned()]));
    assert_eq!(normalize_output(&[]), None);
    let _ = std::fs::remove_dir_all(root);
}

#[cfg(unix)]
#[test]
fn cancellation_stops_running_copilot_process() {
    let root = temp_dir("cancel");
    let binary = fake_cli(&root, "cat >/dev/null\nwhile true; do sleep 1; done\n");
    let request = request(ExecutionMode::Implementation);
    request
        .cancel
        .store(true, std::sync::atomic::Ordering::Relaxed);
    let args = super::execute::command(&binary, &request, None, None);
    let error = super::execute::run_process(&request, &args, "context".into()).unwrap_err();
    assert!(matches!(
        error,
        crate::error::AppError::HarnessFailed { .. }
    ));
    let _ = std::fs::remove_dir_all(root);
}

#[cfg(unix)]
#[test]
fn probe_reports_missing_and_validates_version_command() {
    use std::os::unix::fs::PermissionsExt;
    let root = temp_dir("probe");
    let path = root.join("copilot");
    std::fs::write(&path, "#!/bin/sh\necho 'GitHub Copilot CLI 1.0.82.'\n").unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert!(executable(&path));
    let (output, ok) = run(&path, &["--version"]).unwrap();
    assert!(ok);
    assert!(output.contains("1.0.82"));
    let _ = std::fs::remove_dir_all(root);
}

#[cfg(unix)]
#[test]
fn usage_file_capability_is_optional_for_older_cli_versions() {
    let root = temp_dir("usage-help");
    let supported = fake_cli(
        &root,
        "if [ \"$1\" = \"--help\" ]; then echo '--usage-output-file <file>'; fi\n",
    );
    let unsupported = fake_cli(
        &root,
        "if [ \"$1\" = \"--help\" ]; then echo 'help without usage flag'; fi\n",
    );
    assert!(supports_usage_output(&supported));
    assert!(!supports_usage_output(&unsupported));
    let _ = std::fs::remove_dir_all(root);
}

#[cfg(unix)]
fn temp_dir(name: &str) -> std::path::PathBuf {
    let root =
        std::env::temp_dir().join(format!("koolade-copilot-{name}-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    root
}

#[cfg(unix)]
fn fake_cli(root: &std::path::Path, script: &str) -> std::path::PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let path = root.join(format!("fake-copilot-{}.sh", uuid::Uuid::new_v4()));
    std::fs::write(&path, format!("#!/bin/sh\n{script}")).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    path
}
