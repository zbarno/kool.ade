use super::super::{TaskIntentEventEvidenceV1, TaskIntentEventKindV1, TaskIntentGateKindV1, Work};
use crate::core::planning_work::TaskIntentEventV1;

pub(super) fn validate(root: &Work, work: &[Work]) -> anyhow::Result<()> {
    let mut event_uids = std::collections::BTreeSet::new();
    let mut previous_recorded_at_ms = 0;
    for event in &root.task_intent_history {
        anyhow::ensure!(
            uuid::Uuid::parse_str(&event.event_uid).is_ok()
                && event_uids.insert(event.event_uid.as_str())
                && event.recorded_at_ms > previous_recorded_at_ms
                && !event.summary.trim().is_empty()
                && event.summary.len() <= 2048
                && !event
                    .summary
                    .chars()
                    .any(|ch| ch.is_control() && !matches!(ch, '\n' | '\t'))
                && event
                    .related_uid
                    .as_deref()
                    .is_none_or(|uid| uuid::Uuid::parse_str(uid).is_ok())
                && event.cycle_uid.as_deref().is_none_or(|cycle_uid| {
                    uuid::Uuid::parse_str(cycle_uid).is_ok()
                        && root
                            .task_intent_cycles
                            .iter()
                            .any(|cycle| cycle.uid == cycle_uid)
                })
                && event.evidence.as_ref().is_none_or(|evidence| {
                    expected_kind(event, evidence)
                        && evidence.validate(event.related_uid.as_deref())
                        && evidence_is_linked_to_cycle(root, event, evidence, work)
                }),
            "TaskIntent history contains an invalid event"
        );
        previous_recorded_at_ms = event.recorded_at_ms;
    }
    Ok(())
}

fn expected_kind(event: &TaskIntentEventV1, evidence: &TaskIntentEventEvidenceV1) -> bool {
    match evidence {
        TaskIntentEventEvidenceV1::Decision { .. } => {
            event.kind == TaskIntentEventKindV1::DecisionRecorded
        }
        TaskIntentEventEvidenceV1::PlanDecision { .. } => {
            event.kind == TaskIntentEventKindV1::PlanDecisionRecorded
        }
        TaskIntentEventEvidenceV1::GateReceipt { gate, .. } => match gate {
            TaskIntentGateKindV1::IndependentReview => {
                event.kind == TaskIntentEventKindV1::ReviewRecorded
            }
            TaskIntentGateKindV1::RuntimeQa => {
                event.kind == TaskIntentEventKindV1::QualityAssuranceRecorded
            }
            TaskIntentGateKindV1::SpecificationReconciliation => {
                event.kind == TaskIntentEventKindV1::SpecificationReconciliationRecorded
            }
        },
        TaskIntentEventEvidenceV1::TaskOutcome { .. }
        | TaskIntentEventEvidenceV1::Deployment { .. } => {
            event.kind == TaskIntentEventKindV1::DeliveryOutcomeRecorded
        }
    }
}

fn evidence_is_linked_to_cycle(
    root: &Work,
    event: &TaskIntentEventV1,
    evidence: &TaskIntentEventEvidenceV1,
    work: &[Work],
) -> bool {
    let feature_uid = match evidence {
        TaskIntentEventEvidenceV1::PlanDecision { feature_uid, .. } => Some(feature_uid),
        TaskIntentEventEvidenceV1::Deployment {
            current_implementation,
            ..
        } => Some(&current_implementation.feature_uid),
        _ => None,
    };
    let Some(feature_uid) = feature_uid else {
        return true;
    };
    if let Some(cycle_uid) = event.cycle_uid.as_deref() {
        let Some(cycle) = root
            .task_intent_cycles
            .iter()
            .find(|cycle| cycle.uid == cycle_uid)
        else {
            return false;
        };
        return work
            .iter()
            .find(|candidate| candidate.uid == cycle.work_uid)
            .is_some_and(|candidate| {
                candidate.task_intent_root_uid() == Some(root.uid.as_str())
                    && candidate.feature_uid.as_deref() == Some(feature_uid.as_str())
            });
    }
    work.iter().any(|candidate| {
        candidate.task_intent_root_uid() == Some(root.uid.as_str())
            && candidate.feature_uid.as_deref() == Some(feature_uid.as_str())
    })
}
