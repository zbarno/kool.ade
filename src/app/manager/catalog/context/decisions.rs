use crate::app::session::Project;
use crate::harness::prompt_catalog::{EvidenceConfidentiality, EvidenceItem, EvidenceOrigin};

const MAX_DECISIONS: usize = 8;

pub(crate) fn validated_decisions(project: &Project, workspace_uid: &str) -> Vec<EvidenceItem> {
    let active_feature = project
        .state
        .active_feature
        .as_ref()
        .and_then(|(_, markdown)| {
            crate::domain::ArtifactIdentity::from_markdown(markdown)
                .ok()
                .flatten()
        })
        .map(|identity| identity.uid);
    let Some(active_feature) = active_feature else {
        return Vec::new();
    };
    project
        .task_documents
        .iter()
        .filter_map(|document| {
            let metadata = document.metadata.as_ref()?;
            let intent_uid = metadata.task_intent_uid.as_deref()?;
            let task = crate::core::lifecycle::LifecycleStore::new(&project.state.planning_store)
                .load_task(&metadata.uid)
                .ok()?;
            if task.feature_uid != active_feature {
                return None;
            }
            let root = project
                .planning_work
                .iter()
                .find(|work| work.uid == intent_uid)?;
            let has_active_direction = root.task_decisions.iter().any(|decision| {
                decision.task_uid == task.task_uid
                    && decision.status == crate::core::planning_work::TaskDecisionStatusV1::Active
            });
            if !has_active_direction {
                return None;
            }
            if root.task_intent_cycle_uid.as_deref() != metadata.task_intent_cycle_uid.as_deref() {
                return Some(stale_decision_evidence(workspace_uid, &task.task_uid));
            }
            let context = match crate::core::planning_work::task_decision_context(
                &root.task_decisions,
                &task.task_uid,
                intent_uid,
                metadata.task_intent_cycle_uid.as_deref(),
                &task.feature_uid,
                task.task_spec_sha256.as_deref().unwrap_or_default(),
                &task.approved_plan_sha256,
            ) {
                Ok(Some(context)) => context,
                Ok(None) => return None,
                Err(_) => return Some(stale_decision_evidence(workspace_uid, &task.task_uid)),
            };
            Some(EvidenceItem {
                id: format!("authorized-task-direction:{}", task.task_uid),
                workspace_uid: workspace_uid.to_owned(),
                origin: EvidenceOrigin::Application,
                confidentiality: EvidenceConfidentiality::WorkspacePrivate,
                source: format!(
                    "validated current user directions for task {} at cycle {}",
                    task.task_uid,
                    metadata.task_intent_cycle_uid.as_deref().unwrap_or("none")
                ),
                content: crate::core::context_build::clip(&context, 4_000),
            })
        })
        .take(MAX_DECISIONS)
        .collect()
}

fn stale_decision_evidence(workspace_uid: &str, task_uid: &str) -> EvidenceItem {
    EvidenceItem {
        id: format!("stale-authorized-task-direction:{task_uid}"),
        workspace_uid: workspace_uid.to_owned(),
        origin: EvidenceOrigin::Application,
        confidentiality: EvidenceConfidentiality::WorkspacePrivate,
        source: "stale user direction excluded by current task or plan binding".into(),
        content: "An earlier active direction failed current task, cycle, task-specification, or approved-plan checks and was omitted. Do not apply it as current guidance.".into(),
    }
}
