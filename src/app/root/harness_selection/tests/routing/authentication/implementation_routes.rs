use super::*;

#[test]
fn explicit_implementation_route_rejects_unsupported_provider_capability() {
    let now = crate::persistence::harness_settings::current_unix_ms();
    let settings = settings_with("unknown", ReadinessState::Ready, now);
    let routes = std::collections::BTreeMap::from([(
        IMPLEMENTATION.into(),
        WorkRoute {
            harness: "unknown".into(),
            model: None,
        },
    )]);
    let path = std::env::temp_dir().join(format!(
        "harness-route-capability-{}.json",
        uuid::Uuid::new_v4()
    ));

    crate::persistence::harness_settings::with_settings_path(path.clone(), || {
        crate::persistence::harness_settings::save(&settings).unwrap();
        let mut override_harness = None;
        let harness =
            configured_harness_for_task(&mut override_harness, Some(IMPLEMENTATION), &routes);
        let error = harness.check_available().unwrap_err().detail();
        assert!(error.contains("no supported application-owned implementation route"));
    });
    let _ = std::fs::remove_file(path);
}

#[test]
fn uncategorized_implementation_route_rejects_runtime_unavailable_provider() {
    let now = crate::persistence::harness_settings::current_unix_ms();
    let mut settings = settings_with("opencode", ReadinessState::Ready, now);
    settings
        .discovered
        .get_mut("opencode")
        .unwrap()
        .implementation_available = false;
    let routes = std::collections::BTreeMap::from([(
        IMPLEMENTATION.into(),
        WorkRoute {
            harness: "opencode".into(),
            model: None,
        },
    )]);
    let path = std::env::temp_dir().join(format!(
        "harness-route-runtime-unavailable-{}.json",
        uuid::Uuid::new_v4()
    ));

    crate::persistence::harness_settings::with_settings_path(path.clone(), || {
        crate::persistence::harness_settings::save(&settings).unwrap();
        let mut override_harness = None;
        let harness =
            configured_harness_for_task(&mut override_harness, Some(IMPLEMENTATION), &routes);
        let error = harness.check_available().unwrap_err().detail();
        assert!(error.contains("not currently available for implementation"));
    });
    let _ = std::fs::remove_file(path);
}

#[test]
fn context_only_default_route_does_not_require_implementation_runtime() {
    let now = crate::persistence::harness_settings::current_unix_ms();
    let mut settings = settings_with("opencode", ReadinessState::Ready, now);
    let detected = settings.discovered.get_mut("opencode").unwrap();
    detected.implementation_available = false;
    let snapshot = detected.capability_snapshot.as_mut().unwrap();
    snapshot.execution_target = ReadinessState::Unavailable;
    snapshot.docker_mcp_bridge = ReadinessState::Unavailable;
    for role in &mut snapshot.roles {
        if role.role == crate::execution::contract::HarnessRole::PlanningAnalyst {
            role.available = ReadinessState::Unavailable;
        }
    }
    let path = std::env::temp_dir().join(format!(
        "harness-context-only-{}.json",
        uuid::Uuid::new_v4()
    ));

    crate::persistence::harness_settings::with_settings_path(path.clone(), || {
        crate::persistence::harness_settings::save(&settings).unwrap();
        let mut override_harness = None;
        let harness = configured_harness_for_task(
            &mut override_harness,
            None,
            &std::collections::BTreeMap::new(),
        );
        let provenance = harness
            .route_provenance(&planning_request())
            .expect("fresh context-only planning should have route provenance");
        assert_eq!(provenance.provider_id, "opencode");
    });
    let _ = std::fs::remove_file(path);
}

#[test]
fn stale_saved_default_is_rejected_before_it_can_route() {
    use crate::persistence::harness_settings::DISCOVERY_FRESHNESS_MS;

    let now = crate::persistence::harness_settings::current_unix_ms();
    let settings = settings_with(
        "opencode",
        ReadinessState::Ready,
        now.saturating_sub(DISCOVERY_FRESHNESS_MS + 1),
    );
    let path = std::env::temp_dir().join(format!(
        "harness-stale-default-{}.json",
        uuid::Uuid::new_v4()
    ));

    crate::persistence::harness_settings::with_settings_path(path.clone(), || {
        crate::persistence::harness_settings::save(&settings).unwrap();
        let mut override_harness = None;
        let harness = configured_harness_for_task(
            &mut override_harness,
            None,
            &std::collections::BTreeMap::new(),
        );
        let error = harness.check_available().unwrap_err().detail();
        assert!(error.contains("reverified automatically"), "{error}");
    });
    let _ = std::fs::remove_file(path);
}

fn planning_request() -> crate::harness::PlanningRequest {
    crate::harness::PlanningRequest {
        prompt_manifest: None,
        mode: crate::harness::ExecutionMode::Planning,
        task_id: None,
        reasoning_level: "medium".into(),
        telemetry_phase: None,
        repo_root: std::path::PathBuf::from("/synthetic/project"),
        runtime_config_source: None,
        prompt_body: String::new(),
        system_instructions: String::new(),
        timeout: std::time::Duration::from_secs(1),
        progress_tx: std::sync::mpsc::channel().0,
        cancel: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        execution_workspace: None,
        destination_sha: None,
        authorization_verifier: None,
        review_identity: None,
    }
}
