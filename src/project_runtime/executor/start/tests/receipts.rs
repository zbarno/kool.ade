use super::*;

#[test]
// Production boundary: direct Start execution must require the latest Test
// receipt for each declared test recipe and the exact source revision. Queue
// order alone cannot protect callers of ProjectRuntimeExecutor::execute.
fn direct_start_requires_current_passing_test_receipts() {
    let mut target = target();
    target.test_recipe_refs = vec!["test-web".into()];
    let source_sha = "a".repeat(40);
    let passed = RuntimeReceiptV1 {
        schema_version: 1,
        receipt_id: "test-pass".into(),
        operation: RuntimeOperation::Test,
        task_id: target.owning_task_id.clone(),
        target_id: target.target_id.clone(),
        source_sha: source_sha.clone(),
        recipe_id: Some("test-web".into()),
        state: RuntimeReceiptState::Passed,
        exit_code: Some(0),
        artifact_hashes: Default::default(),
        prepared_dependency_ids: Vec::new(),
        log_reference: None,
        observed_at_unix_ms: 10,
    };

    assert!(require_passed_test_receipts(&target, &[], &source_sha).is_err());
    assert!(
        require_passed_test_receipts(&target, std::slice::from_ref(&passed), &source_sha).is_ok()
    );

    let failed_latest = RuntimeReceiptV1 {
        receipt_id: "test-failed".into(),
        state: RuntimeReceiptState::Failed,
        observed_at_unix_ms: 11,
        ..passed.clone()
    };
    assert!(
        require_passed_test_receipts(&target, &[passed.clone(), failed_latest], &source_sha)
            .is_err()
    );
    assert!(require_passed_test_receipts(&target, &[passed], &"b".repeat(40)).is_err());
}
