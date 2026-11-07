use crate::{
    app::session::Project,
    core::implementation::FailureKind,
    harness::prompt_catalog::{EvidenceConfidentiality, EvidenceItem, EvidenceOrigin},
};
use sha2::Digest;

#[cfg(test)]
#[path = "failures/tests.rs"]
mod tests;

pub(super) fn task_failure_evidence(project: &Project, workspace_uid: &str) -> Vec<EvidenceItem> {
    project
        .queue
        .blocked
        .iter()
        .take(16)
        .map(|(task, failure)| {
            let task_uid = project
                .task_documents
                .iter()
                .find(|document| &document.path == task)
                .and_then(|document| document.identity.as_ref())
                .map(|identity| identity.uid.as_str())
                .unwrap_or("unlinked-task");
            let digest = task_failure_evidence_digest(workspace_uid, task_uid, task);
            let mismatch = failure.kind == FailureKind::ProviderCapabilityMismatch;
            EvidenceItem {
                id: format!(
                    "{}:{}",
                    if mismatch {
                        "provider-capability-mismatch"
                    } else {
                        "implementation-failure"
                    },
                    &digest[..24]
                ),
                workspace_uid: workspace_uid.into(),
                origin: EvidenceOrigin::Application,
                confidentiality: EvidenceConfidentiality::WorkspacePrivate,
                source: if mismatch {
                    "typed provider capability mismatch"
                } else {
                    "typed implementation queue failure"
                }
                .into(),
                content: if mismatch {
                    format!(
                        "task_uid={task_uid} recovery={:?} observed_capability_mismatch={}",
                        failure.recovery,
                        crate::core::context_build::clip(&failure.message, 512)
                    )
                } else {
                    format!(
                        "task_uid={task_uid} failure_kind={:?} recovery={:?} observed_failure={}",
                        failure.kind,
                        failure.recovery,
                        crate::core::context_build::clip(&failure.message, 1200)
                    )
                },
            }
        })
        .collect()
}

fn task_failure_evidence_digest(workspace_uid: &str, task_uid: &str, task_path: &str) -> String {
    format!(
        "{:x}",
        sha2::Sha256::digest(format!("{workspace_uid}\0{task_uid}\0{task_path}").as_bytes())
    )
}
