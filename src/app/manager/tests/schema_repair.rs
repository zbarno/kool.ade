use super::*;
use crate::harness::{AiHarness, HarnessOutcome, PlanningRequest};
use std::sync::{Arc, Mutex};

struct RepairHarness(Arc<Mutex<Vec<String>>>);
impl AiHarness for RepairHarness {
    fn label(&self) -> String {
        "schema-repair-fixture".into()
    }
    fn check_available(&self) -> Result<String, crate::error::AppError> {
        Ok(self.label())
    }
    fn execute(&self, request: &PlanningRequest) -> Result<HarnessOutcome, crate::error::AppError> {
        let mut prompts = self.0.lock().unwrap();
        prompts.push(request.prompt_body.clone());
        Ok(HarnessOutcome {
            final_text: if prompts.len() == 1 {
                r#"{"schema_version":2,"summary":"Current update","proposed_actions":[],"opportunities":[]}"#
            } else {
                r#"{"summary":"Current update","proposed_actions":[],"opportunities":[]}"#
            }.into(), envelope: None, stderr_tail: String::new(),
        })
    }
}

/// Production boundary: Manager harness responses pass strict catalog validation
/// and bounded repair before publication. A validator test alone cannot prove
/// the second invocation receives correction feedback. Local Git/temp files and
/// a mock provider only; no Docker/network; low setup cost.
#[test]
fn manager_repairs_unknown_fields_with_specific_feedback() {
    let mut environment = crate::test_support::ProcessEnvironmentGuard::new();
    let home = tempfile::tempdir().unwrap();
    environment.set("KOOLADE_HOME", home.path());
    let (_temp, project, _) = opportunities::initialized_repository();
    let prompts = Arc::new(Mutex::new(Vec::new()));
    let manager = Manager::start(
        &project,
        &["manager-refresh".into()],
        Box::new(RepairHarness(prompts.clone())),
    );
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        if let Some(result) = manager.result() {
            assert!(result.is_ok(), "{:?}", result.err());
            break;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(5));
    }
    let prompts = prompts.lock().unwrap();
    assert!(prompts[1].contains("Application validation rejected the previous response"));
    assert!(prompts[1].contains("exactly these top-level fields"));
    assert!(prompts[1].contains("Do not add schema_version"));
}
