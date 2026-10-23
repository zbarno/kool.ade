use sha2::Digest;

use crate::{
    app::session::Project,
    core::lifecycle::{LifecycleEventKind, LifecycleEventRecord, ProductSpecFindingV1},
    harness::prompt_catalog::{EvidenceConfidentiality, EvidenceItem, EvidenceOrigin},
};

pub(super) fn specification_finding_evidence(
    workspace_uid: &str,
    findings: &[ProductSpecFindingV1],
) -> Vec<EvidenceItem> {
    findings
        .iter()
        .take(32)
        .map(|finding| {
            let identity = format!(
                "{workspace_uid}\0{}\0{}\0{}\0{}",
                finding.feature_uid,
                finding.revision,
                finding.event_id,
                finding.requirement_id
            );
            let digest = format!("{:x}", sha2::Sha256::digest(identity.as_bytes()));
            EvidenceItem {
                id: format!("spec-finding:{}", &digest[..24]),
                workspace_uid: workspace_uid.to_owned(),
                origin: EvidenceOrigin::Application,
                confidentiality: EvidenceConfidentiality::WorkspacePrivate,
                source: "validated persisted specification finding".into(),
                content: format!(
                    "feature_uid={} revision={} event_id={} requirement_id={} classification={:?} evidence_refs={} test_refs={} affected_task_ids={} needs_user_reapproval={}",
                    finding.feature_uid,
                    finding.revision,
                    finding.event_id,
                    finding.requirement_id,
                    finding.classification,
                    finding.evidence_refs.iter().take(16).cloned().collect::<Vec<_>>().join(","),
                    finding.test_refs.iter().take(16).cloned().collect::<Vec<_>>().join(","),
                    finding.affected_task_ids.iter().take(16).cloned().collect::<Vec<_>>().join(","),
                    finding.needs_user_reapproval
                ),
            }
        })
        .collect()
}

pub(super) fn typed_event_content(
    project: &Project,
    event: &LifecycleEventRecord,
) -> Option<String> {
    use crate::core::lifecycle::{FeatureEvent, TaskEvent};
    let summary = match &event.event {
        LifecycleEventKind::Feature(FeatureEvent::PlanReview(receipt)) => format!(
            "feature plan review: decision={:?} subject_sha256={} actor={:?}",
            receipt.decision, receipt.subject_sha256, receipt.actor
        ),
        LifecycleEventKind::Feature(event) => format!("feature lifecycle transition: {event:?}"),
        LifecycleEventKind::Task(TaskEvent::RuntimeQaCompleted {
            request_id,
            receipt,
            result_sha256,
            evidence_manifest_sha256,
            ..
        }) => qa_summary(
            project,
            event,
            request_id,
            receipt,
            result_sha256,
            evidence_manifest_sha256,
        )?,
        LifecycleEventKind::Task(TaskEvent::CodeReview { receipt, .. }) => format!(
            "independent code review: decision={:?} subject_sha256={} actor={:?}",
            receipt.decision, receipt.subject_sha256, receipt.actor
        ),
        LifecycleEventKind::Task(TaskEvent::ReconcileSpec { receipt, .. })
        | LifecycleEventKind::Task(TaskEvent::PublicationGatesRefreshed { receipt })
        | LifecycleEventKind::Task(TaskEvent::SubtaskReconciled { receipt }) => format!(
            "specification reconciliation receipt: decision={:?} subject_sha256={} actor={:?}",
            receipt.decision, receipt.subject_sha256, receipt.actor
        ),
        LifecycleEventKind::PullRequestState {
            work_item_uid,
            status,
        } => format!("pull request work item {work_item_uid} state: {status:?}"),
        LifecycleEventKind::Task(event) => format!("task lifecycle transition: {event:?}"),
        LifecycleEventKind::FeatureCreated => "feature created".into(),
        LifecycleEventKind::TaskCreated => "task created".into(),
        LifecycleEventKind::SubtaskCreated { parent_task_uid } => {
            format!("subtask created under parent {parent_task_uid}")
        }
    };
    Some(format!(
        "event_uid={} feature_uid={} task_uid={:?} actor={:?} created_at_ms={} revision={} typed_event={summary}",
        event.uid,
        event.feature_uid,
        event.task_uid,
        event.actor,
        event.created_at_ms,
        event.revision
    ))
}

fn qa_summary(
    project: &Project,
    event: &LifecycleEventRecord,
    request_id: &str,
    receipt: &crate::core::lifecycle::GateReceipt,
    result_sha256: &str,
    evidence_manifest_sha256: &str,
) -> Option<String> {
    let qa_store = crate::runtime_qa::RuntimeQaStore::new(
        project.state.planning_store.root.join("state/runtime-qa"),
    );
    let (request, _, _, result) = qa_store.load_run(request_id).ok()?;
    if request.qa_run_id != request_id
        || request.parent_task_uid != event.task_uid.as_deref().unwrap_or_default()
        || digest_json(&result)? != result_sha256
    {
        return None;
    }
    let findings = result
        .findings
        .iter()
        .take(32)
        .map(|finding| {
            format!(
                "finding={} category={:?} severity={:?} requirement={} expected={} observed={} smallest_fix={} steps={}",
                finding.finding_uid,
                finding.category,
                finding.severity,
                finding.requirement_uids.join(","),
                crate::core::context_build::clip(&finding.expected, 500),
                crate::core::context_build::clip(&finding.actual, 500),
                crate::core::context_build::clip(&finding.smallest_fix, 500),
                crate::core::context_build::clip(&finding.reproducible_steps.join(" -> "), 1000)
            )
        })
        .collect::<Vec<_>>();
    Some(format!(
        "accepted typed runtime QA report: outcome={:?} receipt={:?} result_sha256={} evidence_sha256={} findings=[{}]",
        result.outcome,
        receipt.decision,
        result_sha256,
        evidence_manifest_sha256,
        findings.join("; ")
    ))
}

fn digest_json(value: &impl serde::Serialize) -> Option<String> {
    Some(format!(
        "{:x}",
        sha2::Sha256::digest(serde_json::to_vec(value).ok()?)
    ))
}

#[cfg(test)]
#[path = "events/tests.rs"]
mod tests;
