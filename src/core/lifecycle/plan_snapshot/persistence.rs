use crate::domain::ArtifactIdentity;

use super::{PlanAuthorProvenanceV1, PlanSnapshotV1, author, confirmation, store};

pub(crate) fn record_author(
    planning_store: &crate::artifacts::planning_store::PlanningStore,
    provenance: &PlanAuthorProvenanceV1,
) -> Result<(), crate::artifacts::planning_store::StoreError> {
    author::record(planning_store, provenance)
}

pub(crate) fn from_feature_with_author(
    planning_store: &crate::artifacts::planning_store::PlanningStore,
    feature_id: &str,
    identity: &ArtifactIdentity,
    markdown: &str,
    approval_contract: &str,
) -> Result<PlanSnapshotV1, String> {
    let mut snapshot =
        PlanSnapshotV1::from_feature(feature_id, identity, markdown, approval_contract)?;
    if let Some(author) = author::load(planning_store, &snapshot.feature_uid)?
        && author.approval_contract_sha256 == snapshot.approval_contract_sha256
    {
        snapshot.author_run_id = Some(author.author_run_id);
        snapshot.author_session_id = Some(author.author_session_id);
    }
    snapshot.validate()?;
    Ok(snapshot)
}

pub(crate) fn record_change(
    planning_store: &crate::artifacts::planning_store::PlanningStore,
    snapshot: &PlanSnapshotV1,
) -> Result<
    Option<(
        String,
        Vec<u8>,
        crate::artifacts::planning_store::RecordRevisionCheck,
    )>,
    crate::artifacts::planning_store::StoreError,
> {
    store::record_change(planning_store, snapshot)
}

pub(crate) fn is_persisted(
    planning_store: &crate::artifacts::planning_store::PlanningStore,
    snapshot: &PlanSnapshotV1,
) -> Result<bool, crate::artifacts::planning_store::StoreError> {
    store::is_persisted(planning_store, snapshot)
}

pub(crate) fn review_change(
    planning_store: &crate::artifacts::planning_store::PlanningStore,
    snapshot: &PlanSnapshotV1,
    receipt: &crate::core::lifecycle::GateReceipt,
) -> Result<
    Option<(
        String,
        Vec<u8>,
        crate::artifacts::planning_store::RecordRevisionCheck,
    )>,
    crate::artifacts::planning_store::StoreError,
> {
    store::review_change(planning_store, snapshot, receipt)
}

pub(crate) fn has_approved_review(
    planning_store: &crate::artifacts::planning_store::PlanningStore,
    snapshot: &PlanSnapshotV1,
) -> Result<bool, crate::artifacts::planning_store::StoreError> {
    store::has_approved_review(planning_store, snapshot)
}

#[cfg(not(test))]
pub(crate) fn recorded_review_decision(
    planning_store: &crate::artifacts::planning_store::PlanningStore,
    snapshot: &PlanSnapshotV1,
) -> Result<
    Option<crate::core::lifecycle::GateDecision>,
    crate::artifacts::planning_store::StoreError,
> {
    store::recorded_review_decision(planning_store, snapshot)
}

pub(crate) fn user_confirmation_change(
    planning_store: &crate::artifacts::planning_store::PlanningStore,
    snapshot: &PlanSnapshotV1,
    receipt: &crate::core::lifecycle::GateReceipt,
) -> Result<
    Option<(
        String,
        Vec<u8>,
        crate::artifacts::planning_store::RecordRevisionCheck,
    )>,
    crate::artifacts::planning_store::StoreError,
> {
    confirmation::change(planning_store, snapshot, receipt)
}

pub(crate) fn has_user_confirmation(
    planning_store: &crate::artifacts::planning_store::PlanningStore,
    snapshot: &PlanSnapshotV1,
) -> Result<bool, crate::artifacts::planning_store::StoreError> {
    confirmation::is_confirmed(planning_store, snapshot)
}

pub(crate) fn user_confirmation_receipt(
    planning_store: &crate::artifacts::planning_store::PlanningStore,
    snapshot: &PlanSnapshotV1,
) -> Result<Option<crate::core::lifecycle::GateReceipt>, crate::artifacts::planning_store::StoreError>
{
    confirmation::load_receipt(planning_store, snapshot)
}

pub(crate) fn plan_review_receipt(
    planning_store: &crate::artifacts::planning_store::PlanningStore,
    snapshot: &PlanSnapshotV1,
) -> Result<Option<crate::core::lifecycle::GateReceipt>, crate::artifacts::planning_store::StoreError>
{
    store::load_review_receipt(planning_store, snapshot)
}
