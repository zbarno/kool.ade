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
    let workflow = crate::artifacts::task_docs::load_workflow(planning_store)
        .map_err(|error| error.to_string())?;
    let original_goal = workflow
        .brief
        .as_ref()
        .map(|brief| brief.goal.trim().to_owned())
        .filter(|goal| !goal.is_empty())
        .unwrap_or_else(|| super::content::section(markdown, "Intent"));
    let original_goal_ref = if workflow
        .brief
        .as_ref()
        .is_some_and(|brief| !brief.goal.trim().is_empty())
    {
        format!("workflow:brief.goal:{feature_id}")
    } else {
        format!("feature:{feature_id}#Intent")
    };
    let mut snapshot = PlanSnapshotV1::from_feature_with_original_goal(
        feature_id,
        identity,
        markdown,
        approval_contract,
        &original_goal_ref,
        &original_goal,
    )?;
    let questions = crate::core::planning::QuestionStore::new(planning_store)
        .list()
        .map_err(|error| format!("planning decisions could not be loaded: {error}"))?
        .into_iter()
        .filter(|question| question.feature_uid == identity.uid)
        .collect::<Vec<_>>();
    let deferred = questions
        .iter()
        .filter(|question| question.status == crate::core::planning::QuestionStatus::Deferred)
        .filter_map(|question| {
            question.deferred.as_ref().map(|details| {
                format!(
                    "{} (owner: {}; risk: {}; verification: {}; follow-up: {})",
                    details.assumption,
                    details.owner,
                    details.risk,
                    details.verification_test,
                    details.follow_up_task
                )
            })
        })
        .collect::<Vec<_>>();
    snapshot.set_deferred_assumptions(deferred)?;
    let decisions = questions
        .iter()
        .filter(|question| {
            matches!(
                question.status,
                crate::core::planning::QuestionStatus::Answered
                    | crate::core::planning::QuestionStatus::EvidenceResolved
            )
        })
        .map(|question| {
            format!(
                "{}: {} Answer: {}. Selected option: {}. Evidence: {}.",
                question.uid,
                question.question,
                question.answer_text.as_deref().unwrap_or("not recorded"),
                question.selected_option.as_deref().unwrap_or("none"),
                if question.evidence_refs.is_empty() {
                    "none recorded".into()
                } else {
                    question.evidence_refs.join(", ")
                }
            )
        })
        .collect::<Vec<_>>();
    snapshot.set_recorded_decisions(&decisions)?;
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

#[cfg(test)]
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

pub(crate) fn review_change_with_verdict(
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
    store::review_change_with_verdict(planning_store, snapshot, receipt, Some(verdict))
}

#[allow(clippy::too_many_arguments)] // Mirrors the persisted arbitration receipt fields.
pub(crate) fn arbitration_change(
    planning_store: &crate::artifacts::planning_store::PlanningStore,
    snapshot: &PlanSnapshotV1,
    finding_id: &str,
    evidence_id: &str,
    evidence_sha256: &str,
    source_quote: &str,
    rationale: &str,
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
    store::arbitration_change(
        planning_store,
        snapshot,
        finding_id,
        evidence_id,
        evidence_sha256,
        source_quote,
        rationale,
        receipt,
        verdict,
    )
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

pub(crate) fn latest_plan_arbitration(
    planning_store: &crate::artifacts::planning_store::PlanningStore,
    snapshot: &PlanSnapshotV1,
) -> Result<
    Option<(String, crate::core::lifecycle::GateReceipt)>,
    crate::artifacts::planning_store::StoreError,
> {
    store::load_latest_arbitration(planning_store, snapshot)
}

pub(crate) fn review_retry_available(
    planning_store: &crate::artifacts::planning_store::PlanningStore,
    snapshot: &PlanSnapshotV1,
) -> Result<bool, crate::artifacts::planning_store::StoreError> {
    store::review_retry_available(planning_store, snapshot)
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
