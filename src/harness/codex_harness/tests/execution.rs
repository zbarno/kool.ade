use super::*;

/// Production boundary: the Codex CLI adapter returns the selected logical
/// sources and forwards the route's model to the retrieval pass. A parser-only
/// test cannot prove the adapter launches the selected CLI. This uses a local
/// fake CLI, requires no network or Docker, and has negligible setup cost.
#[cfg(unix)]
#[test]
fn retrieval_uses_selected_model_and_rejects_invalid_plans() {
    let root =
        std::env::temp_dir().join(format!("koolade-codex-retrieval-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let arguments = root.join("arguments");
    let binary = fake_cli(
        &root,
        &format!(
            "printf '%s\\n' \"$@\" > '{}'\nprintf '%s\\n' '{{\"type\":\"item.completed\",\"item\":{{\"type\":\"agent_message\",\"text\":\"{{\\\"documents\\\":[\\\"product:overview\\\"],\\\"openItems\\\":[],\\\"repositoryAreas\\\":[]}}\"}}}}'",
            arguments.display()
        ),
    );
    let mut environment = EnvOverride::set(CODEX_BINARY_ENV, &binary);
    let (tx, _) = mpsc::channel();
    let request = PlanningRequest {
        mode: ExecutionMode::ReadOnlyAnalysis,
        ..request(root.clone(), tx)
    };
    assert!(CodexHarness.supports_context_retrieval());
    let plan = crate::harness::with_uncontained_provider_test_execution(|| {
        CodexHarness.plan_retrieval_with_model(&request, Some("fixture-model"))
    })
    .unwrap()
    .unwrap();
    assert_eq!(plan.documents, ["product:overview"]);
    assert!(
        std::fs::read_to_string(&arguments)
            .unwrap()
            .contains("--model\nfixture-model\n")
    );

    let binary = fake_cli(
        &root,
        "printf '%s\\n' '{\"type\":\"item.completed\",\"item\":{\"type\":\"agent_message\",\"text\":\"{\\\"documents\\\":42}\"}}'",
    );
    environment.set_value(&binary);
    let error = crate::harness::with_uncontained_provider_test_execution(|| {
        CodexHarness.plan_retrieval(&request)
    })
    .unwrap_err();
    assert!(error.detail().contains("Invalid retrieval plan"));
    std::fs::remove_dir_all(root).unwrap();
}

// These lifecycle tests intentionally use the test-only uncontained provider context,
// which lets fake CLI output exercise execution without treating it as auth evidence.
// Production readiness is covered separately by the fail-closed probe tests.

/// Production boundary: the actual CLI adapter must launch an authenticated
/// selected executable for both worker and Manager profiles without bypassing
/// launch validation. Unit profile checks cannot prove process invocation.
/// Uses a synthetic CLI and stored fixture receipt; no network or Docker.
#[cfg(unix)]
#[test]
fn selected_codex_cli_runs_for_worker_and_manager_profiles() {
    let root =
        std::env::temp_dir().join(format!("koolade-codex-selected-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let binary = fake_cli(
        &root,
        "printf '%s\\n' '{\"type\":\"item.completed\",\"item\":{\"type\":\"agent_message\",\"text\":\"fixture answer\"}}'\nexit 0",
    );
    let _env = EnvOverride::set(super::CODEX_BINARY_ENV, &binary);
    crate::persistence::harness_settings::with_settings_path(root.join("settings.json"), || {
        super::probe::seed_codex_detection(Some(
            crate::persistence::harness_settings::current_unix_ms(),
        ));
        for mode in [ExecutionMode::ReadOnlyAnalysis, ExecutionMode::Manager] {
            let (tx, _) = mpsc::channel();
            let outcome = CodexHarness
                .execute(&PlanningRequest {
                    mode,
                    ..request(root.clone(), tx)
                })
                .unwrap();
            assert_eq!(outcome.final_text, "fixture answer");
        }
    });
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
                investigation_authorization: None,
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
