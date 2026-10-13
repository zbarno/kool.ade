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
        prompt_manifest: None,
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
        execution_workspace: None,

        authorization_verifier: None,
        review_identity: None,
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
        prompt_manifest: None,
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
        execution_workspace: None,

        authorization_verifier: None,
        review_identity: None,
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
    assert!(!error.detail().is_empty());
    assert!(error.detail().contains("unsupported"));
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[test]
fn configured_alternate_implementation_route_fails_closed_without_repository_access() {
    let now = crate::persistence::harness_settings::current_unix_ms();
    let settings = crate::persistence::harness_settings::HarnessSettings {
        default_harness: Some("codex".into()),
        discovered: std::collections::BTreeMap::from([(
            "codex".into(),
            crate::persistence::harness_settings::DetectedHarness {
                status: "codex ready".into(),
                version: Some("1.0.0".into()),
                executable: Some("/synthetic/bin/codex".into()),
                diagnostic: None,
                ready: true,
                models: Vec::new(),
                default_model: None,
                model_catalog_fresh: false,
                configuration_required: false,
                implementation_available: false,
                checked_at_unix_ms: now,
                capability_snapshot: None,
            },
        )]),
        ..Default::default()
    };

    let routes = std::collections::BTreeMap::from([(
        crate::persistence::harness_settings::IMPLEMENTATION.into(),
        crate::persistence::harness_settings::WorkRoute {
            harness: "codex".into(),
            model: None,
        },
    )]);
    let error = super::super::validate_task_routes(&settings, &routes).unwrap_err();

    assert!(error.contains("application-owned Docker execution session"));
}
