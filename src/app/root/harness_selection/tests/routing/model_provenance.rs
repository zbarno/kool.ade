use super::*;

fn request() -> crate::harness::PlanningRequest {
    crate::harness::PlanningRequest {
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
}
