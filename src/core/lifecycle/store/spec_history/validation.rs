use std::collections::HashSet;

use crate::{
    artifacts::{planning_store::StoreError, product_docs},
    core::lifecycle::{FeatureEvent, GateDecision, LifecycleEventKind, SpecRevisionDraftV1},
};

use super::super::LifecycleStore;
use sha2::Digest as _;

mod helpers;
mod history;
mod pr_evidence;
mod target;
mod test_refs;
use helpers::{add_receipt, duplicate};
pub(super) use history::{revision_uid, validate_history};
use pr_evidence::persisted_provider_pr_state_evidence_ref;
use target::validate_approved_target;

pub(super) fn require_uuid(value: &str, label: &str) -> Result<(), StoreError> {
    if !crate::core::lifecycle::types::is_uuid(value) {
        return Err(StoreError::MalformedState(format!(
            "specification {label} identity is invalid"
        )));
    }
    Ok(())
}

pub(super) fn validate_draft(
    lifecycle: &LifecycleStore<'_>,
    feature_uid: &str,
    expected_sequence: u64,
    draft: &SpecRevisionDraftV1,
    projected_content_hash: Option<&str>,
    authorization: Option<&crate::core::lifecycle::FeatureDeltaAuthorizationV1>,
) -> Result<(), StoreError> {
    require_uuid(feature_uid, "feature")?;
    if draft.origin_events.is_empty()
        || draft.origin_events.len() > 512
        || duplicate(&draft.origin_events)
        || draft.deltas.len() > 256
    {
        return Err(StoreError::MalformedState(
            "specification revision has invalid origins or too many deltas".into(),
        ));
    }
    let feature = lifecycle.load_feature(feature_uid)?;
    if feature.revision != draft.source_revision {
        return Err(StoreError::MalformedState(
            "specification revision is based on a stale feature source revision".into(),
        ));
    }
    let current_revision = lifecycle.load_spec_revision(feature_uid)?;
    let approval_receipts = validate_approved_target(
        lifecycle,
        feature_uid,
        expected_sequence,
        &feature,
        current_revision.as_ref(),
        draft,
        authorization,
    )?;

    let events = lifecycle.events_for_feature(feature_uid)?;
    let mut known_decisions = feature
        .decision_refs
        .iter()
        .cloned()
        .collect::<HashSet<_>>();
    known_decisions.extend(approval_receipts.iter().cloned());
    known_decisions.extend(lifecycle.feature_delta_authorization_refs(feature_uid)?);
    if draft
        .decision_refs
        .iter()
        .any(|uid| !known_decisions.contains(uid))
        || approval_receipts
            .iter()
            .any(|uid| !draft.decision_refs.contains(uid))
        || duplicate(&draft.decision_refs)
    {
        return Err(StoreError::MalformedState(
            "specification decision reference does not resolve or is duplicated".into(),
        ));
    }
    let requirement_ids = if let Some(snapshot_uid) = feature.plan_snapshot_uid.as_deref() {
        let snapshot =
            crate::core::lifecycle::plan_snapshot::load_persisted(lifecycle.store, snapshot_uid)?
                .ok_or_else(|| {
                StoreError::MalformedState("feature plan snapshot does not resolve".into())
            })?;
        if snapshot.feature_uid != feature_uid
            || feature.plan_snapshot_sha256.as_deref() != Some(snapshot.content_hash.as_str())
        {
            return Err(StoreError::MalformedState(
                "feature plan snapshot belongs to different approved content".into(),
            ));
        }
        snapshot
            .requirements
            .into_iter()
            .map(|requirement| requirement.stable_id)
            .collect::<HashSet<_>>()
    } else {
        HashSet::new()
    };
    if !draft.deltas.is_empty() && requirement_ids.is_empty() {
        return Err(StoreError::MalformedState(
            "specification deltas require a persisted stable-requirement plan snapshot".into(),
        ));
    }
    let rendered = product_docs::render_product(lifecycle.store)
        .map_err(|error| StoreError::MalformedState(error.to_string()))?
        .ok_or_else(|| StoreError::MalformedState("product specification is unavailable".into()))?;
    let current_content_hash = format!(
        "{:x}",
        sha2::Sha256::digest(
            serde_json::to_vec(&Some(rendered))
                .map_err(|error| StoreError::MalformedState(error.to_string()))?
        )
    );
    let known_event_uids = events
        .iter()
        .map(|event| event.uid.as_str())
        .collect::<HashSet<_>>();
    for event_uid in &draft.origin_events {
        require_uuid(event_uid, "origin event")?;
        let path = format!("state/spec_sync_queue/{event_uid}.json");
        let (queue, revision) = lifecycle
            .store
            .read_record::<crate::core::lifecycle::SpecSyncQueueEntryV1>(&path)?;
        queue.validate().map_err(StoreError::MalformedState)?;
        if revision != queue.revision || queue.feature_uid != feature_uid {
            return Err(StoreError::MalformedState(
                "specification origin event does not resolve to this feature".into(),
            ));
        }
        if !known_event_uids.contains(event_uid.as_str())
            && queue.trigger != crate::core::lifecycle::SpecSyncTriggerKindV1::ManualRefresh
        {
            return Err(StoreError::MalformedState(
                "specification origin event has no lifecycle event record".into(),
            ));
        }
    }

    let tasks = feature
        .task_uids
        .iter()
        .map(|uid| lifecycle.load_task(uid))
        .collect::<Result<Vec<_>, _>>()?;
    let known_tasks = feature
        .task_uids
        .iter()
        .map(String::as_str)
        .collect::<HashSet<_>>();
    let mut known_evidence = HashSet::new();
    for event in &events {
        known_evidence.insert(event.uid.clone());
        known_evidence.insert(format!("lifecycle-event:{}", event.uid));
        match &event.event {
            LifecycleEventKind::Feature(FeatureEvent::UserPlanDecision(receipt)) => {
                add_receipt(&mut known_evidence, receipt);
            }
            LifecycleEventKind::Feature(FeatureEvent::PlanReview(receipt)) => {
                add_receipt(&mut known_evidence, receipt);
            }
            LifecycleEventKind::Task(
                crate::core::lifecycle::TaskEvent::CodeReview { receipt, .. }
                | crate::core::lifecycle::TaskEvent::RuntimeQaCompleted { receipt, .. }
                | crate::core::lifecycle::TaskEvent::ReconcileSpec { receipt, .. }
                | crate::core::lifecycle::TaskEvent::SubtaskReconciled { receipt },
            ) => add_receipt(&mut known_evidence, receipt),
            LifecycleEventKind::Task(
                crate::core::lifecycle::TaskEvent::RuntimeQaNotApplicable {
                    reviewer_receipt,
                    receipt,
                    ..
                },
            ) => {
                add_receipt(&mut known_evidence, reviewer_receipt);
                add_receipt(&mut known_evidence, receipt);
            }
            _ => {}
        }
    }
    for trigger in lifecycle.list_spec_sync_queue(feature_uid, true)? {
        known_evidence.insert(trigger.uid.clone());
        known_evidence.insert(format!("spec-trigger:{}", trigger.uid));
    }
    for task in &tasks {
        known_evidence.insert(task.task_uid.clone());
        known_evidence.insert(format!("task:{}", task.task_uid));
        if let Some(planning_uid) = &task.planning_item_uid {
            known_evidence.insert(planning_uid.clone());
            known_evidence.insert(format!("planning-item:{planning_uid}"));
        }
        if let Some(qa) = task
            .qa_execution
            .as_ref()
            .and_then(|state| state.request.as_ref())
        {
            known_evidence.insert(qa.qa_run_id.clone());
            known_evidence.insert(format!("qa:{}", qa.qa_run_id));
        }
        for receipt in task
            .code_review
            .iter()
            .chain(task.runtime_qa.iter())
            .chain(task.spec_reconciliation.iter())
            .chain(task.review_history.iter())
        {
            add_receipt(&mut known_evidence, receipt);
        }
        if let Some(work_item_uid) = task.pr_work_item_uid.as_deref() {
            known_evidence.insert(persisted_provider_pr_state_evidence_ref(
                lifecycle,
                feature_uid,
                &task.task_uid,
                work_item_uid,
            )?);
        }
    }
    if let Some(hash) = &feature.approved_plan_sha256 {
        known_evidence.insert(format!("plan:{hash}"));
        known_evidence.insert(hash.clone());
    }
    if let Some(snapshot_uid) = &feature.plan_snapshot_uid {
        known_evidence.insert(format!("plan:{snapshot_uid}"));
        known_evidence.insert(snapshot_uid.clone());
    }
    known_evidence.insert(format!("current-spec:{current_content_hash}"));
    known_evidence.insert(current_content_hash.clone());
    if let Some(projected_hash) = projected_content_hash {
        known_evidence.insert(format!("current-spec:{projected_hash}"));
        known_evidence.insert(projected_hash.to_owned());
    }

    let mut seen_requirements = HashSet::new();
    for delta in &draft.deltas {
        test_refs::validate_requirement_tests(lifecycle, &feature, delta)?;
        let mut trace_tasks = delta.affected_task_ids.clone();
        trace_tasks.extend(delta.followup_ids.clone());
        let valid_decisions = lifecycle.requirement_decision_refs(
            feature_uid,
            &delta.requirement_id,
            &trace_tasks,
        )?;
        delta.validate().map_err(StoreError::MalformedState)?;
        if !draft.origin_events.contains(&delta.event_id)
            || !requirement_ids.contains(&delta.requirement_id)
            || delta
                .operator_decision
                .as_ref()
                .is_some_and(|decision| !draft.decision_refs.contains(&decision.receipt_uid))
            || delta
                .decision_refs
                .iter()
                .any(|decision| !valid_decisions.contains(decision))
            || !seen_requirements.insert(delta.requirement_id.as_str())
            || delta
                .affected_task_ids
                .iter()
                .any(|uid| !known_tasks.contains(uid.as_str()))
            || delta
                .followup_ids
                .iter()
                .any(|uid| !known_tasks.contains(uid.as_str()))
            || delta
                .evidence_refs
                .iter()
                .any(|evidence| !known_evidence.contains(evidence))
        {
            return Err(StoreError::MalformedState(
                "specification delta contains an unresolved event, task, evidence, or duplicate requirement reference".into(),
            ));
        }
    }

    if let Some(head) = &draft.implementation_head_sha
        && !tasks.iter().any(|task| {
            task.reviewed_code_head_sha.as_deref() == Some(head)
                && task.code_review.as_ref().is_some_and(|receipt| {
                    receipt.decision == GateDecision::Approved
                        && task.implementation_sha256.as_deref()
                            == Some(receipt.subject_sha256.as_str())
                })
        })
    {
        return Err(StoreError::MalformedState(
            "implementation head is not bound to a current passing code review".into(),
        ));
    }

    if draft.content_hash != projected_content_hash.unwrap_or(&current_content_hash) {
        return Err(StoreError::MalformedState(
            "specification content hash does not match the persisted product specification".into(),
        ));
    }
    Ok(())
}
