use super::*;
use crate::persistence::harness_settings::{
    DetectedHarness, HarnessSettings, IMPLEMENTATION, PLANNING, REPAIR, WorkRoute,
};
use std::collections::BTreeMap;

#[test]
fn uncategorized_project_planning_uses_the_global_planning_route() {
    let settings = settings_with_routes(BTreeMap::from([(PLANNING.into(), route("codex"))]));
    with_settings(&settings, || {
        let mut override_harness = None;
        let harness = configured_harness_for_task(&mut override_harness, None, &BTreeMap::new());
        let provenance = harness.route_provenance(&request()).unwrap();
        assert_eq!(provenance.provider_id, "codex");
        assert_eq!(
            provenance.role,
            crate::execution::contract::HarnessRole::PlanningAnalyst
        );
    });
}

#[test]
fn implementation_repair_attempt_uses_the_configured_repair_provider() {
    let settings = settings_with_routes(BTreeMap::from([
        (IMPLEMENTATION.into(), route("pi")),
        (REPAIR.into(), route("claude")),
    ]));
    with_settings(&settings, || {
        let mut override_harness = None;
        let harness = configured_harness_for_task(
            &mut override_harness,
            Some(IMPLEMENTATION),
            &BTreeMap::new(),
        );
        let mut repair_request = request();
        repair_request.mode = crate::harness::ExecutionMode::Implementation;
        repair_request.telemetry_phase = Some("repair".into());
        let provenance = harness.route_provenance(&repair_request).unwrap();
        assert_eq!(provenance.provider_id, "claude");
        assert_eq!(
            provenance.role,
            crate::execution::contract::HarnessRole::RepairWorker
        );
    });
}

#[test]
fn every_role_route_selects_each_supported_provider_and_pins_its_model() {
    use crate::persistence::harness_settings as categories;

    let providers = [
        "pi",
        "codex",
        "claude",
        "opencode",
        "copilot",
        "antigravity",
    ];
    let work_categories = [
        categories::MANAGER,
        categories::PLANNING,
        categories::SPECIFICATION,
        categories::FEATURE_PLANNING,
        categories::TASK_GENERATION,
        categories::INVESTIGATION,
        categories::PLAN_REVIEW,
        categories::IMPLEMENTATION,
        categories::QA,
        categories::RUNTIME_QA,
        categories::DOCUMENTATION,
        categories::RECONCILIATION,
        categories::DECISION_EXPLANATION,
        categories::REPAIR,
    ];

    for provider in providers {
        let routes = work_categories
            .iter()
            .map(|category| {
                (
                    (*category).to_owned(),
                    WorkRoute {
                        harness: provider.into(),
                        model: Some("synthetic-default".into()),
                    },
                )
            })
            .collect();
        let settings = settings_with_routes(routes);

        for category in work_categories {
            let harness = routed_harness(&settings, Some(category), None, None);
            let mut role_request = request();
            role_request.mode =
                crate::app::root::harness_selection::role_for_work_category(category)
                    .expect("every configured role has a mode")
                    .harness_mode();
            let provenance = harness
                .route_provenance(&role_request)
                .unwrap_or_else(|| panic!("{provider} route for {category} lacks provenance"));

            assert_eq!(provenance.provider_id, provider, "{category}");
            assert_eq!(provenance.work_category.as_deref(), Some(category));
            assert_eq!(
                provenance.selected_model.as_deref(),
                Some("synthetic-default")
            );
            assert_eq!(
                provenance.requested_model.as_deref(),
                Some("synthetic-default")
            );
            assert_eq!(
                provenance.role,
                crate::app::root::harness_selection::role_for_work_category(category).unwrap(),
                "{provider} {category}"
            );
        }
    }
}

fn settings_with_routes(work_routes: BTreeMap<String, WorkRoute>) -> HarnessSettings {
    let now = crate::persistence::harness_settings::current_unix_ms();
    let discovered = [
        "pi",
        "codex",
        "claude",
        "opencode",
        "copilot",
        "antigravity",
    ]
    .into_iter()
    .map(|provider| {
        (
            provider.into(),
            DetectedHarness {
                status: format!("{provider} ready"),
                version: Some("1.0.0".into()),
                executable: Some(format!("/synthetic/bin/{provider}")),
                diagnostic: None,
                ready: true,
                models: vec!["synthetic-default".into()],
                default_model: Some("synthetic-default".into()),
                model_catalog_fresh: true,
                configuration_required: false,
                implementation_available: true,
                checked_at_unix_ms: now,
                capability_snapshot: Some(super::authentication::ready_snapshot(provider)),
            },
        )
    })
    .collect();
    HarnessSettings {
        default_harness: Some("pi".into()),
        work_routes,
        discovered,
        ..HarnessSettings::default()
    }
}

fn route(harness: &str) -> WorkRoute {
    WorkRoute {
        harness: harness.into(),
        model: None,
    }
}

fn with_settings(settings: &HarnessSettings, run: impl FnOnce()) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("harness-settings.json");
    crate::persistence::harness_settings::with_settings_path(path, || {
        crate::persistence::harness_settings::save(settings).unwrap();
        run();
    });
}

fn request() -> crate::harness::PlanningRequest {
    crate::harness::PlanningRequest {
        prompt_manifest: None,
        mode: crate::harness::ExecutionMode::Planning,
        task_id: None,
        execution_workspace: None,
        destination_sha: None,
        authorization_verifier: None,
        reasoning_level: "medium".into(),
        telemetry_phase: None,
        repo_root: std::path::PathBuf::from("/synthetic/project"),
        runtime_config_source: None,
        prompt_body: String::new(),
        system_instructions: String::new(),
        timeout: std::time::Duration::from_secs(1),
        progress_tx: std::sync::mpsc::channel().0,
        cancel: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        review_identity: None,
    }
}
