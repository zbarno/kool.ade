use super::*;
use crate::artifacts::planning_store::StoreMode;

fn fixture() -> (tempfile::TempDir, PlanningStore, PlanSnapshotV1) {
    let root = tempfile::tempdir().unwrap();
    crate::artifacts::migration::bootstrap_product(root.path(), "Confirmation fixture").unwrap();
    let store = PlanningStore::new(
        uuid::Uuid::new_v4(),
        root.path().join(".koolade-packet"),
        StoreMode::ManagedLocal,
    );
    let markdown = crate::domain::ArtifactIdentity::preserve_markdown(
        "# CHG-981: Confirm\n\n## Intent\nConfirm current content.\n\n## Current Behavior\nHistorical confirmation.\n\n## Desired Behavior\nConfirm current product context.\n\n## Scope\nConfirmation.\n\n## Requirements\n- REQ-981: Preserve the approved target.\n\n## Acceptance Criteria\n- Approval remains auditable.\n",
        None, "CHG-981", "Confirm").unwrap();
    let identity = crate::domain::ArtifactIdentity::from_markdown(&markdown)
        .unwrap()
        .unwrap();
    let snapshot = PlanSnapshotV1::from_feature(
        "CHG-981",
        &identity,
        &markdown,
        &crate::core::workflow::feature_contract(&markdown),
    )
    .unwrap();
    (root, store, snapshot)
}

fn receipt(store: &PlanningStore, snapshot: &PlanSnapshotV1) -> GateReceipt {
    GateReceipt {
        receipt_uid: uuid::Uuid::new_v4().to_string(),
        subject_uid: snapshot.uid.clone(),
        subject_sha256: snapshot.content_hash.clone(),
        actor: GateActor::User,
        decision: GateDecision::Approved,
        recorded_at_unix_ms: 1,
        evidence_refs: vec![
            format!("plan:{}", snapshot.uid),
            format!(
                "product-spec:{}",
                crate::core::lifecycle::product_spec::current_product_hash(store).unwrap()
            ),
        ],
        prompt_sha256: None,
        prompt_manifest: None,
        reviewer_run_id: None,
        author_run_id: None,
        reviewer_session_id: None,
        author_session_id: None,
    }
}

fn save(store: &PlanningStore, snapshot: &PlanSnapshotV1, receipt: &GateReceipt) {
    let (path, bytes, check) = change(store, snapshot, receipt).unwrap().unwrap();
    store
        .transaction_with_record_revisions(&[(path, bytes)], &[check])
        .unwrap();
}

#[test]
// Production boundary: immutable confirmation files survive upgrade and explicit
// reconfirmation. A receipt validator cannot prove both records remain intact.
// Local temporary PlanningStore only; low setup cost, no services.
fn legacy_confirmation_is_preserved_when_current_product_is_explicitly_confirmed() {
    let (_root, store, snapshot) = fixture();
    let mut legacy = receipt(&store, &snapshot);
    legacy.evidence_refs.truncate(1);
    let path = confirmation_path(&snapshot.uid);
    let bytes = serde_json::to_vec(&ConfirmationRecord {
        schema_version: 1,
        uid: snapshot.uid.clone(),
        created_at_ms: 1,
        updated_at_ms: 1,
        revision: 1,
        receipt: legacy.clone(),
    })
    .unwrap();
    store
        .save_record(
            &path,
            &serde_json::from_slice::<serde_json::Value>(&bytes).unwrap(),
            0,
        )
        .unwrap();
    let before = store.read(&path).unwrap();
    assert_eq!(load_receipt(&store, &snapshot).unwrap(), Some(legacy));
    assert!(is_confirmed(&store, &snapshot).unwrap());
    assert!(!is_current(&store, &snapshot).unwrap());
    let current = receipt(&store, &snapshot);
    save(&store, &snapshot, &current);
    assert_eq!(load_receipt(&store, &snapshot).unwrap(), Some(current));
    assert!(is_current(&store, &snapshot).unwrap());
    assert_eq!(store.read(&path).unwrap(), before);
    assert_eq!(
        store
            .list_record_paths("state/lifecycle_plan_confirmations")
            .unwrap()
            .len(),
        2
    );
}

#[test]
// Same production boundary and local setup as the upgrade regression above.
fn product_change_creates_a_superseding_confirmation_without_changing_the_plan() {
    let (_root, store, snapshot) = fixture();
    let first = receipt(&store, &snapshot);
    save(&store, &snapshot, &first);
    let path = confirmation_path(
        &confirmation_uid(&snapshot, confirmed_product_spec_hash(&first).unwrap()).unwrap(),
    );
    let before = store.read(&path).unwrap();
    let docs = crate::artifacts::product_docs::load_documents(&store)
        .unwrap()
        .unwrap();
    let doc = &docs[0];
    let relative = store
        .layout()
        .product_root()
        .join(&doc.module.path)
        .strip_prefix(store.layout().root())
        .unwrap()
        .to_string_lossy()
        .into_owned();
    store
        .transaction(
            &[(
                relative,
                format!("{}\nUpdated product context.\n", doc.content).into_bytes(),
            )],
            None,
        )
        .unwrap();
    let second = receipt(&store, &snapshot);
    assert_ne!(first.evidence_refs, second.evidence_refs);
    assert!(!is_current(&store, &snapshot).unwrap());
    assert_eq!(
        load_receipt(&store, &snapshot).unwrap(),
        Some(first.clone())
    );
    assert!(change(&store, &snapshot, &first).is_err());
    save(&store, &snapshot, &second);
    assert_eq!(load_receipt(&store, &snapshot).unwrap(), Some(second));
    assert!(is_current(&store, &snapshot).unwrap());
    assert_eq!(store.read(&path).unwrap(), before);
}

#[test]
// Production boundary: two shared-store clients race to confirm the same
// immutable candidate; a single record revision must select one durable winner.
// Receipt-only unit checks cannot prove the cross-client transaction fence.
// External services: none. Setup cost: local temporary shared store.
fn competing_device_confirmations_have_one_durable_winner() {
    let (_root, store, snapshot) = fixture();
    let (snapshot_path, snapshot_bytes, snapshot_check) =
        crate::core::lifecycle::plan_snapshot::record_change(&store, &snapshot)
            .unwrap()
            .unwrap();
    store
        .transaction_with_record_revisions(&[(snapshot_path, snapshot_bytes)], &[snapshot_check])
        .unwrap();

    let device_a = PlanningStore::new(
        store.project_id,
        store.root.clone(),
        StoreMode::ManagedLocal,
    );
    let device_b = PlanningStore::new(
        store.project_id,
        store.root.clone(),
        StoreMode::ManagedLocal,
    );
    let receipt_a = receipt(&device_a, &snapshot);
    let receipt_b = receipt(&device_b, &snapshot);
    let (path_a, bytes_a, check_a) = change(&device_a, &snapshot, &receipt_a).unwrap().unwrap();
    let (path_b, bytes_b, check_b) = change(&device_b, &snapshot, &receipt_b).unwrap().unwrap();
    assert_eq!(path_a, path_b);

    let (result_a, result_b) = std::thread::scope(|scope| {
        let a = scope
            .spawn(|| device_a.transaction_with_record_revisions(&[(path_a, bytes_a)], &[check_a]));
        let b = scope
            .spawn(|| device_b.transaction_with_record_revisions(&[(path_b, bytes_b)], &[check_b]));
        (a.join().unwrap(), b.join().unwrap())
    });
    assert_ne!(result_a.is_ok(), result_b.is_ok());

    let winner = load_receipt(&store, &snapshot).unwrap().unwrap();
    assert!(winner == receipt_a || winner == receipt_b);
    assert!(is_current(&store, &snapshot).unwrap());
    assert!(change(&store, &snapshot, &winner).unwrap().is_none());
}
