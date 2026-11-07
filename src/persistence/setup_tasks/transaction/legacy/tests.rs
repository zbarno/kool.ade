use super::*;

fn task(id: &str, domain: &str) -> serde_json::Value {
    serde_json::json!({
        "schemaVersion":1, "id":id, "title":"Verify runtime", "explanation":"Historical setup record",
        "nextStep":"Recheck readiness", "dependencies":[], "blockedBy":[], "state":"verified",
        "createdAtUnixMs":1, "observedAtUnixMs":2, "scanGeneration":1, "verifiedAtUnixMs":2,
        "verifiedResult":{"evidenceDomain":domain,"scanGeneration":1,"verifiedAtUnixMs":2},
        "attempts":[],"events":[],"supersededBy":[],"legacySourceId":null
    })
}

#[test]
fn legacy_aliases_load_but_old_rootless_evidence_cannot_grant_readiness() {
    let mut child = task("setup:images:general:amd64", "images:general:amd64");
    child["dependencies"] = serde_json::json!([{"prerequisiteId":"setup:docker:rootless", "resolution":{"kind":"satisfied_by_task", "scan_generation":1}}]);
    let value = serde_json::json!({"schemaVersion":2,"latestScanGeneration":1,"tasks":{
        "setup:docker:rootless":task("setup:docker:rootless","docker"),
        "setup:images:general:amd64":child
    }});
    let store = decode(&serde_json::to_vec(&value).unwrap()).unwrap();
    let rootless = &store.tasks["setup:docker:rootless"];
    assert_eq!(rootless.state, State::NeedsVerification);
    assert!(rootless.verified_result.is_none());
    let image = &store.tasks["setup:images:general:amd64"];
    assert_eq!(image.state, State::BlockedByPrerequisite);
    assert!(image.verified_result.is_none());
    assert_eq!(image.blocked_by, ["setup:docker:rootless"]);
    assert!(!store.has_valid_image_preparation_receipt("setup:images:general:amd64"));
}

#[test]
fn compatibility_does_not_accept_unknown_fields_or_nonempty_source_mappings() {
    let mut value = serde_json::json!({"schemaVersion":2,"latestScanGeneration":1,"tasks":{
        "setup:cli:pi:capability":task("setup:cli:pi:capability","cli:pi")
    }});
    assert!(decode(&serde_json::to_vec(&value).unwrap()).is_ok());
    value["tasks"]["setup:cli:pi:capability"]["legacySourceId"] = "old-source".into();
    assert!(decode(&serde_json::to_vec(&value).unwrap()).is_err());
    value["tasks"]["setup:cli:pi:capability"]["legacySourceId"] = serde_json::Value::Null;
    value["tasks"]["setup:cli:pi:capability"]["unrecognizedApproval"] = true.into();
    assert!(decode(&serde_json::to_vec(&value).unwrap()).is_err());
}

#[test]
fn malformed_legacy_task_identity_returns_an_error_before_graph_traversal() {
    let value = serde_json::json!({"schemaVersion":2,"latestScanGeneration":1,"tasks":{
        "setup:docker:rootless":task("setup:docker:install","docker")
    }});
    assert!(decode(&serde_json::to_vec(&value).unwrap()).is_err());
}

#[test]
fn historical_rootless_proof_is_retained_as_invalidated_history() {
    let mut historical = task("setup:docker:rootless", "docker");
    historical["attempts"] = serde_json::json!([{
        "operationId":"00000000-0000-4000-8000-000000000001", "version":1,
        "ownerInstanceId":null,"requestedScopes":["setup:docker:rootless"],
        "authorizationBinding":null,"authorizationHistory":null,"state":"verified",
        "startedAtUnixMs":1,"updatedAtUnixMs":2,
        "verifiedResult":{"evidenceDomain":"docker","scanGeneration":1,"verifiedAtUnixMs":2},
        "remediationResult":null,"imagePreparationReceipt":null,"failure":null
    }]);
    let value = serde_json::json!({"schemaVersion":2,"latestScanGeneration":1,"tasks":{
        "setup:docker:rootless":historical
    }});
    let store = decode(&serde_json::to_vec(&value).unwrap()).unwrap();
    let task = &store.tasks["setup:docker:rootless"];
    assert_eq!(task.state, State::NeedsVerification);
    assert_eq!(task.attempts.len(), 1);
    assert_eq!(task.attempts[0].state, AttemptState::Failed);
    assert!(task.attempts[0].verified_result.is_none());
}

#[test]
fn blocked_dependent_image_cannot_reuse_a_historical_smoke_receipt() {
    use crate::{
        execution::docker::images::{ImageLock, RuntimeProfile},
        persistence::setup_tasks::ImagePreparationReceiptV1,
    };
    let engine = "b".repeat(64);
    let id = format!("setup:images:general:amd64:{engine}");
    let domain = format!("images:general:amd64:{engine}");
    let selected = ImageLock::bundled()
        .unwrap()
        .select(RuntimeProfile::General, "amd64")
        .unwrap();
    let receipt = ImagePreparationReceiptV1 {
        profile: RuntimeProfile::General,
        architecture: "amd64".into(),
        image_digest: selected.digest,
        engine_identity_fingerprint: engine,
        successful_smoke_at_unix_ms: 2,
    };
    let mut image = task(&id, &domain);
    image["dependencies"] = serde_json::json!([{"prerequisiteId":"setup:docker:rootless", "resolution":{"kind":"satisfied_by_task", "scan_generation":1}}]);
    image["attempts"] = serde_json::json!([{
        "operationId":"00000000-0000-4000-8000-000000000002", "version":1,
        "ownerInstanceId":null,"requestedScopes":[id],"authorizationBinding":null,
        "authorizationHistory":null,"state":"verified","startedAtUnixMs":1,"updatedAtUnixMs":2,
        "verifiedResult":{"evidenceDomain":domain,"scanGeneration":1,"verifiedAtUnixMs":2},
        "remediationResult":null,"imagePreparationReceipt":receipt,"failure":null
    }]);
    let mut tasks = serde_json::Map::new();
    tasks.insert(
        "setup:docker:rootless".into(),
        task("setup:docker:rootless", "docker"),
    );
    tasks.insert(id.clone(), image);
    let value = serde_json::json!({"schemaVersion":2,"latestScanGeneration":1,"tasks":tasks});
    let store = decode(&serde_json::to_vec(&value).unwrap()).unwrap();
    assert_eq!(store.tasks[&id].state, State::BlockedByPrerequisite);
    assert!(
        store.tasks[&id].attempts[0]
            .image_preparation_receipt
            .is_some()
    );
    assert!(!store.has_valid_image_preparation_receipt(&id));
}
