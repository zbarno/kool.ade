use super::*;
use crate::artifacts::planning_store::PlanningStore;
use crate::core::workflow::TaskBatchRef;

fn fixture(name: &str, ids: &[&str]) -> (std::path::PathBuf, PlanningStore) {
    let root = std::env::temp_dir().join(format!(
        "koolade_workflow_records_{name}_{}-{}",
        std::process::id(),
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&root).unwrap();
    assert!(
        std::process::Command::new("git")
            .args(["init", "-q"])
            .current_dir(&root)
            .status()
            .unwrap()
            .success()
    );
    let store = PlanningStore::legacy_embedded(uuid::Uuid::new_v4(), &root);
    for id in ids {
        feature(&root, id, &format!("Feature {id}"));
    }
    (root, store)
}

fn feature(root: &std::path::Path, id: &str, title: &str) -> String {
    let directory = root
        .join(".koolade-packet/planning/changes")
        .join(format!("{id}-record-fixture"));
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("specification.md");
    let body = format!(
        "# {id}: {title}\n\n## Intent\n\nKeep this fixture stable.\n\n## Requirements\n\n- A fixture requirement.\n"
    );
    let identified =
        crate::artifacts::product_docs::identity::preserve_feature_identity(&path, id, &body)
            .unwrap();
    let identity = crate::domain::ArtifactIdentity::from_markdown(&identified)
        .unwrap()
        .unwrap();
    let marked = crate::domain::ChangeMetadata::write_markdown(
        &identified,
        &identity,
        crate::domain::ChangeStatus::Ready,
    )
    .unwrap();
    std::fs::write(path, &marked).unwrap();
    marked
}

fn approvals(ids: &[&str]) -> Workflow {
    Workflow {
        approved_features: ids
            .iter()
            .map(|id| ((*id).to_owned(), format!("approved {id}")))
            .collect(),
        ..Workflow::default()
    }
}

fn batch(store: &PlanningStore, id: &str, created_at_ms: u64) -> TaskBatchRef {
    let feature_uid = feature_uid(store, id).unwrap();
    let mut identity = crate::domain::ArtifactIdentity::new("batch", "fixture batch");
    identity.parent_uid = Some(feature_uid);
    TaskBatchRef {
        identity: Some(identity),
        feature: format!("Feature {id}"),
        directory: format!("planning/tasks/{id}-{created_at_ms}"),
        count: 1,
        created_at_ms,
    }
}

#[test]
fn feature_workflow_records_round_trip_without_a_monolithic_file() {
    let (root, store) = fixture("roundtrip", &["CHG-201", "CHG-202"]);
    let mut workflow = approvals(&["CHG-201", "CHG-202"]);
    workflow.task_batches = vec![
        batch(&store, "CHG-201", 10),
        batch(&store, "CHG-202", 20),
        batch(&store, "CHG-201", 30),
    ];
    crate::artifacts::task_docs::save_workflow(&store, &workflow).unwrap();

    let records = store.list_files(paths::WORKFLOW_RECORDS).unwrap();
    assert_eq!(records.len(), 2);
    assert!(!store.layout().workflow_state().exists());
    let loaded = crate::artifacts::task_docs::load_workflow(&store).unwrap();
    assert_eq!(loaded.approved_features, workflow.approved_features);
    assert_eq!(loaded.task_batches, workflow.task_batches);
    for id in ["CHG-201", "CHG-202"] {
        let uid = feature_uid(&store, id).unwrap();
        let record: WorkflowRecord = serde_json::from_slice(
            &store
                .read(format!("{}/{uid}.json", paths::WORKFLOW_RECORDS))
                .unwrap(),
        )
        .unwrap();
        assert_eq!(record.feature_id.as_deref(), Some(id));
        assert_eq!(record.uid, uid);
        assert_eq!(record.revision, 1);
    }
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn unrelated_feature_updates_coexist_and_same_feature_stale_update_is_rejected() {
    let (root, store) = fixture("concurrency", &["CHG-203", "CHG-204"]);
    crate::artifacts::task_docs::save_workflow(&store, &approvals(&["CHG-203", "CHG-204"]))
        .unwrap();
    let mut operator_a = crate::artifacts::task_docs::load_workflow(&store).unwrap();
    let mut operator_b = crate::artifacts::task_docs::load_workflow(&store).unwrap();
    operator_a
        .approved_features
        .insert("CHG-203".into(), "operator A update".into());
    operator_b
        .approved_features
        .insert("CHG-204".into(), "operator B update".into());
    crate::artifacts::task_docs::save_workflow(&store, &operator_a).unwrap();
    crate::artifacts::task_docs::save_workflow(&store, &operator_b).unwrap();
    let merged = crate::artifacts::task_docs::load_workflow(&store).unwrap();
    assert_eq!(merged.approved_features["CHG-203"], "operator A update");
    assert_eq!(merged.approved_features["CHG-204"], "operator B update");

    let mut first = crate::artifacts::task_docs::load_workflow(&store).unwrap();
    let mut stale = crate::artifacts::task_docs::load_workflow(&store).unwrap();
    first
        .approved_features
        .insert("CHG-203".into(), "winner".into());
    stale
        .approved_features
        .insert("CHG-203".into(), "stale overwrite".into());
    crate::artifacts::task_docs::save_workflow(&store, &first).unwrap();
    let error = crate::artifacts::task_docs::save_workflow(&store, &stale).unwrap_err();
    assert!(error.to_string().contains("expected revision"));
    let final_state = crate::artifacts::task_docs::load_workflow(&store).unwrap();
    assert_eq!(final_state.approved_features["CHG-203"], "winner");
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn legacy_workflow_migrates_to_records_and_keeps_recovery_source() {
    let (root, store) = fixture("migration", &["CHG-205"]);
    let legacy = serde_json::to_vec_pretty(&approvals(&["CHG-205"])).unwrap();
    store.atomic_write(paths::WORKFLOW, &legacy).unwrap();
    let loaded = crate::artifacts::task_docs::load_workflow(&store).unwrap();
    crate::artifacts::task_docs::save_workflow(&store, &loaded).unwrap();

    assert_eq!(store.read(paths::WORKFLOW).unwrap(), legacy);
    assert_eq!(store.list_files(paths::WORKFLOW_RECORDS).unwrap().len(), 1);
    assert_eq!(
        crate::artifacts::task_docs::load_workflow(&store)
            .unwrap()
            .approved_features["CHG-205"],
        "approved CHG-205"
    );
    let _ = std::fs::remove_dir_all(root);
}
