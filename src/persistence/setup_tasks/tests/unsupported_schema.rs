use std::fs;

use super::super::load_from_for_test;
use crate::persistence::setup_tasks::SetupAttemptV1;

#[test]
fn old_setup_store_schema_is_rejected_without_rewrite_or_import() {
    // Production boundary: current local store deserialization. A real file is
    // needed to prove rejection does not rewrite or import data. Uses only a
    // temporary directory; no Git, Docker, SSH, or external service. Low setup cost.
    let temporary = tempfile::tempdir().unwrap();
    let path = temporary.path().join("machine-setup/tasks.json");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let original =
        br#"{"schemaVersion":1,"latestScanGeneration":0,"tasks":{},"requiredImageProfiles":[]}"#;
    fs::write(&path, original).unwrap();

    assert!(load_from_for_test(&path).is_err());
    assert_eq!(fs::read(&path).unwrap(), original);
    assert_eq!(
        fs::read_dir(temporary.path()).unwrap().count(),
        1,
        "unsupported state must not create a second copy"
    );
}

#[test]
fn current_store_with_incomplete_task_is_rejected_without_rewrite() {
    // Production boundary: current task-record deserialization through local
    // storage. File I/O proves the rejected payload remains untouched; no
    // external services are needed and setup cost is low.
    let temporary = tempfile::tempdir().unwrap();
    let path = temporary.path().join("machine-setup/tasks.json");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let original = br#"{"schemaVersion":2,"latestScanGeneration":0,"tasks":{"setup:docker:daemon":{"schemaVersion":1,"id":"setup:docker:daemon","title":"Docker","explanation":"Docker unavailable","dependencies":[],"blockedBy":[],"state":"detected","createdAtUnixMs":1,"observedAtUnixMs":1,"scanGeneration":0,"verifiedAtUnixMs":null,"verifiedResult":null,"attempts":[],"events":[],"supersededBy":[]}},"requiredImageProfiles":[]}"#;
    fs::write(&path, original).unwrap();

    assert!(load_from_for_test(&path).is_err());
    assert_eq!(fs::read(&path).unwrap(), original);
}

#[test]
fn omitted_optional_attempt_fields_still_deserialize_as_none() {
    let attempt: SetupAttemptV1 = serde_json::from_str(
        r#"{"operationId":"operation-1","version":1,"requestedScopes":[],"state":"needs_approval","startedAtUnixMs":1,"updatedAtUnixMs":1}"#,
    )
    .unwrap();

    assert_eq!(attempt.owner_instance_id, None);
    assert_eq!(attempt.remediation_result, None);
    assert_eq!(attempt.image_preparation_receipt, None);
}
