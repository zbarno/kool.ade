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
        repair: None,
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
        destination_sha: None,

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
fn runnable_provider_clis_pass_implementation_requests_through_the_shared_policy() {
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
        destination_sha: None,

        authorization_verifier: None,
        review_identity: None,
    };

    for provider in ["pi", "antigravity", "opencode", "copilot"] {
        let calls = Arc::new(AtomicUsize::new(0));
        let harness = RoutedHarness {
            inner: Box::new(CountHarness(calls.clone(), "supported")),
            harness_id: provider.into(),
            model: None,
            provenance: test_route_provenance(provider),
        };
        harness.execute(&request).unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 1, "{provider}");
    }

    for provider in ["codex", "claude"] {
        let calls = Arc::new(AtomicUsize::new(0));
        let harness = RoutedHarness {
            inner: Box::new(CountHarness(calls.clone(), "must not run")),
            harness_id: provider.into(),
            model: None,
            provenance: test_route_provenance(provider),
        };
        let error = harness.execute(&request).unwrap_err().detail();
        assert!(
            error.contains("organization-managed hooks"),
            "{provider}: {error}"
        );
        assert_eq!(calls.load(Ordering::SeqCst), 0, "{provider}");
    }

    let calls = Arc::new(AtomicUsize::new(0));
    let unsupported = RoutedHarness {
        inner: Box::new(CountHarness(calls.clone(), "unsupported")),
        harness_id: "unsupported".into(),
        model: None,
        provenance: test_route_provenance("unsupported"),
    };
    let error = unsupported.execute(&request).unwrap_err();
    assert!(!error.detail().is_empty());
    assert!(
        error.detail().contains("unsupported")
            || error
                .detail()
                .contains("no verified application-owned execution profile"),
        "{}",
        error.detail()
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[test]
fn routed_provider_refuses_expired_or_changed_role_evidence_before_execution() {
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
        destination_sha: None,
        authorization_verifier: None,
        review_identity: None,
    };
    let provider = "pi";
    let changed_authentication =
        match crate::execution::contract::current_authentication_mode(provider) {
            crate::execution::contract::AuthenticationModeV1::Environment => {
                crate::execution::contract::AuthenticationModeV1::ProviderProfile
            }
            _ => crate::execution::contract::AuthenticationModeV1::Environment,
        };
    let cases = [
        {
            let mut route = test_route_provenance(provider);
            route.capability_expires_at_unix_ms = Some(0);
            route
        },
        {
            let mut route = test_route_provenance(provider);
            route.cli_version = Some("0.0.0".into());
            route.profile_evidence_id = Some(
                crate::execution::contract::profile_evidence_id_for_snapshot(
                    provider,
                    route.cli_version.as_deref(),
                    route.authentication_mode.unwrap(),
                    crate::harness::ExecutionProfile::Worker,
                ),
            );
            route
        },
        {
            let mut route = test_route_provenance(provider);
            route.authentication_mode = Some(changed_authentication);
            route.profile_evidence_id = Some(
                crate::execution::contract::profile_evidence_id_for_snapshot(
                    provider,
                    route.cli_version.as_deref(),
                    changed_authentication,
                    crate::harness::ExecutionProfile::Worker,
                ),
            );
            route
        },
    ];

    for provenance in cases {
        let calls = Arc::new(AtomicUsize::new(0));
        let harness = RoutedHarness {
            inner: Box::new(CountHarness(calls.clone(), "supported")),
            harness_id: provider.into(),
            model: None,
            provenance,
        };
        assert!(harness.execute(&request).is_err());
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }
}

fn test_route_provenance(provider: &str) -> crate::harness::HarnessRouteProvenance {
    crate::harness::HarnessRouteProvenance {
        provider_id: provider.into(),
        role: crate::execution::contract::HarnessRole::Implementer,
        work_category: Some("implementation".into()),
        selected_model: None,
        requested_model: None,
        capability_probe_id: None,
        capability_generation: None,
        authentication_mode: Some(crate::execution::contract::current_authentication_mode(
            provider,
        )),
        profile_evidence_id: Some(
            crate::execution::contract::profile_evidence_id_for_current_snapshot(
                provider,
                Some("supported"),
                crate::harness::ExecutionProfile::Worker,
            ),
        ),
        cli_version: Some("supported".into()),
        capability_expires_at_unix_ms: Some(u64::MAX),
    }
}

#[test]
fn configured_alternate_implementation_route_fails_closed_without_repository_access() {
    let now = crate::persistence::harness_settings::current_unix_ms();
    let settings = crate::persistence::harness_settings::HarnessSettings {
        default_harness: Some("opencode".into()),
        discovered: std::collections::BTreeMap::from([(
            "opencode".into(),
            crate::persistence::harness_settings::DetectedHarness {
                status: "opencode ready".into(),
                version: Some("1.0.0".into()),
                executable: Some("/synthetic/bin/opencode".into()),
                diagnostic: None,
                ready: true,
                models: Vec::new(),
                default_model: None,
                model_catalog_fresh: false,
                configuration_required: false,
                implementation_available: false,
                checked_at_unix_ms: now,
                capability_snapshot: Some(super::routing::authentication::ready_snapshot(
                    "opencode",
                )),
            },
        )]),
        ..Default::default()
    };

    let routes = std::collections::BTreeMap::from([(
        crate::persistence::harness_settings::IMPLEMENTATION.into(),
        crate::persistence::harness_settings::WorkRoute {
            harness: "opencode".into(),
            model: None,
        },
    )]);
    let error = super::super::validate_task_routes(&settings, &routes).unwrap_err();

    assert!(error.contains("application-owned Docker execution session"));
}
