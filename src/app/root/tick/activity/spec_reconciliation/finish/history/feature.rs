use crate::{
    app::session::Project,
    core::lifecycle::{
        FeatureGoal, LifecycleStore, SpecDeltaClassificationV1, SpecDeltaRecordV1,
        SpecReconciliationStatusV1, SpecRevisionDraftV1, SpecSyncQueueEntryV1,
    },
    harness::prompt_catalog::{ApprovalEffect, DeltaClassification, SpecDeltaV1},
};

pub(in crate::app::root::tick::activity) fn record_feature(
    project: &Project,
    feature: &FeatureGoal,
    origins: &[SpecSyncQueueEntryV1],
    proposal: Option<&SpecDeltaV1>,
    run_uid: Option<&str>,
    reason: Option<&str>,
) -> Result<(), String> {
    let lifecycle = LifecycleStore::new(&project.state.planning_store);
    let origin_events = origins
        .iter()
        .filter(|origin| {
            origin.source_task_uid.is_none()
                && origin.status == crate::core::lifecycle::SpecSyncQueueStatusV1::Pending
        })
        .map(|origin| origin.source_event_uid.clone())
        .collect::<Vec<_>>();
    if origin_events.is_empty()
        || origins
            .iter()
            .any(|origin| origin.feature_uid != feature.feature_uid)
    {
        return Err("feature reconciliation has no current feature-scoped trigger".into());
    }
    let current = lifecycle
        .load_spec_revision(&feature.feature_uid)
        .map_err(|error| error.to_string())?;
    let expected_sequence = current.as_ref().map_or(0, |revision| revision.seq);
    let event_uid = origin_events
        .last()
        .expect("nonempty feature reconciliation origins");
    let deltas = proposal
        .map(|proposal| {
            map_deltas(
                proposal,
                event_uid,
                &project.state.planning_store,
                &feature.feature_uid,
                &lifecycle,
            )
        })
        .transpose()?
        .unwrap_or_default();
    let nonblocking_only = proposal.is_some_and(|proposal| {
        !proposal.deltas.is_empty()
            && proposal.deltas.iter().all(|delta| {
                matches!(&delta.classification, DeltaClassification::Documentation)
                    && matches!(
                        &delta.approval_effect,
                        ApprovalEffect::DoesNotChangeApproval
                    )
            })
    });
    let status = if reason.is_some() {
        SpecReconciliationStatusV1::NeedsHumanDecision
    } else if proposal.is_some_and(|proposal| !proposal.deltas.is_empty()) {
        if nonblocking_only {
            SpecReconciliationStatusV1::OutstandingNonblockingFindings
        } else {
            SpecReconciliationStatusV1::NeedsHumanDecision
        }
    } else {
        SpecReconciliationStatusV1::Synchronized
    };
    let outcome_reason = match status {
        SpecReconciliationStatusV1::NeedsHumanDecision => Some(
            reason
                .map(str::to_owned)
                .unwrap_or_else(|| {
                    "Feature refresh found a proposed target change requiring user review.".into()
                })
                .chars()
                .take(4096)
                .collect(),
        ),
        _ => None,
    };
    let revision = SpecRevisionDraftV1 {
        no_change_reason: proposal.filter(|proposal| proposal.deltas.is_empty()).map(|_| {
            "Application policy accepted the validated SpecReconciler's empty delta result for the exact persisted approved feature plan, current product specification and included feature triggers; no documentation or target change was proposed. This pass establishes no current implementation claim.".into()
        }),
        content_hash: crate::core::lifecycle::product_spec::current_product_hash(
            &project.state.planning_store,
        )
        .map_err(|error| error.to_string())?,
        source_revision: feature.revision,
        produced_by_run_id: run_uid.map(str::to_owned),
        origin_events,
        decision_refs: feature.decision_refs.clone(),
        delta_approval_refs: Vec::new(),
        approved_target_hash: current
            .as_ref()
            .and_then(|revision| revision.approved_target_hash.clone())
            .or_else(|| feature.approved_plan_sha256.clone()),
        implementation_head_sha: current.and_then(|revision| revision.implementation_head_sha),
        reconciliation_status: status,
        outcome_reason,
        deltas,
    };
    lifecycle
        .append_spec_revision(&feature.feature_uid, expected_sequence, revision)
        .map(|_| ())
        .map_err(|error| error.to_string())
}

fn map_deltas(
    proposal: &SpecDeltaV1,
    event_uid: &str,
    store: &crate::artifacts::planning_store::PlanningStore,
    feature_uid: &str,
    lifecycle: &LifecycleStore<'_>,
) -> Result<Vec<SpecDeltaRecordV1>, String> {
    proposal
        .deltas
        .iter()
        .map(|delta| {
            if matches!(&delta.classification, DeltaClassification::CurrentTruth) {
                return Err("feature-only refresh cannot assert current implementation without task review and QA evidence".into());
            }
            let followup_ids = delta
                .followup_task_ids
                .iter()
                .map(|uid| {
                    uuid::Uuid::parse_str(uid)
                        .map(|uid| uid.hyphenated().to_string())
                        .map_err(|_| format!("follow-up reference {uid} is not a task UUID"))
                })
                .collect::<Result<Vec<_>, _>>()?;
            let (old_target, proposed_target) = match &delta.classification {
                DeltaClassification::FutureTarget | DeltaClassification::Documentation => {
                    (Some(delta.old.clone()), Some(delta.new.clone()))
                }
                DeltaClassification::CurrentTruth => unreachable!(),
            };
            let classification = match &delta.classification {
                DeltaClassification::FutureTarget => SpecDeltaClassificationV1::UserDecision,
                DeltaClassification::Documentation => SpecDeltaClassificationV1::Documentation,
                DeltaClassification::CurrentTruth => unreachable!(),
            };
            let decision_refs = lifecycle
                .requirement_decision_refs(feature_uid, &delta.requirement_id, &[])
                .map_err(|error| error.to_string())?;
            Ok(SpecDeltaRecordV1 {
                requirement_id: delta.requirement_id.clone(),
                event_id: event_uid.to_owned(),
                old_target,
                proposed_target,
                observed_current: None,
                classification,
                evidence_refs: delta.evidence.clone(),
                decision_refs,
                test_refs: crate::core::planning::QuestionStore::new(store)
                    .load_readiness(feature_uid)
                    .map_err(|error| error.to_string())?
                    .map(|(readiness, _)| readiness.acceptance_tests.into_iter()
                        .filter(|test| test.requirement_id == delta.requirement_id)
                        .map(|test| test.test_ref).collect())
                    .unwrap_or_default(),
                affected_task_ids: Vec::new(),
                needs_user_reapproval: !matches!(
                    &delta.approval_effect,
                    ApprovalEffect::DoesNotChangeApproval
                ),
                followup_ids,
                operator_decision: None,
            })
        })
        .collect()
}
