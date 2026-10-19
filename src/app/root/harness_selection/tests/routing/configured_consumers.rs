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

fn settings_with_routes(work_routes: BTreeMap<String, WorkRoute>) -> HarnessSettings {
    let now = crate::persistence::harness_settings::current_unix_ms();
    let discovered = ["pi", "codex", "claude"]
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
