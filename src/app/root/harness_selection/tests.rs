use super::*;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

#[test]
fn removed_saved_harness_is_reported_instead_of_silently_routing_to_pi() {
    let harness = UnavailableHarness("codex".into());
    assert_eq!(harness.label(), "codex (unavailable)");
    let error = harness.check_available().unwrap_err();
    assert!(
        error
            .detail()
            .contains("saved coding harness route 'codex'")
    );
    assert!(error.detail().contains("choose an available harness"));
}

struct CountHarness(Arc<AtomicUsize>, &'static str);

impl AiHarness for CountHarness {
    fn label(&self) -> String {
        self.1.into()
    }
    fn check_available(&self) -> Result<String, crate::error::AppError> {
        Ok(self.1.into())
    }
    fn execute(
        &self,
        _request: &crate::harness::PlanningRequest,
    ) -> Result<crate::harness::HarnessOutcome, crate::error::AppError> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Ok(crate::harness::HarnessOutcome {
            final_text: String::new(),
            envelope: None,
            stderr_tail: String::new(),
        })
    }
}

#[test]
fn implementation_repairs_after_verification_failure_use_the_independent_qa_harness() {
    let implementation_calls = Arc::new(AtomicUsize::new(0));
    let qa_calls = Arc::new(AtomicUsize::new(0));
    let harness = ImplementationQaHarness {
        implementation: Box::new(CountHarness(implementation_calls.clone(), "implementation")),
        qa: Box::new(CountHarness(qa_calls.clone(), "qa")),
    };
    let (progress_tx, _progress_rx) = std::sync::mpsc::channel();
    let request = crate::harness::PlanningRequest {
        mode: crate::harness::ExecutionMode::Implementation,
        reasoning_level: "medium".into(),
        telemetry_phase: Some("qa_verification".into()),
        repo_root: std::path::PathBuf::from("/synthetic/project"),
        prompt_body: String::new(),
        system_instructions: String::new(),
        timeout: std::time::Duration::from_secs(1),
        progress_tx,
        cancel: Arc::new(std::sync::atomic::AtomicBool::new(false)),
    };
    harness.execute(&request).unwrap();
    assert_eq!(implementation_calls.load(Ordering::SeqCst), 0);
    assert_eq!(qa_calls.load(Ordering::SeqCst), 1);
}

#[test]
fn saved_supported_harness_ids_resolve_to_their_adapters() {
    assert!(resolve(Some("pi")).label().starts_with("pi "));
    assert!(resolve(Some("codex")).label().starts_with("codex "));
    assert!(resolve(Some("claude")).label().starts_with("claude "));
}

#[test]
fn work_categories_route_independently_and_unmapped_categories_use_the_global_default() {
    use crate::persistence::harness_settings::{HarnessSettings, WorkRoute};
    let settings = HarnessSettings {
        default_harness: Some("pi".into()),
        work_routes: std::collections::BTreeMap::from([
            (
                "implementation".into(),
                WorkRoute {
                    harness: "pi".into(),
                    model: Some("local-model".into()),
                },
            ),
            (
                "manager".into(),
                WorkRoute {
                    harness: "codex".into(),
                    model: Some("codex-model".into()),
                },
            ),
            (
                "qa_verification".into(),
                WorkRoute {
                    harness: "claude".into(),
                    model: Some("claude-model".into()),
                },
            ),
            (
                "documentation".into(),
                WorkRoute {
                    harness: "codex".into(),
                    model: None,
                },
            ),
        ]),
        ..HarnessSettings::default()
    };
    for (category, expected_harness, expected_model) in [
        ("implementation", "pi", Some("local-model")),
        ("manager", "codex", Some("codex-model")),
        ("qa_verification", "claude", Some("claude-model")),
        ("documentation", "codex", None),
    ] {
        let (harness, route) = route_selection(&settings, Some(category), None);
        assert_eq!(harness, Some(expected_harness));
        assert_eq!(
            route.and_then(|route| route.model.as_deref()),
            expected_model
        );
    }
    let (harness, route) = route_selection(&settings, Some("future-category"), None);
    assert_eq!(harness, Some("pi"));
    assert!(route.is_none());
}

#[test]
fn configured_model_missing_from_refreshed_catalog_fails_closed() {
    use crate::persistence::harness_settings::{DetectedHarness, HarnessSettings, WorkRoute};
    let settings = HarnessSettings {
        work_routes: std::collections::BTreeMap::from([(
            "manager".into(),
            WorkRoute {
                harness: "codex".into(),
                model: Some("removed-model".into()),
            },
        )]),
        discovered: std::collections::BTreeMap::from([(
            "codex".into(),
            DetectedHarness {
                status: "codex ready".into(),
                version: None,
                executable: None,
                diagnostic: None,
                ready: true,
                models: vec!["available-model".into()],
                default_model: None,
                configuration_required: false,
            },
        )]),
        ..HarnessSettings::default()
    };
    let harness = routed_harness(&settings, Some("manager"), None);
    let error = harness.check_available().unwrap_err().detail();
    assert!(error.contains("removed-model"));
    assert!(error.contains("refresh discovery"));
}

#[test]
fn route_label_reports_configured_default_model_before_harness_usage() {
    use crate::persistence::harness_settings::{DetectedHarness, HarnessSettings, WorkRoute};
    let settings = HarnessSettings {
        work_routes: std::collections::BTreeMap::from([(
            "manager".into(),
            WorkRoute {
                harness: "codex".into(),
                model: None,
            },
        )]),
        discovered: std::collections::BTreeMap::from([(
            "codex".into(),
            DetectedHarness {
                status: "codex ready".into(),
                version: None,
                executable: None,
                diagnostic: None,
                ready: true,
                models: vec!["gpt-configured".into()],
                default_model: Some("gpt-configured".into()),
                configuration_required: false,
            },
        )]),
        ..HarnessSettings::default()
    };
    let harness = routed_harness(&settings, Some("manager"), None);
    assert!(
        harness
            .label()
            .ends_with(" / CLI default (last discovered: gpt-configured)")
    );
}

#[test]
fn route_label_says_when_cli_does_not_report_its_default_model() {
    use crate::persistence::harness_settings::{DetectedHarness, HarnessSettings, WorkRoute};
    let settings = HarnessSettings {
        work_routes: std::collections::BTreeMap::from([(
            "manager".into(),
            WorkRoute {
                harness: "codex".into(),
                model: None,
            },
        )]),
        discovered: std::collections::BTreeMap::from([(
            "codex".into(),
            DetectedHarness {
                status: "codex ready".into(),
                version: None,
                executable: None,
                diagnostic: None,
                ready: true,
                models: vec![],
                default_model: None,
                configuration_required: false,
            },
        )]),
        ..HarnessSettings::default()
    };
    let harness = routed_harness(&settings, Some("manager"), None);
    assert!(
        harness
            .label()
            .contains("CLI default (model not reported for codex)")
    );
}
