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
fn controller_run_identity_is_stable_and_reviewer_identity_is_distinct() {
    let root = std::env::temp_dir().join(format!("review-identity-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let capture = CaptureHarness::new(&IdentityHarness, &root, &root, "task", None, Some("task-1"));
    let author = capture.current_execution_identity().unwrap();
    let reviewer = crate::harness::ReviewExecutionIdentity {
        execution_id: uuid::Uuid::new_v4().hyphenated().to_string(),
        session_id: uuid::Uuid::new_v4().hyphenated().to_string(),
    };
    assert_ne!(author.execution_id, reviewer.execution_id);
    assert_ne!(author.session_id, reviewer.session_id);
    assert!(uuid::Uuid::parse_str(&author.execution_id).is_ok());
    assert!(uuid::Uuid::parse_str(&author.session_id).is_ok());
    assert!(uuid::Uuid::parse_str(&reviewer.execution_id).is_ok());
    assert!(uuid::Uuid::parse_str(&reviewer.session_id).is_ok());
    let _ = std::fs::remove_dir_all(root);
}
