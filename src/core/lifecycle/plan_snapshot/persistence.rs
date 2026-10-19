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
    let deferred_assumptions = crate::core::planning::QuestionStore::new(planning_store)
        .list()
        .map_err(|error| error.to_string())?
        .into_iter()
        .filter(|question| question.feature_uid == identity.uid)
        .filter_map(|question| {
            question.deferred.map(|deferred| {
                format!(
                    "{} — assumption: {}; owner: {}; risk: {}; verification: {}; follow-up: {}",
                    question.question,
                    deferred.assumption,
                    deferred.owner,
                    deferred.risk,
                    deferred.verification_test,
                    deferred.follow_up_task,
                )
            })
        })
        .collect();
    let mut snapshot =
        PlanSnapshotV1::from_feature(feature_id, identity, markdown, approval_contract)?;
    if let Some(work) = crate::core::planning_work::load(planning_store)
        .map_err(|error| error.to_string())?
        .into_iter()
        .find(|work| {
            work.kind == crate::core::planning_work::WorkKind::Feature
                && work.feature_id.as_deref() == Some(feature_id)
                && work.feature_uid.as_deref() == Some(identity.uid.as_str())
        })
    {
        snapshot = snapshot.with_original_goal_ref(format!(
            "planning_work:{}:request_sha256:{}",
            work.uid,
            super::content::sha256(work.request.as_bytes())
        ))?;
    }
    let mut snapshot = snapshot.with_deferred_assumptions(deferred_assumptions)?;
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

pub(crate) fn load_persisted(
    planning_store: &crate::artifacts::planning_store::PlanningStore,
    uid: &str,
) -> Result<Option<PlanSnapshotV1>, crate::artifacts::planning_store::StoreError> {
    store::load_persisted(planning_store, uid)
}

pub(crate) fn review_change(
    planning_store: &crate::artifacts::planning_store::PlanningStore,
    snapshot: &PlanSnapshotV1,
    receipt: &crate::core::lifecycle::GateReceipt,
    verdict: &crate::harness::prompt_catalog::ReviewVerdictV1,
) -> Result<
    Option<(
        String,
        Vec<u8>,
        crate::artifacts::planning_store::RecordRevisionCheck,
    )>,
    crate::artifacts::planning_store::StoreError,
> {
    store::review_change(planning_store, snapshot, receipt, verdict)
}

pub(crate) fn plan_review_verdict(
    planning_store: &crate::artifacts::planning_store::PlanningStore,
    snapshot: &PlanSnapshotV1,
) -> Result<
    Option<crate::harness::prompt_catalog::ReviewVerdictV1>,
    crate::artifacts::planning_store::StoreError,
> {
    store::load_review_verdict(planning_store, snapshot)
}

pub(crate) fn has_approved_review(
    planning_store: &crate::artifacts::planning_store::PlanningStore,
    snapshot: &PlanSnapshotV1,
) -> Result<bool, crate::artifacts::planning_store::StoreError> {
    store::has_approved_review(planning_store, snapshot)
}

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

pub(crate) fn has_current_user_confirmation(
    planning_store: &crate::artifacts::planning_store::PlanningStore,
    snapshot: &PlanSnapshotV1,
) -> Result<bool, crate::artifacts::planning_store::StoreError> {
    confirmation::is_current(planning_store, snapshot)
}

pub(crate) fn plan_review_receipt(
    planning_store: &crate::artifacts::planning_store::PlanningStore,
    snapshot: &PlanSnapshotV1,
) -> Result<Option<crate::core::lifecycle::GateReceipt>, crate::artifacts::planning_store::StoreError>
{
    store::load_review_receipt(planning_store, snapshot)
}
