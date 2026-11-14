use super::*;

fn request() -> crate::harness::PlanningRequest {
    crate::harness::PlanningRequest {
        investigation_authorization: None,
        prompt_manifest: None,
        mode: crate::harness::ExecutionMode::Investigation,
        task_id: None,
        execution_workspace: None,
        destination_sha: None,
        authorization_verifier: None,
        reasoning_level: "high".into(),
        telemetry_phase: None,
        repo_root: std::path::PathBuf::from("/synthetic/project"),
        runtime_config_source: None,
        prompt_body: String::new(),
        system_instructions: String::new(),
        timeout: std::time::Duration::from_secs(1),
        progress_tx: std::sync::mpsc::channel().0,
        cancel: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        review_identity: None,
        computer_use: None,
    }
}

#[test]
fn route_provenance_does_not_mislabel_a_discovered_model_hint_as_requested() {
    use crate::persistence::harness_settings::{DetectedHarness, HarnessSettings, QA, WorkRoute};
    let settings = HarnessSettings {
        work_routes: std::collections::BTreeMap::from([(
            QA.into(),
            WorkRoute {
                harness: "pi".into(),
                model: None,
            },
        )]),
        discovered: std::collections::BTreeMap::from([(
            "pi".into(),
            DetectedHarness {
                verified_at_unix_ms: None,
                status: "pi ready".into(),
                version: Some("1.0.0".into()),
                executable: Some("/synthetic/bin/pi".into()),
                diagnostic: None,
                ready: true,
                models: vec!["configured-default".into()],
                default_model: Some("configured-default".into()),
                model_catalog_fresh: true,
                configuration_required: false,
                implementation_available: true,
                checked_at_unix_ms: crate::persistence::harness_settings::current_unix_ms(),
                capability_snapshot: Some(super::authentication::ready_snapshot("pi")),
            },
        )]),
        ..HarnessSettings::default()
    };
    let harness = routed_harness(&settings, Some(QA), None, None);
    assert!(harness.label().contains("configured-default"));
    let provenance = harness.route_provenance(&request()).unwrap();
    assert_eq!(provenance.requested_model, None);
    assert_eq!(
        provenance.selected_model.as_deref(),
        Some("configured-default")
    );
    assert_eq!(provenance.provider_id, "pi");
    assert!(
        harness
            .capability_refresh_binding()
            .is_some_and(|binding| binding.observed_at_unix_ms > 0)
    );
}

#[test]
fn stale_selected_route_exposes_the_exact_binding_while_proof_refreshes() {
    use crate::persistence::harness_settings::{
        DetectedHarness, HarnessSettings, IMPLEMENTATION, WorkRoute,
    };
    let settings = HarnessSettings {
        work_routes: std::collections::BTreeMap::from([(
            IMPLEMENTATION.into(),
            WorkRoute {
                harness: "pi".into(),
                model: Some("pinned-model".into()),
            },
        )]),
        discovered: std::collections::BTreeMap::from([(
            "pi".into(),
            DetectedHarness {
                verified_at_unix_ms: None,
                status: "pi ready".into(),
                version: Some("1.0.0".into()),
                executable: Some("/synthetic/bin/pi".into()),
                diagnostic: None,
                ready: true,
                models: vec!["pinned-model".into()],
                default_model: Some("default-model".into()),
                model_catalog_fresh: false,
                configuration_required: false,
                implementation_available: true,
                checked_at_unix_ms: 1,
                capability_snapshot: None,
            },
        )]),
        ..HarnessSettings::default()
    };

    let harness = routed_harness(&settings, Some(IMPLEMENTATION), None, None);
    assert!(harness.route_blocker().is_some());
    assert_eq!(
        harness.capability_refresh_binding(),
        Some(crate::harness::CapabilityRefreshBindingV1 {
            provider_id: "pi".into(),
            role: crate::execution::contract::HarnessRole::Implementer,
            work_category: IMPLEMENTATION.into(),
            selected_model: "pinned-model".into(),
            requested_model: Some("pinned-model".into()),
            observed_at_unix_ms: 1,
        })
    );
}

#[test]
fn capability_refresh_state_requires_fresh_proof_and_rejects_route_changes() {
    use crate::persistence::harness_settings::{
        DetectedHarness, HarnessSettings, IMPLEMENTATION, WorkRoute,
    };
    let stale_settings = HarnessSettings {
        work_routes: std::collections::BTreeMap::from([(
            IMPLEMENTATION.into(),
            WorkRoute {
                harness: "pi".into(),
                model: Some("pinned-model".into()),
            },
        )]),
        discovered: std::collections::BTreeMap::from([(
            "pi".into(),
            DetectedHarness {
                verified_at_unix_ms: None,
                status: "pi ready".into(),
                version: Some("1.0.0".into()),
                executable: Some("/synthetic/bin/pi".into()),
                diagnostic: None,
                ready: true,
                models: vec!["pinned-model".into()],
                default_model: Some("pinned-model".into()),
                model_catalog_fresh: false,
                configuration_required: false,
                implementation_available: true,
                checked_at_unix_ms: 1,
                capability_snapshot: None,
            },
        )]),
        ..HarnessSettings::default()
    };
    let binding = crate::harness::CapabilityRefreshBindingV1 {
        provider_id: "pi".into(),
        role: crate::execution::contract::HarnessRole::Implementer,
        work_category: IMPLEMENTATION.into(),
        selected_model: "pinned-model".into(),
        requested_model: Some("pinned-model".into()),
        observed_at_unix_ms: 1,
    };
    let path =
        std::env::temp_dir().join(format!("capability-refresh-{}.json", uuid::Uuid::new_v4()));
    crate::persistence::harness_settings::with_settings_path(path.clone(), || {
        crate::persistence::harness_settings::save(&stale_settings).unwrap();
        assert_eq!(
            capability_refresh_route_state(&binding, &Default::default(), &Default::default()),
            CapabilityRefreshRouteState::Failed
        );

        let mut fresh = stale_settings.clone();
        let detected = fresh.discovered.get_mut("pi").unwrap();
        detected.checked_at_unix_ms = crate::persistence::harness_settings::current_unix_ms();
        detected.verified_at_unix_ms = Some(detected.checked_at_unix_ms);
        detected.model_catalog_fresh = true;
        detected.capability_snapshot = Some(super::authentication::ready_snapshot("pi"));
        crate::persistence::harness_settings::save(&fresh).unwrap();
        assert_eq!(
            capability_refresh_route_state(&binding, &Default::default(), &Default::default()),
            CapabilityRefreshRouteState::Ready
        );

        fresh.work_routes.get_mut(IMPLEMENTATION).unwrap().model = Some("other-model".into());
        fresh
            .discovered
            .get_mut("pi")
            .unwrap()
            .models
            .push("other-model".into());
        crate::persistence::harness_settings::save(&fresh).unwrap();
        assert_eq!(
            capability_refresh_route_state(&binding, &Default::default(), &Default::default()),
            CapabilityRefreshRouteState::RouteChanged
        );
    });
    let _ = std::fs::remove_file(path);
}
