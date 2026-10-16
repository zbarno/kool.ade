use std::collections::HashSet;

use crate::{
    artifacts::planning_store::StoreError,
    core::lifecycle::{
        FeatureEvent, GateActor, GateDecision, LifecycleEventKind, SpecDeltaApprovalRefV1,
    },
};

use super::super::super::LifecycleStore;

pub(super) fn has_current_user_approval(
    lifecycle: &LifecycleStore<'_>,
    feature_uid: &str,
    target_hash: Option<&str>,
) -> Result<bool, StoreError> {
    let Some(target_hash) = target_hash else {
        return Ok(false);
    };
    Ok(lifecycle
        .events_for_feature(feature_uid)?
        .iter()
        .any(|event| {
            matches!(
                &event.event,
                LifecycleEventKind::Feature(FeatureEvent::UserPlanDecision(receipt))
                    if receipt.actor == GateActor::User
                        && receipt.decision == GateDecision::Approved
                        && receipt.subject_sha256 == target_hash
            )
        }))
}

pub(super) fn load_delta_approval(
    lifecycle: &LifecycleStore<'_>,
    feature_uid: &str,
    reference: &SpecDeltaApprovalRefV1,
) -> Result<crate::core::lifecycle::SpecDeltaApprovalV1, StoreError> {
    if !crate::core::lifecycle::types::is_uuid(&reference.task_uid)
        || !crate::core::lifecycle::types::is_sha256(&reference.proposal_sha256)
    {
        return Err(StoreError::MalformedState(
            "specification delta approval reference is invalid".into(),
        ));
    }
    let task = lifecycle.load_task(&reference.task_uid)?;
    if task.feature_uid != feature_uid {
        return Err(StoreError::MalformedState(
            "specification delta approval belongs to another feature".into(),
        ));
    }
    let approval =
        lifecycle.load_spec_delta_approval(&reference.task_uid, &reference.proposal_sha256)?;
    approval.validate().map_err(StoreError::MalformedState)?;
    Ok(approval)
}

pub(super) fn add_receipt(
    known: &mut HashSet<String>,
    receipt: &crate::core::lifecycle::GateReceipt,
) {
    known.insert(receipt.receipt_uid.clone());
    known.insert(format!("receipt:{}", receipt.receipt_uid));
    known.insert(format!("review:{}", receipt.receipt_uid));
    known.insert(format!("code-review:{}", receipt.receipt_uid));
    known.insert(format!("runtime-qa:{}", receipt.receipt_uid));
    known.extend(receipt.evidence_refs.iter().cloned());
}

pub(super) fn duplicate(values: &[String]) -> bool {
    values
        .iter()
        .enumerate()
        .any(|(index, value)| values[..index].contains(value))
}

pub(super) fn duplicate_approvals(values: &[SpecDeltaApprovalRefV1]) -> bool {
    values.iter().enumerate().any(|(index, value)| {
        values[..index].iter().any(|prior| {
            prior.task_uid == value.task_uid && prior.proposal_sha256 == value.proposal_sha256
        })
    })
}
