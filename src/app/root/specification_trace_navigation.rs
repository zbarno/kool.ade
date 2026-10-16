use crate::{
    app::session::Project,
    core::lifecycle::{
        GateReceipt, LifecycleEventKind, LifecycleStore, SpecTraceNavigationV1,
        SpecTraceReferenceKindV1, SpecTraceTargetKindV1,
    },
};

#[cfg(test)]
#[path = "specification_trace_navigation/tests.rs"]
mod tests;

pub(super) fn open(
    project: &mut Project,
    feature_uid: &str,
    kind: SpecTraceReferenceKindV1,
    reference: &str,
) -> Result<(), String> {
    let lifecycle = LifecycleStore::new(&project.state.planning_store);
    let traces = lifecycle
        .requirements_for_specification_reference(feature_uid, reference)
        .map_err(|error| error.to_string())?;
    if traces.is_empty() {
        return Err("No persisted requirement trace resolves this reference.".into());
    }
    let (target_kind, title, detail) = resolve_target(
        &lifecycle,
        &project.state.planning_store,
        feature_uid,
        kind,
        reference,
    )?;
    project.specification_trace_navigation = Some(SpecTraceNavigationV1 {
        feature_uid: feature_uid.to_owned(),
        target_kind,
        target_uid: reference.to_owned(),
        title,
        detail,
        requirement_traces: traces,
    });
    Ok(())
}

fn resolve_target(
    lifecycle: &LifecycleStore<'_>,
    store: &crate::artifacts::planning_store::PlanningStore,
    feature_uid: &str,
    kind: SpecTraceReferenceKindV1,
    reference: &str,
) -> Result<(SpecTraceTargetKindV1, String, String), String> {
    use SpecTraceReferenceKindV1 as RefKind;
    let target = match kind {
        RefKind::Task | RefKind::FollowupTask => {
            let task = lifecycle
                .load_task(reference)
                .map_err(|error| error.to_string())?;
            if task.feature_uid != feature_uid {
                return Err("Task reference belongs to another feature.".into());
            }
            (
                SpecTraceTargetKindV1::Task,
                format!("Task {}", task.task_uid),
                format!(
                    "Phase {:?} · approved plan {}",
                    task.phase, task.approved_plan_sha256
                ),
            )
        }
        RefKind::PullRequestState => {
            let (work_uid, revision) = parse_pr_reference(reference)?;
            let work = lifecycle
                .load_pr_preparation(work_uid)
                .map_err(|error| error.to_string())?;
            if work.feature_uid != feature_uid || work.revision != revision {
                return Err("PR-state reference is stale or belongs to another feature.".into());
            }
            (
                SpecTraceTargetKindV1::PullRequestState,
                format!("Pull request {}", work.provider_pr_id.unwrap_or_default()),
                format!(
                    "{:?} · {}",
                    work.status,
                    work.provider_pr_url
                        .unwrap_or_else(|| "URL not recorded".into())
                ),
            )
        }
        RefKind::LifecycleEvent => {
            let event = lifecycle
                .events_for_feature(feature_uid)
                .map_err(|error| error.to_string())?
                .into_iter()
                .find(|event| event.uid == reference)
                .ok_or_else(|| "Lifecycle event is not recorded for this feature.".to_owned())?;
            (
                SpecTraceTargetKindV1::LifecycleEvent,
                format!("Lifecycle event {}", event.uid),
                format!("{:?}", event.event),
            )
        }
        RefKind::DecisionOrReceipt => {
            resolve_decision_or_receipt(lifecycle, store, feature_uid, reference)?
        }
        RefKind::OperatorDecision => resolve_operator_decision(lifecycle, feature_uid, reference)?,
        RefKind::Test => {
            let (readiness, _) = crate::core::planning::QuestionStore::new(store)
                .load_readiness(feature_uid)
                .map_err(|error| error.to_string())?
                .ok_or_else(|| "Acceptance-test trace is not recorded.".to_owned())?;
            if !readiness
                .acceptance_tests
                .iter()
                .any(|test| test.test_ref == reference)
            {
                return Err("Test reference is not recorded for this feature.".into());
            }
            (
                SpecTraceTargetKindV1::TestReference,
                "Acceptance test reference".into(),
                reference.to_owned(),
            )
        }
        RefKind::Evidence => (
            SpecTraceTargetKindV1::EvidenceReference,
            "Evidence reference".into(),
            reference.to_owned(),
        ),
    };
    Ok(target)
}

fn resolve_decision_or_receipt(
    lifecycle: &LifecycleStore<'_>,
    store: &crate::artifacts::planning_store::PlanningStore,
    feature_uid: &str,
    reference: &str,
) -> Result<(SpecTraceTargetKindV1, String, String), String> {
    if lifecycle
        .spec_revision_history(feature_uid)
        .map_err(|error| error.to_string())?
        .into_iter()
        .flat_map(|(_, deltas)| deltas)
        .any(|delta| {
            delta
                .operator_decision
                .is_some_and(|decision| decision.receipt_uid == reference)
        })
    {
        return resolve_operator_decision(lifecycle, feature_uid, reference);
    }
    if let Some(question) = crate::core::planning::QuestionStore::new(store)
        .list()
        .map_err(|error| error.to_string())?
        .into_iter()
        .find(|question| question.uid == reference && question.feature_uid == feature_uid)
    {
        return Ok((
            SpecTraceTargetKindV1::Decision,
            format!("Decision {}", question.uid),
            format!("{:?} · {}", question.status, question.question),
        ));
    }
    for task in lifecycle.list_tasks().map_err(|error| error.to_string())? {
        if task.feature_uid != feature_uid {
            continue;
        }
        for (receipt, kind) in task_receipts(&task) {
            if receipt.receipt_uid == reference {
                return Ok((
                    kind,
                    format!(
                        "{} receipt",
                        if kind == SpecTraceTargetKindV1::CodeReview {
                            "Code review"
                        } else {
                            "Runtime QA"
                        }
                    ),
                    receipt_summary(receipt),
                ));
            }
        }
    }
    for event in lifecycle
        .events_for_feature(feature_uid)
        .map_err(|error| error.to_string())?
    {
        let receipt = match event.event {
            LifecycleEventKind::Feature(
                crate::core::lifecycle::FeatureEvent::UserPlanDecision(receipt),
            ) => Some((receipt, SpecTraceTargetKindV1::Decision)),
            LifecycleEventKind::Feature(crate::core::lifecycle::FeatureEvent::PlanReview(
                receipt,
            )) => Some((receipt, SpecTraceTargetKindV1::CodeReview)),
            _ => None,
        };
        if let Some((receipt, kind)) =
            receipt.filter(|(receipt, _)| receipt.receipt_uid == reference)
        {
            return Ok((kind, "Plan gate receipt".into(), receipt_summary(&receipt)));
        }
    }
    Err("Decision or review receipt is not persisted for this feature.".into())
}

fn resolve_operator_decision(
    lifecycle: &LifecycleStore<'_>,
    feature_uid: &str,
    reference: &str,
) -> Result<(SpecTraceTargetKindV1, String, String), String> {
    let found = lifecycle
        .spec_revision_history(feature_uid)
        .map_err(|error| error.to_string())?
        .into_iter()
        .flat_map(|(_, deltas)| deltas)
        .find_map(|delta| {
            delta
                .operator_decision
                .filter(|decision| decision.receipt_uid == reference)
        })
        .ok_or_else(|| "Operator decision receipt is not persisted.".to_owned())?;
    let authorization = lifecycle
        .load_feature_delta_authorization(reference)
        .map_err(|_| "User authorization receipt is not persisted for this decision.".to_owned())?;
    if authorization.feature_uid != feature_uid
        || authorization.decision != found.decision
        || authorization.actor != found.actor
    {
        return Err("Operator decision does not match its persisted authorization.".into());
    }
    Ok((
        SpecTraceTargetKindV1::OperatorDecision,
        format!("Operator decision {}", found.receipt_uid),
        format!(
            "{:?} by {:?}; authorized for requirement {} at specification revision {}",
            authorization.decision,
            authorization.actor,
            authorization.requirement_id,
            authorization.source_spec_sequence
        ),
    ))
}

fn task_receipts(
    task: &crate::core::lifecycle::TaskRecord,
) -> Vec<(&GateReceipt, SpecTraceTargetKindV1)> {
    task.code_review
        .iter()
        .chain(task.review_history.iter())
        .map(|receipt| (receipt, SpecTraceTargetKindV1::CodeReview))
        .chain(
            task.runtime_qa
                .iter()
                .map(|receipt| (receipt, SpecTraceTargetKindV1::RuntimeQa)),
        )
        .collect()
}

fn receipt_summary(receipt: &GateReceipt) -> String {
    format!(
        "{:?} · {:?} · {}",
        receipt.actor,
        receipt.decision,
        receipt.evidence_refs.join(", ")
    )
}

fn parse_pr_reference(reference: &str) -> Result<(&str, u64), String> {
    let rest = reference
        .strip_prefix("provider-pr-state:")
        .ok_or_else(|| "PR-state reference format is invalid.".to_owned())?;
    let (uid, revision) = rest
        .rsplit_once(':')
        .ok_or_else(|| "PR-state reference format is invalid.".to_owned())?;
    Ok((
        uid,
        revision
            .parse()
            .map_err(|_| "PR-state revision is invalid.".to_owned())?,
    ))
}
