use crate::{
    app::session::Project,
    core::lifecycle::{
        LifecycleStore, SpecDeltaApprovalRefV1, SpecDeltaClassificationV1, SpecDeltaRecordV1,
        SpecReconciliationStatusV1, SpecRevisionDraftV1,
    },
    harness::prompt_catalog::{ApprovalEffect, DeltaClassification, SpecDeltaV1},
};

mod feature;
pub(in crate::app::root::tick::activity) use feature::record_feature;

pub(super) fn record(
    project: &Project,
    task_uid: &str,
    proposal: Option<&SpecDeltaV1>,
    outcome_reason: Option<&str>,
    run_uid: Option<&str>,
) -> Result<(), String> {
    record_internal(project, task_uid, proposal, outcome_reason, run_uid, None)
}

pub(super) fn record_user_approved_delta(
    project: &Project,
    task_uid: &str,
    proposal: &SpecDeltaV1,
    approval: &crate::core::lifecycle::SpecDeltaApprovalV1,
) -> Result<(), String> {
    let approval_ref = SpecDeltaApprovalRefV1 {
        task_uid: task_uid.to_owned(),
        proposal_sha256: approval.proposal_sha256.clone(),
    };
    record_internal(
        project,
        task_uid,
        Some(proposal),
        Some(
            "User approved the proposed target; implementation must be reconciled against the new specification.",
        ),
        None,
        Some((approval_ref, approval)),
    )
}

fn record_internal(
    project: &Project,
    task_uid: &str,
    proposal: Option<&SpecDeltaV1>,
    outcome_reason: Option<&str>,
    run_uid: Option<&str>,
    approval: Option<(
        SpecDeltaApprovalRefV1,
        &crate::core::lifecycle::SpecDeltaApprovalV1,
    )>,
) -> Result<(), String> {
    let lifecycle = LifecycleStore::new(&project.state.planning_store);
    let task = lifecycle
        .load_task(task_uid)
        .map_err(|error| error.to_string())?;
    let feature = lifecycle
        .load_feature(&task.feature_uid)
        .map_err(|error| error.to_string())?;
    let queue = lifecycle
        .list_spec_sync_queue(&task.feature_uid, true)
        .map_err(|error| error.to_string())?;
    let origin_events = queue
        .iter()
        .filter(|entry| {
            entry.status != crate::core::lifecycle::SpecSyncQueueStatusV1::Declined
                && entry.source_task_uid.as_deref() == Some(task_uid)
        })
        .map(|entry| entry.source_event_uid.clone())
        .collect::<Vec<_>>();
    if origin_events.is_empty() {
        return Err(
            "no durable lifecycle or manual-refresh trigger resolves to this feature".into(),
        );
    }
    let delta_event_uid = queue
        .iter()
        .rev()
        .find(|entry| {
            entry.source_task_uid.as_deref() == Some(task_uid)
                && entry.status != crate::core::lifecycle::SpecSyncQueueStatusV1::Declined
        })
        .map(|entry| entry.source_event_uid.as_str())
        .unwrap_or_else(|| origin_events.last().expect("nonempty origin list"));
    let deltas = proposal
        .map(|proposal| {
            map_deltas(
                proposal,
                &task.task_uid,
                delta_event_uid,
                approval.is_some(),
                &lifecycle,
                &task.feature_uid,
                &project.state.planning_store,
            )
        })
        .transpose()?
        .unwrap_or_default();
    let status = if proposal.is_some_and(|proposal| !proposal.deltas.is_empty()) {
        if approval.is_some() {
            SpecReconciliationStatusV1::BlockingDrift
        } else {
            SpecReconciliationStatusV1::NeedsHumanDecision
        }
    } else if outcome_reason.is_some() {
        SpecReconciliationStatusV1::NeedsHumanDecision
    } else {
        SpecReconciliationStatusV1::Synchronized
    };
    let reason = outcome_reason.map(|reason| reason.chars().take(4096).collect::<String>());
    let current = lifecycle
        .load_spec_revision(&task.feature_uid)
        .map_err(|error| error.to_string())?;
    let expected_sequence = current.as_ref().map_or(0, |revision| revision.seq);
    let approved_target_hash = current
        .as_ref()
        .and_then(|revision| revision.approved_target_hash.clone())
        .or_else(|| feature.approved_plan_sha256.clone());
    let approved_target_hash = approval
        .as_ref()
        .map(|(_, approval)| approval.resulting_spec_sha256.clone())
        .or(approved_target_hash);
    let mut decision_refs = feature.decision_refs.clone();
    if let Some((_, approval)) = &approval {
        decision_refs.push(approval.receipt.receipt_uid.clone());
    }
    let implementation_head_sha = task
        .code_review
        .as_ref()
        .filter(|receipt| {
            receipt.decision == crate::core::lifecycle::GateDecision::Approved
                && task.implementation_sha256.as_deref() == Some(receipt.subject_sha256.as_str())
        })
        .and_then(|_| task.reviewed_code_head_sha.clone());
    let draft = SpecRevisionDraftV1 {
        content_hash: crate::core::lifecycle::product_spec::current_product_hash(
            &project.state.planning_store,
        )
        .map_err(|error| error.to_string())?,
        source_revision: feature.revision,
        produced_by_run_id: run_uid.map(str::to_owned),
        origin_events,
        decision_refs,
        delta_approval_refs: approval
            .map(|(reference, _)| vec![reference])
            .unwrap_or_default(),
        approved_target_hash,
        implementation_head_sha,
        reconciliation_status: status,
        outcome_reason: reason,
        deltas,
    };
    lifecycle
        .append_spec_revision(&task.feature_uid, expected_sequence, draft)
        .map(|_| ())
        .map_err(|error| error.to_string())
}

fn map_deltas(
    proposal: &SpecDeltaV1,
    task_uid: &str,
    event_uid: &str,
    already_approved: bool,
    lifecycle: &LifecycleStore<'_>,
    feature_uid: &str,
    planning_store: &crate::artifacts::planning_store::PlanningStore,
) -> Result<Vec<SpecDeltaRecordV1>, String> {
    proposal
        .deltas
        .iter()
        .map(|delta| {
            let followup_ids = delta
                .followup_task_ids
                .iter()
                .map(|uid| {
                    uuid::Uuid::parse_str(uid)
                        .map(|uid| uid.hyphenated().to_string())
                        .map_err(|_| format!("follow-up reference {} is not a task UUID", uid))
                })
                .collect::<Result<Vec<_>, _>>()?;
            let (old_target, proposed_target, observed_current) = match &delta.classification {
                DeltaClassification::CurrentTruth => {
                    (Some(delta.old.clone()), None, Some(delta.new.clone()))
                }
                DeltaClassification::FutureTarget => {
                    (Some(delta.old.clone()), Some(delta.new.clone()), None)
                }
                DeltaClassification::Documentation => {
                    (Some(delta.old.clone()), Some(delta.new.clone()), None)
                }
            };
            let classification = match &delta.classification {
                DeltaClassification::CurrentTruth => SpecDeltaClassificationV1::Implementation,
                DeltaClassification::FutureTarget => SpecDeltaClassificationV1::UserDecision,
                DeltaClassification::Documentation => SpecDeltaClassificationV1::Documentation,
            };
            let affected_task_ids = vec![task_uid.to_owned()];
            let decision_refs = lifecycle
                .requirement_decision_refs(feature_uid, &delta.requirement_id, &affected_task_ids)
                .map_err(|error| error.to_string())?;
            Ok(SpecDeltaRecordV1 {
                requirement_id: delta.requirement_id.clone(),
                event_id: event_uid.to_owned(),
                old_target,
                proposed_target,
                observed_current,
                classification,
                evidence_refs: delta.evidence.clone(),
                decision_refs,
                test_refs: crate::core::planning::QuestionStore::new(planning_store)
                    .load_readiness(feature_uid)
                    .map_err(|error| error.to_string())?
                    .map(|(readiness, _)| {
                        readiness
                            .acceptance_tests
                            .into_iter()
                            .filter(|test| test.requirement_id == delta.requirement_id)
                            .map(|test| test.test_ref)
                            .collect()
                    })
                    .unwrap_or_default(),
                affected_task_ids,
                needs_user_reapproval: !already_approved
                    && matches!(&delta.approval_effect, ApprovalEffect::RequiresUserApproval),
                followup_ids,
                operator_decision: None,
            })
        })
        .collect()
}
