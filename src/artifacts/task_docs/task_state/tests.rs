use super::*;
use crate::artifacts::planning_store::{PlanningStore, StoreError, StoreMode};
use crate::domain::ArtifactIdentity;

fn linkage(repository_id: &str) -> TaskExecutionLinkage {
    TaskExecutionLinkage {
        repository_id: repository_id.into(),
        ..Default::default()
    }
}

#[test]
fn task_execution_status_is_revised_per_record_and_stale_updates_are_rejected() {
    let root = std::env::temp_dir().join(format!("koolade-task-state-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let store = PlanningStore::new(uuid::Uuid::new_v4(), &root, StoreMode::ManagedLocal);
    let identity = ArtifactIdentity {
        uid: uuid::Uuid::new_v4().to_string(),
        display_id: "TASK-001".into(),
        title: "Implement feature".into(),
        parent_uid: Some(uuid::Uuid::new_v4().to_string()),
    };
    let metadata =
        super::super::metadata::TaskMetadata::new(&identity, "repo-main", vec![]).unwrap();
    let (path, bytes, check) = create(&metadata).unwrap();
    store
        .transaction_with_record_revisions(&[(path, bytes)], &[check])
        .unwrap();

    let initial = load(&store, &metadata.uid).unwrap().unwrap();
    assert_eq!(initial.status, WorkStatus::Todo);
    assert_eq!(initial.revision, 1);
    let updated = update_execution_status(
        &store,
        &metadata,
        initial.revision,
        WorkStatus::InProgress,
        "implementing",
        &linkage("repo-main"),
    )
    .unwrap();
    assert_eq!(updated.status, WorkStatus::InProgress);
    assert_eq!(updated.execution_status.as_deref(), Some("implementing"));
    assert_eq!(updated.revision, 2);

    let stale = update_execution_status(
        &store,
        &metadata,
        initial.revision,
        WorkStatus::NeedsAttention,
        "blocked",
        &linkage("repo-main"),
    );
    assert!(matches!(
        stale,
        Err(error) if error.downcast_ref::<StoreError>().is_some_and(|error|
            matches!(error, StoreError::StaleRecordRevision { .. }))
    ));
    let restored = load(&store, &metadata.uid).unwrap().unwrap();
    assert_eq!(restored.status, WorkStatus::InProgress);
    assert_eq!(restored.execution_status.as_deref(), Some("implementing"));
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn execution_linkage_round_trips_and_unsupported_schema_is_preserved() {
    let root = std::env::temp_dir().join(format!("koolade-task-linkage-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let store = PlanningStore::new(uuid::Uuid::new_v4(), &root, StoreMode::ManagedLocal);
    let identity = ArtifactIdentity {
        uid: uuid::Uuid::new_v4().to_string(),
        display_id: "TASK-002".into(),
        title: "Ship the release task".into(),
        parent_uid: Some(uuid::Uuid::new_v4().to_string()),
    };
    let metadata = super::super::metadata::TaskMetadata::new(&identity, "repo-api", vec![])
        .unwrap()
        .with_branch_targets(Some(&crate::core::workflow::BranchTargets {
            source: "release/1.2".into(),
            destination: "develop".into(),
        }))
        .unwrap();
    let (path, bytes, check) = create(&metadata).unwrap();
    store
        .transaction_with_record_revisions(&[(path.clone(), bytes)], &[check])
        .unwrap();

    let initial = load(&store, &metadata.uid).unwrap().unwrap();
    assert_eq!(initial.source_branch.as_deref(), Some("release/1.2"));
    assert_eq!(initial.destination_branch.as_deref(), Some("develop"));
    let linked = TaskExecutionLinkage {
        repository_id: "repo-api".into(),
        source_branch: Some("release/1.2".into()),
        source_sha: Some("a".repeat(40)),
        destination_branch: Some("develop".into()),
        implementation_branch: Some("koolade/task-002".into()),
        publication_status: ExecutionPublicationStatus::Shared,
        pull_requests: vec![PullRequestLinkage {
            repository_id: "repo-api".into(),
            number: 27,
            url: "https://github.com/fixture/api/pull/27".into(),
            head_sha: "b".repeat(40),
            base_branch: "develop".into(),
            merge_sha: None,
            status: ExecutionPublicationStatus::Shared,
        }],
    };
    let saved = update_execution_status(
        &store,
        &metadata,
        initial.revision,
        WorkStatus::InReview,
        "awaiting_review",
        &linked,
    )
    .unwrap();
    assert_eq!(saved.publication_status, ExecutionPublicationStatus::Shared);
    assert_eq!(saved.source_sha.as_deref(), Some("a".repeat(40).as_str()));
    assert_eq!(saved.pull_requests, linked.pull_requests);
    let encoded = serde_json::to_value(&saved).unwrap();
    assert_eq!(encoded["repositoryId"], "repo-api");
    assert_eq!(encoded["sourceBranch"], "release/1.2");
    assert_eq!(encoded["sourceSha"], "a".repeat(40));
    assert_eq!(encoded["destinationBranch"], "develop");
    assert_eq!(encoded["implementationBranch"], "koolade/task-002");
    assert_eq!(encoded["pullRequests"][0]["number"], 27);
    assert_eq!(encoded["pullRequests"][0]["baseBranch"], "develop");
    assert_eq!(encoded["pullRequests"][0]["status"], "shared");

    let old = serde_json::json!({
        "schemaVersion": 1,
        "uid": metadata.uid,
        "revision": 1,
        "createdAtMs": 1,
        "updatedAtMs": 1,
        "data": {
            "batchUid": metadata.batch_uid,
            "repositoryId": "repo-api",
            "dependencyUids": [],
            "status": "todo"
        }
    });
    store
        .atomic_write(
            format!(
                "{}/{task_uid}.json",
                paths::TASK_STATES,
                task_uid = metadata.uid
            ),
            &serde_json::to_vec_pretty(&old).unwrap(),
        )
        .unwrap();
    let old_bytes = serde_json::to_vec_pretty(&old).unwrap();
    let error = load(&store, &metadata.uid).unwrap_err();
    assert!(error.to_string().contains("Unsupported task state schema"));
    assert_eq!(
        store
            .read(format!(
                "{}/{task_uid}.json",
                paths::TASK_STATES,
                task_uid = metadata.uid
            ))
            .unwrap(),
        old_bytes
    );
    std::fs::remove_dir_all(root).unwrap();
}
