use super::*;

struct IdentityHarness;

impl AiHarness for IdentityHarness {
    fn label(&self) -> String {
        "identity-test".into()
    }
    fn check_available(&self) -> Result<String, crate::error::AppError> {
        Ok("available".into())
    }
    fn execute(&self, _: &PlanningRequest) -> Result<HarnessOutcome, crate::error::AppError> {
        unreachable!("identity test does not execute a provider")
    }
}

#[test]
fn capture_harness_execution_identity_is_stable_and_well_formed() {
    let root = tempfile::tempdir().unwrap();
    let capture = CaptureHarness::new(
        &IdentityHarness,
        root.path(),
        root.path(),
        "task",
        None,
        Some("task-1"),
    );
    let identity = capture.current_execution_identity().unwrap();
    assert_eq!(Some(identity.clone()), capture.current_execution_identity());
    assert!(uuid::Uuid::parse_str(&identity.execution_id).is_ok());
    assert!(uuid::Uuid::parse_str(&identity.session_id).is_ok());
}
