use super::*;

#[test]
fn two_device_approvals_for_one_candidate_commit_only_once() {
    // Production boundary: two separately loaded PlanningStore clients race to create the same approval record.
    // Invariant: only one exact user receipt wins; a stale second device cannot overwrite or duplicate it.
    // External dependencies: none. Setup cost: one temporary project artifact store.
    use crate::artifacts::planning_store::{PlanningStore, StoreMode};
    use crate::core::lifecycle::{GateActor, GateDecision, GateReceipt};

    let root =
        std::env::temp_dir().join(format!("plan-confirmation-race-{}", uuid::Uuid::new_v4()));
    crate::artifacts::migration::bootstrap_product(&root, "Plan Confirmation Race").unwrap();
    let project_id = uuid::Uuid::new_v4();
    let path = root.clone();
    let first = PlanningStore::new(project_id, path.clone(), StoreMode::ManagedLocal);
    let second = PlanningStore::new(project_id, path, StoreMode::ManagedLocal);
    let (identity, markdown) = identified(&spec());
    let contract = crate::core::workflow::feature_contract(&markdown);
    let snapshot =
        PlanSnapshotV1::from_feature("CHG-021", &identity, &markdown, &contract).unwrap();
    persist_snapshot(&first, &snapshot);
    let expected_revision = first.revision().unwrap();
    let product_hash = crate::core::lifecycle::product_spec::current_product_hash(&first).unwrap();
    let make_receipt = || GateReceipt {
        receipt_uid: uuid::Uuid::new_v4().hyphenated().to_string(),
        subject_uid: snapshot.uid.clone(),
        subject_sha256: snapshot.content_hash.clone(),
        actor: GateActor::User,
        decision: GateDecision::Approved,
        recorded_at_unix_ms: 1,
        evidence_refs: vec![
            format!("plan:{}", snapshot.uid),
            format!("product-spec:{product_hash}"),
        ],
        prompt_sha256: None,
        prompt_manifest: None,
        reviewer_run_id: None,
        author_run_id: None,
        reviewer_session_id: None,
        author_session_id: None,
    };
    let first_change = user_confirmation_change(&first, &snapshot, &make_receipt())
        .unwrap()
        .unwrap();
    let second_change = user_confirmation_change(&second, &snapshot, &make_receipt())
        .unwrap()
        .unwrap();
    first
        .transaction_with_revision_and_record_revisions(
            &[(first_change.0, first_change.1)],
            Some(&expected_revision),
            &[first_change.2],
        )
        .unwrap();
    assert!(
        second
            .transaction_with_revision_and_record_revisions(
                &[(second_change.0, second_change.1)],
                Some(&expected_revision),
                &[second_change.2],
            )
            .is_err()
    );
    assert!(
        crate::core::lifecycle::plan_snapshot::has_user_confirmation(&first, &snapshot).unwrap()
    );
    assert_eq!(
        first
            .list_record_paths("state/lifecycle_plan_confirmations")
            .unwrap()
            .len(),
        1
    );
    let _ = std::fs::remove_dir_all(root);
}
