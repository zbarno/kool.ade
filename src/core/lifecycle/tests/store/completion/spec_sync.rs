use crate::{
    artifacts::planning_store::PlanningStore,
    core::lifecycle::{LifecycleStore, SpecReconciliationStatusV1, SpecRevisionDraftV1},
};
use sha2::Digest as _;

pub(super) fn append_current_revision(
    lifecycle: &LifecycleStore<'_>,
    store: &PlanningStore,
    feature_uid: &str,
    expected_sequence: u64,
    origin_events: Vec<String>,
    implementation_head_sha: Option<String>,
) {
    let feature = lifecycle.load_feature(feature_uid).unwrap();
    let origin_events = if origin_events.is_empty() {
        lifecycle
            .events_for_feature(feature_uid)
            .unwrap()
            .into_iter()
            .map(|event| event.uid)
            .collect()
    } else {
        origin_events
    };
    let expected_sequence = lifecycle
        .load_spec_revision(feature_uid)
        .unwrap()
        .map_or(expected_sequence, |revision| revision.seq);
    let product = crate::artifacts::product_docs::render_product(store)
        .unwrap()
        .unwrap();
    let content_hash = format!(
        "{:x}",
        sha2::Sha256::digest(serde_json::to_vec(&Some(product)).unwrap())
    );
    lifecycle
        .append_spec_revision(
            feature_uid,
            expected_sequence,
            SpecRevisionDraftV1 {
                no_change_reason: Some(
                    "Synthetic fixture records an explicit unchanged-specification result.".into(),
                ),
                content_hash,
                source_revision: feature.revision,
                produced_by_run_id: None,
                origin_events,
                decision_refs: feature.decision_refs,
                delta_approval_refs: Vec::new(),
                approved_target_hash: feature.approved_plan_sha256,
                implementation_head_sha,
                reconciliation_status: SpecReconciliationStatusV1::Synchronized,
                outcome_reason: None,
                deltas: Vec::new(),
            },
        )
        .unwrap();
}
