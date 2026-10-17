use std::collections::HashSet;

use crate::{
    artifacts::planning_store::StoreError,
    core::lifecycle::{
        FeatureDeltaAuthorizationV1, FeatureGoal, SpecDeltaOperatorChoiceV1, SpecRevisionDraftV1,
        SpecRevisionV1,
    },
};

use super::super::super::LifecycleStore;
use super::helpers::{duplicate_approvals, has_current_user_approval, load_delta_approval};

#[cfg(test)]
#[path = "target/tests.rs"]
mod tests;

pub(super) fn validate_approved_target(
    lifecycle: &LifecycleStore<'_>,
    feature_uid: &str,
    expected_sequence: u64,
    feature: &FeatureGoal,
    current: Option<&SpecRevisionV1>,
    draft: &SpecRevisionDraftV1,
    pending_authorization: Option<&FeatureDeltaAuthorizationV1>,
) -> Result<HashSet<String>, StoreError> {
    if draft.delta_approval_refs.len() > 64 || duplicate_approvals(&draft.delta_approval_refs) {
        return Err(StoreError::MalformedState(
            "specification delta approval references are duplicated or exceed the limit".into(),
        ));
    }
    if draft.delta_approval_refs.iter().any(|reference| {
        !crate::core::lifecycle::types::is_uuid(&reference.task_uid)
            || !crate::core::lifecycle::types::is_sha256(&reference.proposal_sha256)
    }) {
        return Err(StoreError::MalformedState(
            "specification delta approval reference is invalid".into(),
        ));
    }
    let mut receipts = HashSet::new();
    let mut approvals = Vec::new();
    for reference in &draft.delta_approval_refs {
        let approval = load_delta_approval(lifecycle, feature_uid, reference)?;
        if approval.resulting_spec_sha256 != draft.content_hash {
            return Err(StoreError::MalformedState(
                "user-approved delta does not bind the current specification content hash".into(),
            ));
        }
        receipts.insert(approval.receipt.receipt_uid.clone());
        approvals.push(approval);
    }
    let task_approval = approvals.iter().any(|approval| {
        Some(approval.resulting_spec_sha256.as_str()) == draft.approved_target_hash.as_deref()
    });
    let (feature_adoption, feature_receipts) = validate_feature_authorization(
        lifecycle,
        feature_uid,
        expected_sequence,
        draft,
        pending_authorization,
    )?;
    receipts.extend(feature_receipts);
    if let Some(current) = current {
        if current.seq != expected_sequence {
            return Err(StoreError::StaleRecordRevision {
                path: format!("state/spec_revisions/{feature_uid}.json"),
                expected: expected_sequence,
                actual: current.seq,
            });
        }
        if current.approved_target_hash.is_some() && draft.approved_target_hash.is_none() {
            return Err(StoreError::MalformedState(
                "approved target history cannot be erased by reconciliation".into(),
            ));
        }
        if current.approved_target_hash != draft.approved_target_hash
            && !has_current_user_approval(
                lifecycle,
                feature_uid,
                draft.approved_target_hash.as_deref(),
            )?
            && !task_approval
            && !feature_adoption
        {
            return Err(StoreError::MalformedState(
                "approved target cannot change without a matching user plan approval".into(),
            ));
        }
    } else if draft.approved_target_hash.is_some()
        && !has_current_user_approval(
            lifecycle,
            feature_uid,
            draft.approved_target_hash.as_deref(),
        )?
        && !task_approval
        && !feature_adoption
    {
        return Err(StoreError::MalformedState(
            "initial approved target requires a matching user plan approval".into(),
        ));
    }
    if draft.approved_target_hash.as_deref() != feature.approved_plan_sha256.as_deref()
        && !current
            .is_some_and(|revision| revision.approved_target_hash == draft.approved_target_hash)
        && !task_approval
        && !feature_adoption
    {
        return Err(StoreError::MalformedState(
            "approved target hash must match the current lifecycle plan snapshot".into(),
        ));
    }
    Ok(receipts)
}

fn validate_feature_authorization(
    lifecycle: &LifecycleStore<'_>,
    feature_uid: &str,
    source_sequence: u64,
    draft: &SpecRevisionDraftV1,
    pending: Option<&FeatureDeltaAuthorizationV1>,
) -> Result<(bool, HashSet<String>), StoreError> {
    let mut receipts = HashSet::new();
    let mut adopted = false;
    let decisions = draft.deltas.iter().filter_map(|delta| {
        delta
            .operator_decision
            .as_ref()
            .map(|decision| (delta, decision))
    });
    for (delta, decision) in decisions {
        let stored_authorization;
        let authorization =
            if pending.is_some_and(|value| value.receipt_uid == decision.receipt_uid) {
                pending.expect("checked pending authorization")
            } else {
                match lifecycle.load_feature_delta_authorization(&decision.receipt_uid) {
                    Ok(value) => {
                        stored_authorization = value;
                        &stored_authorization
                    }
                    Err(_) => {
                        return Err(StoreError::MalformedState(
                            "feature operator decision has no persisted user authorization".into(),
                        ));
                    }
                }
            };
        authorization
            .validate()
            .map_err(StoreError::MalformedState)?;
        use sha2::Digest as _;
        let text_hash = |value: Option<&str>| {
            value.map(|text| format!("{:x}", sha2::Sha256::digest(text.as_bytes())))
        };
        let current_authorization = authorization.source_spec_sequence == source_sequence;
        let historical_authorization = authorization.source_spec_sequence < source_sequence
            && lifecycle
                .spec_revision_history(feature_uid)?
                .iter()
                .any(|(entry, deltas)| {
                    entry.seq == authorization.source_spec_sequence + 1
                        && deltas.iter().any(|prior| prior == delta)
                        && (decision.decision == SpecDeltaOperatorChoiceV1::Declined
                            || (entry.approved_target_hash
                                == authorization.resulting_product_sha256
                                && Some(entry.content_hash.as_str())
                                    == authorization.resulting_product_sha256.as_deref()))
                });
        let approves_current_target = current_authorization
            && decision.decision == SpecDeltaOperatorChoiceV1::Adopted
            && authorization.resulting_product_sha256.as_deref()
                == draft.approved_target_hash.as_deref()
            && authorization.resulting_product_sha256.as_deref()
                == Some(draft.content_hash.as_str());
        let exact = authorization.feature_uid == feature_uid
            && (current_authorization || historical_authorization)
            && authorization.requirement_id == delta.requirement_id
            && authorization.event_uid == delta.event_id
            && authorization.old_target_sha256 == text_hash(delta.old_target.as_deref())
            && authorization.proposed_target_sha256 == text_hash(delta.proposed_target.as_deref())
            && authorization.decision == decision.decision
            && authorization.actor == decision.actor
            && authorization.recorded_at_ms == decision.recorded_at_ms
            && authorization.receipt_uid == decision.receipt_uid
            && authorization.resulting_product_sha256.as_deref()
                == decision.resulting_spec_sha256.as_deref()
            && (decision.decision == SpecDeltaOperatorChoiceV1::Declined
                || historical_authorization
                || approves_current_target);
        if !exact {
            return Err(StoreError::MalformedState(
                "feature operator decision does not match its persisted authorization".into(),
            ));
        }
        adopted |= approves_current_target;
        receipts.insert(decision.receipt_uid.clone());
    }
    if let Some(pending) = pending
        && !receipts.contains(&pending.receipt_uid)
    {
        return Err(StoreError::MalformedState(
            "pending feature authorization has no matching operator decision".into(),
        ));
    }
    Ok((adopted, receipts))
}
