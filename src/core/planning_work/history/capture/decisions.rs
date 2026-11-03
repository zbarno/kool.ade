use super::super::{TaskIntentEventEvidenceV1, TaskIntentEventKindV1};
use super::record_if_new_with_evidence;

pub(super) fn capture(
    root: &mut crate::core::planning_work::Work,
    items: &[crate::domain::OpenItem],
    plan_decisions: &[(String, crate::core::lifecycle::GateReceipt)],
    task_work: &[crate::core::planning_work::Work],
    now: u64,
) -> bool {
    let mut changed = false;
    let root_uid = root.uid.clone();
    for decision in items
        .iter()
        .filter(|item| item.task_intent_uid.as_deref() == Some(root_uid.as_str()))
    {
        let cycle_uid = decision.task_intent_cycle_uid.clone().or_else(|| {
            (root.task_intent_cycles.len() == 1)
                .then(|| {
                    root.task_intent_cycles
                        .first()
                        .map(|cycle| cycle.uid.clone())
                })
                .flatten()
        });
        let decision_uid = decision
            .uid
            .clone()
            .filter(|uid| uuid::Uuid::parse_str(uid).is_ok());
        let summary = format!(
            "Decision {} recorded as {:?}: {}. Evidence: {}",
            decision.id, decision.status, decision.question, decision.evidence
        );
        changed |= record_if_new_with_evidence(
            root,
            TaskIntentEventKindV1::DecisionRecorded,
            &summary,
            cycle_uid,
            decision_uid.clone(),
            now,
            Some(TaskIntentEventEvidenceV1::Decision {
                decision_uid,
                status: decision.status,
                subject_sha256: sha256_serialized(&(
                    decision.uid.as_deref(),
                    decision.id.as_str(),
                    decision.status,
                    decision.question.as_str(),
                    decision.evidence.as_str(),
                )),
            }),
        );
    }

    for (feature_uid, receipt) in plan_decisions.iter().filter(|(_, receipt)| {
        receipt.actor == crate::core::lifecycle::GateActor::User
            && receipt.decision != crate::core::lifecycle::GateDecision::NotApplicable
    }) {
        let matching_cycles = root
            .task_intent_cycles
            .iter()
            .filter(|cycle| {
                task_work
                    .iter()
                    .find(|candidate| candidate.uid == cycle.work_uid)
                    .is_some_and(|candidate| {
                        candidate.feature_uid.as_deref() == Some(feature_uid.as_str())
                    })
            })
            .collect::<Vec<_>>();
        let matching_roots = task_work
            .iter()
            .filter(|candidate| {
                candidate.parent_uid.is_none()
                    && candidate.task_intent_root_uid() == Some(candidate.uid.as_str())
            })
            .filter(|candidate| {
                candidate.task_intent_cycles.iter().any(|cycle| {
                    task_work
                        .iter()
                        .find(|work| work.uid == cycle.work_uid)
                        .is_some_and(|work| {
                            work.feature_uid.as_deref() == Some(feature_uid.as_str())
                        })
                })
            })
            .count();
        if matching_cycles.is_empty() || matching_roots != 1 {
            continue;
        }
        let cycle_uid = (matching_cycles.len() == 1).then(|| matching_cycles[0].uid.clone());
        changed |= record_if_new_with_evidence(
            root,
            TaskIntentEventKindV1::PlanDecisionRecorded,
            &format!("Plan decision recorded as {:?}", receipt.decision),
            cycle_uid,
            Some(receipt.subject_uid.clone()),
            now,
            Some(TaskIntentEventEvidenceV1::PlanDecision {
                feature_uid: feature_uid.clone(),
                snapshot_uid: receipt.subject_uid.clone(),
                snapshot_sha256: receipt.subject_sha256.clone(),
                receipt: receipt.clone(),
            }),
        );
    }
    changed
}

fn sha256_serialized(value: &impl serde::Serialize) -> String {
    use sha2::Digest as _;
    let bytes = serde_json::to_vec(value).expect("bounded planning decision serializes");
    format!("{:x}", sha2::Sha256::digest(bytes))
}
