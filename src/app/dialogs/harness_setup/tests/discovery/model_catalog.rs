use super::super::super::{
    DlgHarnessSetup, HarnessSettingsSection, ProbeView, discovery, has_current_model_catalog,
    probe_result,
};
use super::super::{HarnessSettings, report};
use crate::harness::discovery::ModelCatalogStatus;

#[test]
fn failed_refresh_keeps_last_known_catalog_as_stale_and_not_selectable() {
    let mut settings = HarnessSettings::default();
    let mut previous = report(true);
    previous.models = vec!["known-model".into()];
    previous.default_model = Some("known-model".into());
    discovery::apply_probe_results(&mut settings, &[previous], None, 1);
    super::super::super::invalidate_discovery(&mut settings, Some("pi"));
    let mut dialog = DlgHarnessSetup {
        settings,
        probe_view: ProbeView::Pending,
        feedback: None,
        section: HarnessSettingsSection::Tools,
        manual_path_drafts: Default::default(),
        probe_rx: None,
        scan_generation: 2,
        scan_cancel: None,
        runtime_available: None,
        scan_results: Vec::new(),
        probe_results: Default::default(),
        running_providers: Default::default(),
    };
    let mut failed_refresh = report(true);
    failed_refresh.models.clear();
    failed_refresh.default_model = None;
    failed_refresh.model_catalog_fresh = false;
    failed_refresh.diagnostic = Some("Model catalog could not be refreshed".into());
    let result = probe_result::from_harness_probe(&failed_refresh, 2, 2_000, 75);

    dialog.apply_probe_message(discovery::ProbeMessage::Provider {
        generation: 2,
        result,
    });

    let result = &dialog.probe_results["pi"];
    assert_eq!(result.model_catalog_status, ModelCatalogStatus::Stale);
    assert_eq!(result.models, ["known-model"]);
    assert_eq!(result.default_model.as_deref(), Some("known-model"));
    let detected = &dialog.settings.discovered["pi"];
    assert_eq!(detected.models, ["known-model"]);
    assert_eq!(detected.model_catalog_status(), ModelCatalogStatus::Stale);
    assert_eq!(
        detected.capability_snapshot.as_ref().unwrap().model_catalog,
        crate::execution::contract::ReadinessState::Unavailable
    );
    assert!(!detected.model_catalog_fresh);
    let now = crate::persistence::harness_settings::current_unix_ms();
    assert!(detected.is_fresh_at(now));
    assert!(!has_current_model_catalog("pi", detected, now));
}

#[test]
fn unavailable_catalog_without_previous_models_stays_unavailable() {
    let mut dialog = DlgHarnessSetup {
        settings: HarnessSettings::default(),
        probe_view: ProbeView::Pending,
        feedback: None,
        section: HarnessSettingsSection::Tools,
        manual_path_drafts: Default::default(),
        probe_rx: None,
        scan_generation: 5,
        scan_cancel: None,
        runtime_available: None,
        scan_results: Vec::new(),
        probe_results: Default::default(),
        running_providers: Default::default(),
    };
    let mut failed = report(true);
    failed.models.clear();
    failed.default_model = None;
    failed.model_catalog_fresh = false;
    failed.diagnostic = Some("Model catalog could not be refreshed".into());
    let result = probe_result::from_harness_probe(&failed, 5, 5_000, 20);

    dialog.apply_probe_message(discovery::ProbeMessage::Provider {
        generation: 5,
        result,
    });

    assert_eq!(
        dialog.probe_results["pi"].model_catalog_status,
        ModelCatalogStatus::Unavailable
    );
    assert!(dialog.probe_results["pi"].models.is_empty());
    assert_eq!(
        dialog.settings.discovered["pi"]
            .capability_snapshot
            .as_ref()
            .unwrap()
            .model_catalog,
        crate::execution::contract::ReadinessState::Unavailable
    );
}
