use crate::{
    artifacts::planning_store::{RecordRevisionCheck, StoreError},
    core::{lifecycle::PlanSnapshotV1, lifecycle::plan_snapshot::user_confirmation_change},
};

pub(super) fn record_user_approval(
    store: &crate::artifacts::planning_store::PlanningStore,
    snapshot: &PlanSnapshotV1,
) -> Result<Option<(String, Vec<u8>, RecordRevisionCheck)>, StoreError> {
    let product_hash = crate::core::lifecycle::product_spec::current_product_hash(store)?;
    let receipt = crate::core::lifecycle::GateReceipt {
        receipt_uid: uuid::Uuid::new_v4().hyphenated().to_string(),
        subject_uid: snapshot.uid.clone(),
        subject_sha256: snapshot.content_hash.clone(),
        actor: crate::core::lifecycle::GateActor::User,
        decision: crate::core::lifecycle::GateDecision::Approved,
        recorded_at_unix_ms: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
            .min(u64::MAX as u128) as u64,
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
    user_confirmation_change(store, snapshot, &receipt)
}
