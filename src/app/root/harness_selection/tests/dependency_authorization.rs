use super::*;
#[test]
fn alternate_qa_harness_runs_read_only_when_verifying_implementation() {
    use std::sync::Mutex;

    struct CaptureMode(Arc<Mutex<Option<crate::harness::ExecutionMode>>>);

    impl AiHarness for CaptureMode {
        fn label(&self) -> String {
            "capture".into()
        }

        fn check_available(&self) -> Result<String, crate::error::AppError> {
            Ok("capture".into())
        }

        fn execute(
            &self,
            request: &crate::harness::PlanningRequest,
        ) -> Result<crate::harness::HarnessOutcome, crate::error::AppError> {
            *self.0.lock().unwrap() = Some(request.mode);
            Ok(crate::harness::HarnessOutcome {
                final_text: "reviewed".into(),
                envelope: None,
                stderr_tail: String::new(),
            })
        }
    }

    let observed = Arc::new(Mutex::new(None));
    let harness = ImplementationQaHarness {
        implementation: Box::new(CountHarness(Arc::new(AtomicUsize::new(0)), "impl")),
        qa: Box::new(CaptureMode(observed.clone())),
    };
    let (progress_tx, _progress_rx) = std::sync::mpsc::channel();
    let request = crate::harness::PlanningRequest {
        mode: crate::harness::ExecutionMode::Implementation,
        task_id: None,
        reasoning_level: "medium".into(),
        telemetry_phase: Some("qa_verification".into()),
        repo_root: std::path::PathBuf::from("/synthetic/project"),
        runtime_config_source: None,
        prompt_body: String::new(),
        system_instructions: String::new(),
        timeout: std::time::Duration::from_secs(1),
        progress_tx,
        cancel: Arc::new(std::sync::atomic::AtomicBool::new(false)),
    };

    harness.execute(&request).unwrap();

    assert_eq!(
        *observed.lock().unwrap(),
        Some(crate::harness::ExecutionMode::ReadOnlyAnalysis)
    );
}

#[test]
fn supported_provider_clis_pass_implementation_requests_through_the_shared_policy() {
    let (progress_tx, _progress_rx) = std::sync::mpsc::channel();
    let request = crate::harness::PlanningRequest {
        mode: crate::harness::ExecutionMode::Implementation,
        task_id: None,
        reasoning_level: "medium".into(),
        telemetry_phase: None,
        repo_root: std::path::PathBuf::from("/synthetic/project"),
        runtime_config_source: None,
        prompt_body: String::new(),
        system_instructions: String::new(),
        timeout: std::time::Duration::from_secs(1),
        progress_tx,
        cancel: Arc::new(std::sync::atomic::AtomicBool::new(false)),
    };

    for provider in ["codex", "claude", "antigravity", "opencode", "copilot"] {
        let calls = Arc::new(AtomicUsize::new(0));
        let harness = RoutedHarness {
            inner: Box::new(CountHarness(calls.clone(), "supported")),
            harness_id: provider.into(),
            model: None,
            default_model_hint: None,
        };
        harness.execute(&request).unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 1, "{provider}");
    }

    let calls = Arc::new(AtomicUsize::new(0));
    let unsupported = RoutedHarness {
        inner: Box::new(CountHarness(calls.clone(), "unsupported")),
        harness_id: "unsupported".into(),
        model: None,
        default_model_hint: None,
    };
    let error = unsupported.execute(&request).unwrap_err();
    assert!(error.detail().contains("application-owned Linux sandbox"));
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[test]
fn configured_alternate_implementation_route_reports_broker_requirement() {
    let settings = crate::persistence::harness_settings::HarnessSettings {
        default_harness: Some("codex".into()),
        ..Default::default()
    };

    let error = routed_harness(
        &settings,
        Some(crate::persistence::harness_settings::IMPLEMENTATION),
        None,
        None,
    )
    .check_available()
    .unwrap_err();

    assert!(error.detail().contains("application-owned Linux sandbox"));
    assert!(error.detail().contains("no fallback"));
}
