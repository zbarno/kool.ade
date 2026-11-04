use super::{record_revision, validate_record_collection, validate_record_path};
use crate::artifacts::planning_store::{PlanningStore, RecordRevisionCheck, StoreMode};

#[test]
fn lifecycle_record_collections_are_separate_and_revision_fenced() {
    for collection in [
        "lifecycle_features",
        "lifecycle_tasks",
        "lifecycle_events",
        "lifecycle_plans",
        "lifecycle_plan_reviews",
        "lifecycle_plan_authors",
        "lifecycle_plan_confirmations",
        "lifecycle_pr_work_items",
        "lifecycle_feature_delta_authorizations",
        "current_implementations",
        "current_implementation_history",
        "deployments",
        "opportunities",
        "opportunity_decisions",
        "self_improvements",
        "runtime-receipts",
    ] {
        let path = format!("state/{collection}/00000000-0000-0000-0000-000000000001.json");
        assert!(validate_record_collection(&format!("state/{collection}")).is_ok());
        assert!(validate_record_path(&path).is_ok());
    }
    assert!(validate_record_collection("state/lifecycle").is_err());
    assert!(
        validate_record_path("state/lifecycle/features/00000000-0000-0000-0000-000000000001.json")
            .is_err()
    );
}

#[test]
fn opportunity_and_self_improvement_record_paths_are_validated_by_shape() {
    assert!(validate_record_path("state/opportunity_discovery_settings.json").is_ok());
    assert!(validate_record_path("state/other_settings.json").is_err());
    assert!(validate_record_path("state/opportunity_quota/2026-10-13.json").is_ok());
    assert!(validate_record_path("state/opportunity_quota/2026-13-40.json").is_err());
    assert!(validate_record_path(
        "state/opportunity_dedup/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa.json"
    )
    .is_ok());
    assert!(
        validate_record_path("state/opportunity_dedup/00000000-0000-0000-0000-000000000001.json")
            .is_err()
    );
    assert!(validate_record_path(
        "state/self_improvement_events/00000000-0000-0000-0000-000000000001/00000000-0000-0000-0000-000000000002.json"
    )
    .is_ok());
    assert!(validate_record_path("state/project-runtime.json").is_ok());
    assert!(
        validate_record_collection(
            "state/self_improvement_events/00000000-0000-0000-0000-000000000001"
        )
        .is_ok()
    );
}

#[test]
fn project_runtime_manifest_revision_can_fence_atomic_target_publication() {
    let root = tempfile::tempdir().unwrap();
    let store = PlanningStore::new(uuid::Uuid::new_v4(), root.path(), StoreMode::ManagedLocal);
    store
        .atomic_write(
            "state/project-runtime.json",
            br#"{"schema_version":1,"revision":4}"#,
        )
        .unwrap();

    let path = "state/project-runtime.json";
    let initial_store_revision = store.revision().unwrap();
    assert_eq!(record_revision(&store, path).unwrap(), 4);
    store
        .transaction_with_revision_and_record_revisions(
            &[(
                path.into(),
                br#"{"schema_version":1,"revision":5}"#.to_vec(),
            )],
            Some(&initial_store_revision),
            &[RecordRevisionCheck {
                path: path.into(),
                expected_revision: 4,
            }],
        )
        .unwrap();

    // A competing publisher can update the manifest after a store snapshot is
    // taken. The record fence still rejects overwriting its newer revision.
    store
        .atomic_write(path, br#"{"schema_version":1,"revision":6}"#)
        .unwrap();
    let concurrent_store_revision = store.revision().unwrap();
    let stale_write = store.transaction_with_revision_and_record_revisions(
        &[(
            path.into(),
            br#"{"schema_version":1,"revision":5}"#.to_vec(),
        )],
        Some(&concurrent_store_revision),
        &[RecordRevisionCheck {
            path: path.into(),
            expected_revision: 4,
        }],
    );
    assert!(matches!(
        stale_write,
        Err(crate::artifacts::planning_store::StoreError::StaleRecordRevision { .. })
    ));
    assert_eq!(record_revision(&store, path).unwrap(), 6);
}
