use super::{
    TaskIntentEventEvidenceV1, TaskIntentEventKindV1, TaskIntentEventV1, TaskIntentGateKindV1,
};

mod decisions;
mod delivery;

pub fn capture_authoritative_events(
    work: &mut [crate::core::planning_work::Work],
    items: &[crate::domain::OpenItem],
    task_documents: &[crate::artifacts::task_docs::TaskDocument],
    plan_decisions: &[(String, crate::core::lifecycle::GateReceipt)],
    tasks: &[crate::core::lifecycle::TaskRecord],
    deployments: &[(
        crate::core::lifecycle::DeploymentEvidenceV1,
        crate::core::lifecycle::CurrentImplementationEvidenceV1,
    )],
) -> bool {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(1, |elapsed| {
            elapsed.as_millis().max(1).min(u128::from(u64::MAX)) as u64
        });
    let task_work = work.to_vec();
    let mut changed = false;
    for root in work.iter_mut().filter(|item| {
        item.parent_uid.is_none() && item.task_intent_root_uid() == Some(item.uid.as_str())
    }) {
        changed |= decisions::capture(root, items, plan_decisions, &task_work, now);
        changed |= delivery::capture(root, task_documents, tasks, deployments, &task_work, now);
    }
    changed
}

fn record_if_new_with_evidence(
    root: &mut crate::core::planning_work::Work,
    kind: TaskIntentEventKindV1,
    summary: &str,
    cycle_uid: Option<String>,
    related_uid: Option<String>,
    now: u64,
    evidence: Option<TaskIntentEventEvidenceV1>,
) -> bool {
    let summary = clip_event_summary(summary).to_owned();
    if root.task_intent_history.iter().any(|event| {
        event.kind == kind
            && event.summary == summary
            && event.cycle_uid == cycle_uid
            && event.related_uid == related_uid
            && event.evidence == evidence
    }) {
        return false;
    }
    let previous = root
        .task_intent_history
        .last()
        .map_or(0, |event| event.recorded_at_ms);
    root.task_intent_history.push(TaskIntentEventV1 {
        event_uid: uuid::Uuid::new_v4().hyphenated().to_string(),
        kind,
        recorded_at_ms: now.max(previous.saturating_add(1)),
        summary,
        cycle_uid,
        related_uid,
        evidence,
    });
    true
}

fn record_gate(
    root: &mut crate::core::planning_work::Work,
    receipt: &crate::core::lifecycle::GateReceipt,
    cycle_uid: Option<String>,
    task_uid: &str,
    gate: TaskIntentGateKindV1,
    now: u64,
) -> bool {
    let (kind, label) = match gate {
        TaskIntentGateKindV1::IndependentReview => {
            (TaskIntentEventKindV1::ReviewRecorded, "Independent review")
        }
        TaskIntentGateKindV1::RuntimeQa => (
            TaskIntentEventKindV1::QualityAssuranceRecorded,
            "Runtime QA",
        ),
        TaskIntentGateKindV1::SpecificationReconciliation => (
            TaskIntentEventKindV1::SpecificationReconciliationRecorded,
            "Specification reconciliation",
        ),
    };
    record_if_new_with_evidence(
        root,
        kind,
        &format!(
            "{label} {:?} receipt {} for reviewed subject {}",
            receipt.decision, receipt.receipt_uid, receipt.subject_sha256
        ),
        cycle_uid,
        Some(task_uid.to_owned()),
        now,
        Some(TaskIntentEventEvidenceV1::GateReceipt {
            task_uid: task_uid.to_owned(),
            gate,
            receipt: receipt.clone(),
        }),
    )
}

pub fn clip_event_summary(value: &str) -> &str {
    clip_utf8_bytes(value, 2048)
}

fn clip_utf8_bytes(value: &str, max_bytes: usize) -> &str {
    let mut end = value.len().min(max_bytes);
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    &value[..end]
}
