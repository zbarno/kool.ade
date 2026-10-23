use crate::core::lifecycle::{
    FeatureEvent, FeatureGoal, GateActor, GateDecision, GateReceipt, LifecycleEventKind,
    LifecycleEventRecord,
};

pub(super) fn reviewed_plan_receipts<'a>(
    feature: &FeatureGoal,
    records: &'a [LifecycleEventRecord],
    plan_hash: &str,
) -> Option<(&'a GateReceipt, &'a GateReceipt)> {
    let user = records.iter().find_map(|record| match &record.event {
        LifecycleEventKind::Feature(FeatureEvent::UserPlanDecision(receipt))
            if record.actor == crate::core::lifecycle::LifecycleActor::User
                && receipt.actor == GateActor::User
                && receipt.decision == GateDecision::Approved
                && receipt.subject_uid
                    == feature.plan_snapshot_uid.as_deref().unwrap_or_default()
                && receipt.subject_sha256 == plan_hash
                && feature.decision_refs.len() == 1
                && feature.decision_refs.first() == Some(&receipt.receipt_uid) =>
        {
            Some(receipt)
        }
        _ => None,
    })?;
    let review = records.iter().find_map(|record| match &record.event {
        LifecycleEventKind::Feature(FeatureEvent::PlanReview(receipt))
            if record.actor == crate::core::lifecycle::LifecycleActor::IndependentReviewer
                && receipt.actor == GateActor::IndependentReviewer
                && receipt.decision == GateDecision::Approved
                && receipt.subject_uid
                    == feature.plan_snapshot_uid.as_deref().unwrap_or_default()
                && receipt.subject_sha256 == plan_hash
                && receipt.prompt_role()
                    == Some(crate::harness::prompt_catalog::PromptRole::PlanReviewer)
                && feature.review_refs.len() == 1
                && feature.review_refs.first() == Some(&receipt.receipt_uid) =>
        {
            Some(receipt)
        }
        _ => None,
    })?;
    Some((user, review))
}
