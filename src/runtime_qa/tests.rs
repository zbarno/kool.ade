use super::*;

mod browser_driver;
#[path = "tests/contracts.rs"]
mod contract_tests;
mod evidence;
mod polyglot_recipe_path;
mod scenario_approval;
mod stack_matrix;
mod target;

fn request() -> QaRunRequestV1 {
    let uid = || uuid::Uuid::new_v4().hyphenated().to_string();
    let sha = || "a".repeat(64);
    QaRunRequestV1 {
        schema_version: 1,
        qa_run_id: uid(),
        parent_task_uid: uid(),
        target_id: Some("fixture-target".into()),
        planning_item_uid: None,
        feature_uid: uid(),
        code_head_sha: sha(),
        build_artifact_sha256: sha(),
        complete_diff_hash: sha(),
        review_subject_sha256: sha(),
        approved_spec_hash: sha(),
        requirement_ids: vec!["REQ-1".into()],
        acceptance_criteria: vec!["The main view renders".into()],
        relevant_platforms: vec![QaPlatform::Web],
        startup_recipe_ref: "recipe:approved-web".into(),
        startup_recipe_sha256: "e".repeat(64),
        execution_session_id: uid(),
        driver_profile_version: "web-driver/v1".into(),
        test_data_fixture_ref: None,
        dependency_cache_refs: Vec::new(),
        bounded_limits: QaBoundedLimitsV1 {
            max_scenarios: 8,
            max_actions_per_scenario: 16,
            max_total_actions: 64,
            timeout_seconds: 120,
            max_evidence_bytes: 1024 * 1024,
            max_screenshots: 16,
        },
        author_run_id: uid(),
        author_session_id: uid(),
    }
}

fn scenario(request: &QaRunRequestV1) -> QaScenarioV1 {
    QaScenarioV1 {
        schema_version: 1,
        id: uuid::Uuid::new_v4().hyphenated().to_string(),
        requirement_ids: vec![request.requirement_ids[0].clone()],
        preconditions: vec![],
        steps: vec![QaScenarioStepV1 {
            driver_action: QaDriverActionV1::NavigateAllowedUrl {
                url: "http://qa-service/".into(),
            },
            expected_state: Some("home view visible".into()),
            screenshot_checkpoint: true,
        }],
        expected_results: vec!["Main view is visible".into()],
        negative_paths: vec![],
        visual_expectations: vec![],
        redaction_regions: vec![],
        priority: 1,
    }
}

#[test]
fn scenario_sets_must_cover_every_approved_requirement() {
    let mut request = request();
    request.requirement_ids.push("REQ-2".into());
    assert!(validate_scenario_set(&request, &[scenario(&request)]).is_err());
}

#[test]
fn invalid_assessment_cannot_claim_unsupported_without_evidence() {
    let request = request();
    let assessment = QaApplicabilityAssessmentV1 {
        schema_version: 1,
        task_uid: request.parent_task_uid,
        target_id: "service-api".into(),
        app_surface: crate::project_runtime::AppSurface::UnsupportedTarget,
        code_sha: request.code_head_sha,
        approved_spec_hash: request.approved_spec_hash,
        applicability: QaApplicability::Unsupported,
        reason: "No supported driver is available".into(),
        evidence_refs: vec![],
        reviewer_execution_id: uuid::Uuid::new_v4().hyphenated().to_string(),
        reviewer_session_id: uuid::Uuid::new_v4().hyphenated().to_string(),
        approved_by: uuid::Uuid::new_v4().hyphenated().to_string(),
        manager_session_id: uuid::Uuid::new_v4().hyphenated().to_string(),
        recorded_at_unix_ms: 1,
    };
    assert!(assessment.validate().is_err());
}

#[test]
fn applicability_is_durable_exact_subject_bound_and_not_a_completed_run() {
    let request = request();
    let assessment = QaApplicabilityAssessmentV1 {
        schema_version: 1,
        task_uid: request.parent_task_uid.clone(),
        target_id: "service-api".into(),
        app_surface: crate::project_runtime::AppSurface::UnsupportedTarget,
        code_sha: request.code_head_sha.clone(),
        approved_spec_hash: request.approved_spec_hash.clone(),
        applicability: QaApplicability::Unsupported,
        reason: "No approved target recipe supports this application surface".into(),
        evidence_refs: vec![
            "target:service-api".into(),
            "capability-manifest:target-1".into(),
        ],
        reviewer_execution_id: uuid::Uuid::new_v4().hyphenated().to_string(),
        reviewer_session_id: uuid::Uuid::new_v4().hyphenated().to_string(),
        approved_by: uuid::Uuid::new_v4().hyphenated().to_string(),
        manager_session_id: uuid::Uuid::new_v4().hyphenated().to_string(),
        recorded_at_unix_ms: 1,
    };
    let directory = tempfile::tempdir().unwrap();
    let store = RuntimeQaStore::new(directory.path());

    store.save_applicability(&request, &assessment).unwrap();
    assert_eq!(
        store.load_applicability(&request.qa_run_id).unwrap(),
        assessment
    );
    assert!(store.load_run(&request.qa_run_id).is_err());

    let mut stale = assessment;
    stale.code_sha = "b".repeat(64);
    assert!(store.save_applicability(&request, &stale).is_err());
}

fn desktop_probe() -> LinuxDesktopProbeV1 {
    let uid = || uuid::Uuid::new_v4().hyphenated().to_string();
    let qa_run_id = uid();
    LinuxDesktopProbeV1 {
        schema_version: 1,
        qa_run_id: qa_run_id.clone(),
        task_uid: uid(),
        image_digest: "a".repeat(64),
        artifact_sha256: "b".repeat(64),
        executable_sha256: "b".repeat(64),
        display_width: 1440,
        display_height: 900,
        dpi: 96,
        private_xvfb_ready: true,
        llvmpipe_ready: true,
        input_driver_ready: true,
        screenshot_capture_ready: true,
        isolated_home_ready: true,
        process_scope_ready: true,
        host_display_mounted: false,
        host_wayland_mounted: false,
        host_dbus_mounted: false,
        host_xauthority_mounted: false,
        host_input_mounted: false,
        host_gpu_mounted: false,
        host_audio_mounted: false,
        evidence_refs: vec![
            format!("qa-evidence/{qa_run_id}/image.json"),
            format!("qa-evidence/{qa_run_id}/display.json"),
            format!("qa-evidence/{qa_run_id}/isolation.json"),
        ],
        recorded_at_unix_ms: 1,
    }
}

#[test]
fn desktop_capability_fails_closed_without_private_display_and_rejects_host_access() {
    let mut probe = desktop_probe();
    probe.private_xvfb_ready = false;
    assert!(matches!(
        probe.assess().unwrap(),
        LinuxDesktopCapabilityV1::Unsupported { missing, .. }
            if missing.iter().any(|item| item.as_str() == "private Xvfb display")
                && missing.iter().any(|item| item.contains("approved Linux desktop QA image"))
    ));

    probe.private_xvfb_ready = true;
    probe.host_display_mounted = true;
    assert!(
        probe
            .assess()
            .unwrap_err()
            .contains("cannot expose host display")
    );
}

#[test]
fn desktop_actions_are_limited_to_current_windows_and_display_geometry() {
    let window_id = "window-1".to_owned();
    let session = LinuxDesktopSessionV1 {
        qa_run_id: uuid::Uuid::new_v4().hyphenated().to_string(),
        display_width: 1024,
        display_height: 768,
        windows: vec![LinuxDesktopWindowV1 {
            window_id: window_id.clone(),
            x: 0,
            y: 0,
            width: 1024,
            height: 768,
        }],
    };
    assert!(
        session
            .validate_action(&QaDriverActionV1::Click {
                window_id: window_id.clone(),
                x: 1023,
                y: 767,
            })
            .is_ok()
    );
    assert!(
        session
            .validate_action(&QaDriverActionV1::Click {
                window_id,
                x: 1024,
                y: 400,
            })
            .is_err()
    );
    assert!(
        session
            .validate_action(&QaDriverActionV1::NavigateAllowedUrl {
                url: "http://qa-service/".into(),
            })
            .is_err()
    );
}

fn browser_binding() -> QaPreviewRelayBindingV1 {
    let uid = || uuid::Uuid::new_v4().hyphenated().to_string();
    QaPreviewRelayBindingV1 {
        schema_version: 1,
        qa_run_id: uid(),
        task_uid: uid(),
        task_execution_id: uid(),
        task_container_id: "a".repeat(64),
        relay_container_id: "b".repeat(64),
        relay_token: "0123456789abcdef".repeat(4),
        browser_container_id: Some("c".repeat(64)),
        task_network_id: "d".repeat(64),
        relay_browser_network_id: "e".repeat(64),
        target_preview_port: 3000,
        relay_port: 8080,
        preview_origin: "http://qa-preview:8080".into(),
        task_build_sha: "b".repeat(64),
        approved_spec_hash: "c".repeat(64),
        startup_recipe_id: "preview-start-v1".into(),
        startup_recipe_sha256: request().startup_recipe_sha256,
        internal_networks_only: true,
        published_ports: false,
        recorded_at_unix_ms: 1,
    }
}

#[test]
fn browser_preview_requests_are_locked_to_one_internal_task_relay_origin() {
    let binding = browser_binding();
    assert!(binding.validate().is_ok());
    assert!(
        binding
            .validate_request_url("http://qa-preview:8080/settings?tab=general")
            .is_ok()
    );
    assert!(
        binding
            .validate_request_url("ws://qa-preview:8080/live")
            .is_ok()
    );
    for blocked in [
        "http://qa-preview:8081/",
        "http://localhost:8080/",
        "http://169.254.169.254/latest/meta-data/",
        "http://10.0.0.1:8080/",
        "https://qa-preview:8080/",
        "file:///etc/passwd",
        "javascript:alert(1)",
        "http://user@qa-preview:8080/",
    ] {
        assert!(binding.validate_request_url(blocked).is_err(), "{blocked}");
    }

    let mut escaped = binding;
    escaped.published_ports = true;
    assert!(escaped.validate().is_err());
    assert!(pinned_browser_qa_image("amd64").is_err());
}
