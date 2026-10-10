use super::*;
use crate::artifacts::planning_store::{PlanningStore, StoreError, StoreMode};
use crate::domain::ArtifactIdentity;

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
fn existing_task_documents_backfill_identity_state_and_dependency_records() {
    let root = std::env::temp_dir().join(format!("koolade-task-backfill-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    assert!(
        std::process::Command::new("git")
            .args(["init", "-q"])
            .current_dir(&root)
            .status()
            .unwrap()
            .success()
    );
    let store = PlanningStore::new(uuid::Uuid::new_v4(), &root, StoreMode::ManagedLocal);
    let directory = "planning/tasks/fixture-batch";
    let batch = ArtifactIdentity::new("BATCH-001", "Fixture batch");
    store
        .atomic_write(
            format!("{directory}/README.md"),
            &identity_markdown(&batch, "# Fixture batch\n"),
        )
        .unwrap();
    let first = task_identity("TASK-001", &batch.uid, "First task");
    let second = task_identity("TASK-002", &batch.uid, "Second task");
    let first_markdown = String::from_utf8(identity_markdown(
        &first,
        "# First task\n\n## Dependencies\n\nNone.\n",
    ))
    .unwrap();
    let second_markdown = identity_markdown(
        &second,
        "# Second task\n\n## Dependencies\n\n- [First task](001-first.md)\n",
    );
    store
        .atomic_write(
            format!("{directory}/001-first.md"),
            first_markdown.as_bytes(),
        )
        .unwrap();
    store
        .atomic_write(format!("{directory}/002-second.md"), &second_markdown)
        .unwrap();
    let workflow = crate::core::workflow::Workflow {
        task_batches: vec![crate::core::workflow::TaskBatchRef {
            identity: Some(batch.clone()),
            feature: "Fixture batch".into(),
            directory: directory.into(),
            count: 2,
            created_at_ms: 1,
        }],
        ..Default::default()
    };

    let (snapshot, planned, checks) = super::plan_backfill(&store, &workflow).unwrap();
    assert_eq!(planned.len(), 4);
    assert!(
        crate::artifacts::transaction::apply_store_with_record_revisions_interruption_for_test(
            &store,
            &planned,
            Some(&snapshot),
            &checks,
            1,
        )
        .is_err()
    );
    assert!(store.recover().unwrap());
    assert!(super::load(&store, &first.uid).unwrap().is_none());

    let changed = super::backfill_missing(&store, &workflow).unwrap();
    assert_eq!(changed.len(), 4);
    assert!(store.read(format!("{directory}/001-first.md")).is_ok());
    assert_eq!(
        super::super::metadata::visible_content(
            &String::from_utf8(store.read(format!("{directory}/001-first.md")).unwrap()).unwrap()
        ),
        super::super::metadata::visible_content(&first_markdown)
    );
    let documents = super::super::board::load_board(&store, &workflow);
    assert_eq!(documents.len(), 2);
    assert_eq!(documents[0].identity.as_ref().unwrap().uid, first.uid);
    assert_eq!(documents[1].identity.as_ref().unwrap().uid, second.uid);
    assert!(documents.iter().all(|document| {
        document.metadata.is_some()
            && document
                .task_state
                .as_ref()
                .is_some_and(|state| state.status == WorkStatus::Todo)
    }));
    assert_eq!(
        documents[1].metadata.as_ref().unwrap().dependency_uids,
        vec![first.uid]
    );
    assert!(
        super::backfill_missing(&store, &workflow)
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        store.list_record_paths(paths::TASK_STATES).unwrap().len(),
        2
    );
    std::fs::remove_dir_all(root).unwrap();
}

fn task_identity(display_id: &str, batch_uid: &str, title: &str) -> ArtifactIdentity {
    let mut identity = ArtifactIdentity::new(display_id, title);
    identity.parent_uid = Some(batch_uid.into());
    identity
}

fn identity_markdown(identity: &ArtifactIdentity, body: &str) -> Vec<u8> {
    format!(
        "<!-- koolade-artifact-id:v1 {} -->\n\n{body}",
        serde_json::to_string(identity).unwrap()
    )
    .into_bytes()
}
