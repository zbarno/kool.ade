use super::super::{TaskIntentEventEvidenceV1, TaskIntentEventKindV1, TaskIntentGateKindV1};
use super::{record_gate, record_if_new_with_evidence};

pub(super) fn capture(
    root: &mut crate::core::planning_work::Work,
    task_documents: &[crate::artifacts::task_docs::TaskDocument],
    tasks: &[crate::core::lifecycle::TaskRecord],
    deployments: &[(
        crate::core::lifecycle::DeploymentEvidenceV1,
        crate::core::lifecycle::CurrentImplementationEvidenceV1,
    )],
    task_work: &[crate::core::planning_work::Work],
    now: u64,
) -> bool {
    let mut changed = false;
    let document_owners = task_documents
        .iter()
        .filter_map(|document| {
            crate::artifacts::task_docs::task_intent_uid_for_document(
                document,
                task_documents,
                task_work,
            )
            .map(|uid| (document, uid))
        })
        .collect::<Vec<_>>();
    let mut task_cycles = std::collections::BTreeMap::new();
    for (document, owner_uid) in document_owners {
        if owner_uid != root.uid {
            continue;
        }
        let Some(metadata) = document.metadata.as_ref() else {
            continue;
        };
        let cycle_uid = crate::artifacts::task_docs::task_intent_cycle_uid_for_document(
            document,
            task_documents,
            task_work,
        );
        if cycle_uid.is_none() && root.task_intent_cycles.len() > 1 {
            continue;
        }
        task_cycles.insert(metadata.uid.clone(), cycle_uid.clone());
        let Some(state) = document.task_state.as_ref() else {
            continue;
        };
        if state.status == crate::core::planning_work::WorkStatus::Done {
            changed |= record_if_new_with_evidence(
                root,
                TaskIntentEventKindV1::DeliveryOutcomeRecorded,
                &format!("Task {} completed", document.title),
                cycle_uid.clone(),
                Some(metadata.uid.clone()),
                now,
                Some(TaskIntentEventEvidenceV1::TaskOutcome {
                    task_uid: metadata.uid.clone(),
                    phase: None,
                    status: Some(state.status),
                    pull_request: None,
                }),
            );
        }
        for pull_request in &state.pull_requests {
            changed |= record_if_new_with_evidence(
                root,
                TaskIntentEventKindV1::DeliveryOutcomeRecorded,
                &format!(
                    "Task {} pull request #{} recorded as {:?}",
                    document.title, pull_request.number, pull_request.status
                ),
                cycle_uid.clone(),
                Some(metadata.uid.clone()),
                now,
                Some(TaskIntentEventEvidenceV1::TaskOutcome {
                    task_uid: metadata.uid.clone(),
                    phase: None,
                    status: Some(state.status),
                    pull_request: Some(pull_request.clone()),
                }),
            );
        }
    }
    for task in tasks {
        let Some(cycle_uid) = task_cycles.get(&task.task_uid) else {
            continue;
        };
        for receipt in task.review_history.iter().chain(task.code_review.iter()) {
            changed |= record_gate(
                root,
                receipt,
                cycle_uid.clone(),
                &task.task_uid,
                TaskIntentGateKindV1::IndependentReview,
                now,
            );
        }
        if let Some(receipt) = &task.runtime_qa {
            changed |= record_gate(
                root,
                receipt,
                cycle_uid.clone(),
                &task.task_uid,
                TaskIntentGateKindV1::RuntimeQa,
                now,
            );
        }
        if let Some(receipt) = &task.spec_reconciliation {
            changed |= record_gate(
                root,
                receipt,
                cycle_uid.clone(),
                &task.task_uid,
                TaskIntentGateKindV1::SpecificationReconciliation,
                now,
            );
        }
        if matches!(
            task.phase,
            crate::core::lifecycle::TaskPhase::Blocked
                | crate::core::lifecycle::TaskPhase::Done
                | crate::core::lifecycle::TaskPhase::PrOpened
                | crate::core::lifecycle::TaskPhase::PrMerged
                | crate::core::lifecycle::TaskPhase::Failed
                | crate::core::lifecycle::TaskPhase::Cancelled
        ) {
            changed |= record_if_new_with_evidence(
                root,
                TaskIntentEventKindV1::DeliveryOutcomeRecorded,
                &format!("Task outcome recorded as {:?}", task.phase),
                cycle_uid.clone(),
                Some(task.task_uid.clone()),
                now,
                Some(TaskIntentEventEvidenceV1::TaskOutcome {
                    task_uid: task.task_uid.clone(),
                    phase: Some(task.phase),
                    status: None,
                    pull_request: None,
                }),
            );
        }
    }
    for (deployment, current) in deployments {
        if !deployment.is_successful_production_for_current(current) {
            continue;
        }
        let matching_cycles = root
            .task_intent_cycles
            .iter()
            .filter(|cycle| {
                task_work.iter().any(|candidate| {
                    candidate.uid == cycle.work_uid
                        && candidate.feature_uid.as_deref() == Some(current.feature_uid.as_str())
                })
            })
            .collect::<Vec<_>>();
        if matching_cycles.len() != 1 {
            continue;
        }
        let cycle = matching_cycles[0];
        changed |= record_if_new_with_evidence(
            root,
            TaskIntentEventKindV1::DeliveryOutcomeRecorded,
            &format!(
                "Production deployment succeeded for merge {} at {}",
                deployment.merge_commit_sha, deployment.observed_at_ms
            ),
            Some(cycle.uid.clone()),
            Some(current.uid.clone()),
            now,
            Some(TaskIntentEventEvidenceV1::Deployment {
                deployment: deployment.clone(),
                current_implementation: current.clone(),
            }),
        );
    }
    changed
}
