use sha2::Digest;

use crate::{
    app::session::Project,
    harness::prompt_catalog::{EvidenceConfidentiality, EvidenceItem, EvidenceOrigin},
    harness::{DependencyRequest, DependencyRequestStatus},
};

pub(super) fn dependency_request_evidence(
    project: &Project,
    workspace_uid: &str,
) -> Vec<EvidenceItem> {
    let mut activities = project
        .activity
        .tasks
        .iter()
        .filter_map(|(ticket, progress)| {
            let task_uid = project
                .task_documents
                .iter()
                .find(|document| document.path == *ticket)?
                .identity
                .as_ref()?
                .uid
                .clone();
            Some((progress, task_uid))
        })
        .collect::<Vec<_>>();
    activities.sort_by_key(|(progress, _)| progress.telemetry.updated_ms);
    activities
        .into_iter()
        .rev()
        .flat_map(|(progress, task_uid)| {
            progress
                .dependency_requests
                .iter()
                .rev()
                .filter(|request| request.status.is_actionable_evidence())
                .take(8)
                .map(move |request| evidence_item(workspace_uid, &task_uid, request))
        })
        .take(32)
        .collect()
}

trait EvidenceStatus {
    fn is_actionable_evidence(&self) -> bool;
}

impl EvidenceStatus for DependencyRequestStatus {
    fn is_actionable_evidence(&self) -> bool {
        matches!(
            self,
            Self::AwaitingUser | Self::Denied | Self::Failed | Self::Prepared
        )
    }
}

fn evidence_item(workspace_uid: &str, task_uid: &str, request: &DependencyRequest) -> EvidenceItem {
    let digest = format!(
        "{:x}",
        sha2::Sha256::digest(format!("{workspace_uid}\0{task_uid}\0{}", request.id).as_bytes())
    );
    let preparation = request
        .preparation
        .as_ref()
        .and_then(|preparation| preparation.status);
    EvidenceItem {
        id: format!("broker-prerequisite:{}", &digest[..24]),
        workspace_uid: workspace_uid.to_owned(),
        origin: EvidenceOrigin::Application,
        confidentiality: EvidenceConfidentiality::WorkspacePrivate,
        source: "typed dependency broker activity".into(),
        content: format!(
            "task_uid={task_uid} status={:?} category={:?} decision={:?} ecosystem={:?} dependency_kind={:?} preparation={preparation:?}",
            request.status,
            request.category,
            request.decision,
            request.need.ecosystem,
            request.need.kind
        ),
    }
}

#[cfg(test)]
#[path = "broker/tests.rs"]
mod tests;
