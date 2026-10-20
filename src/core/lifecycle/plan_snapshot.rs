mod author;
mod confirmation;
mod hash;
mod model;
mod persistence;
mod readiness;
pub use model::{PlanRequirementV1, PlanSnapshotV1};
pub(crate) use persistence::recorded_review_decision;
#[cfg(test)]
pub(crate) use persistence::review_change;
pub(crate) use persistence::review_retry_available;
pub(crate) use persistence::{
    arbitration_change, from_feature_with_author, has_approved_review,
    has_current_user_confirmation, has_user_confirmation, is_persisted, load_persisted,
    plan_review_receipt, record_author, record_change, review_change_with_verdict,
    user_confirmation_change, user_confirmation_receipt,
};
pub(crate) use persistence::{latest_plan_arbitration, plan_review_verdict};
pub(crate) use readiness::require_current_readiness;

pub(crate) fn current_readiness(
    store: &crate::artifacts::planning_store::PlanningStore,
    snapshot: &PlanSnapshotV1,
) -> Result<(), String> {
    require_current_readiness(store, snapshot).map(|_| ())
}

pub(crate) fn confirmed_product_spec_hash(
    receipt: &crate::core::lifecycle::GateReceipt,
) -> Option<&str> {
    confirmation::confirmed_product_spec_hash(receipt)
}
mod content;
mod store;
#[cfg(test)]
use content::snapshot_uid;

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PlanAuthorProvenanceV1 {
    pub feature_uid: String,
    pub approval_contract_sha256: String,
    pub author_run_id: String,
    pub author_session_id: String,
}

#[cfg(test)]
#[path = "plan_snapshot/tests.rs"]
mod tests;
