use super::*;

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
    let outcome = crate::harness::with_uncontained_provider_test_execution(|| {
        CodexHarness.execute(&request(root.clone(), tx))
    })
    .unwrap();
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
    let mut _env = EnvOverride::set(CODEX_BINARY_ENV, &binary);
    let (tx, _) = mpsc::channel();
    let error = crate::harness::with_uncontained_provider_test_execution(|| {
        CodexHarness.execute(&request(root.clone(), tx))
    })
    .unwrap_err();
    assert!(error.detail().contains("approval required"));

    let binary = fake_cli(&root, "echo not-json\nexit 0");
    _env.set_value(&binary);
    let (tx, _) = mpsc::channel();
    let error = crate::harness::with_uncontained_provider_test_execution(|| {
        CodexHarness.execute(&request(root.clone(), tx))
    })
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
        crate::harness::with_uncontained_provider_test_execution(|| {
            CodexHarness.execute(&PlanningRequest {
                prompt_manifest: None,
                cancel: worker_cancel,
                ..request(worker_root, tx)
            })
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
