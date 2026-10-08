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
    };
    harness.execute(&request).unwrap();
    assert_eq!(implementation_calls.load(Ordering::SeqCst), 0);
    assert_eq!(qa_calls.load(Ordering::SeqCst), 1);
}

#[path = "tests/dependency_authorization.rs"]
mod dependency_authorization;
#[path = "tests/routing.rs"]
mod routing;
