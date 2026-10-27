use super::{code_review, runtime_qa_evidence, task_relations::relation_for_planning_uid};
use crate::{app::session::Project, core::lifecycle::LifecycleStore};

pub fn for_ticket(
    project: &Project,
    ticket: &str,
) -> (
    Option<crate::ui::task_detail::LifecycleView>,
    Option<String>,
) {
    let Some(document) = project
        .task_documents
        .iter()
        .find(|document| document.path == ticket)
    else {
        return (None, None);
    };
    let Some(planning_item_uid) = document
        .identity
        .as_ref()
        .map(|identity| identity.uid.as_str())
    else {
        return (None, None);
    };
    let lifecycle = LifecycleStore::new(&project.state.planning_store);
    let task = match lifecycle.task_for_planning_item(planning_item_uid) {
        Ok(Some(task)) => task,
        Ok(None) => return (None, None),
        Err(error) => return (None, Some(error.to_string())),
    };
    let feature = match lifecycle.load_feature(&task.feature_uid) {
        Ok(feature) => feature,
        Err(error) => return (None, Some(error.to_string())),
    };
    let spec_revision = match lifecycle.load_spec_revision(&task.feature_uid) {
        Ok(revision) => revision,
        Err(error) => return (None, Some(error.to_string())),
    };
    let pending_spec_triggers = match lifecycle.list_spec_sync_queue(&task.feature_uid, false) {
        Ok(entries) => entries.len(),
        Err(error) => return (None, Some(error.to_string())),
    };
    let parent_task = task
        .parent_task_uid
        .as_deref()
        .and_then(|parent_uid| lifecycle.load_task(parent_uid).ok())
        .and_then(|parent| {
            relation_for_planning_uid(project, Some(parent.planning_item_uid.as_deref()?))
        });
    let dependencies = document
        .metadata
        .as_ref()
        .into_iter()
        .flat_map(|metadata| metadata.dependency_uids.iter())
        .filter_map(|uid| relation_for_planning_uid(project, Some(uid)))
        .collect();
    let subtasks = project
        .task_documents
        .iter()
        .filter(|candidate| {
            candidate.metadata.as_ref().is_some_and(|metadata| {
                metadata.parent_task_uid.as_deref() == Some(planning_item_uid)
            })
        })
        .filter_map(|child| {
            relation_for_planning_uid(
                project,
                child
                    .metadata
                    .as_ref()
                    .map(|metadata| metadata.uid.as_str()),
            )
        })
        .collect();
    let specification_deltas = match lifecycle.spec_revision_history(&task.feature_uid) {
        Ok(history) => specification_deltas_for_task(history, &task.task_uid),
        Err(error) => return (None, Some(error.to_string())),
    };
    let pr_preparation = task
        .pr_work_item_uid
        .as_deref()
        .map(|uid| lifecycle.load_pr_preparation(uid))
        .transpose();
    let pr_preparation = match pr_preparation {
        Ok(preparation) => preparation,
        Err(error) => return (None, Some(error.to_string())),
    };
    let current_spec_hash =
        crate::core::lifecycle::product_spec::current_product_hash(&project.state.planning_store)
            .ok();
    let (runtime_qa_evidence, runtime_qa_evidence_error) =
        runtime_qa_evidence::for_task(project, &task);
    let current_implementation = match lifecycle.load_current_implementation(&task.feature_uid) {
        Ok(current) => current,
        Err(error) => return (None, Some(error.to_string())),
    };
    let deployment = match lifecycle.load_deployment_evidence(&task.feature_uid) {
        Ok(deployment) => deployment,
        Err(error) => return (None, Some(error.to_string())),
    };
    let deployed_spec_hash = deployment
        .as_ref()
        .zip(current_implementation.as_ref())
        .and_then(|(deployment, current)| deployment.deployed_spec_hash(current))
        .map(str::to_owned);
    let deployment_status = deployment
        .as_ref()
        .zip(current_implementation.as_ref())
        .filter(|(deployment, current)| deployment.matches_current(current))
        .map(|(deployment, _)| {
            deployment.state.clone().unwrap_or_else(|| {
                if deployment.deployment_id.is_some() {
                    "deployment found; status unavailable".into()
                } else {
                    "no matching production deployment".into()
                }
            })
        });
    let (current_code_review, code_review_load_error, code_review_dispositions) =
        code_review::details(project, &task);
    (
        Some(crate::ui::task_detail::LifecycleView {
            feature_uid: task.feature_uid.clone(),
            feature_phase: feature.phase,
            task_phase: task.phase,
            parent_task_uid: task.parent_task_uid.clone(),
            parent_task,
            dependencies,
            subtasks,
            task_revision: task.revision,
            code_review: task.code_review,
            code_review_attempts: task.review_history,
            current_code_review,
            code_review_dispositions,
            code_review_load_error,
            runtime_qa: task.runtime_qa,
            runtime_qa_run_id: task
                .qa_execution
                .as_ref()
                .filter(|execution| {
                    execution.status == crate::core::lifecycle::QaExecutionStatusV1::Completed
                })
                .and_then(|execution| execution.request.as_ref())
                .map(|request| request.qa_run_id.clone()),
            spec_reconciliation: task.spec_reconciliation,
            current_spec_hash,
            implementation_head_sha: spec_revision
                .as_ref()
                .and_then(|revision| revision.implementation_head_sha.clone())
                .or(task.reviewed_code_head_sha),
            approved_target_hash: spec_revision
                .as_ref()
                .and_then(|revision| revision.approved_target_hash.clone())
                .or(feature.approved_plan_sha256),
            deployed_spec_hash,
            deployment_status,
            deployment_observed_at_ms: deployment
                .as_ref()
                .zip(current_implementation.as_ref())
                .filter(|(deployment, current)| deployment.matches_current(current))
                .map(|(deployment, _)| deployment.observed_at_ms),
            spec_revision: spec_revision.as_ref().map(|revision| revision.seq),
            spec_status: spec_revision.map(|revision| revision.reconciliation_status),
            pending_spec_triggers,
            specification_deltas,
            pull_request_status: pr_preparation.as_ref().map(|pr| pr.status),
            merged_commit_sha: pr_preparation.and_then(|pr| pr.merged_commit_sha),
            runtime_qa_evidence,
            runtime_qa_evidence_error,
            runtime_qa_applicability: task
                .qa_execution
                .as_ref()
                .and_then(|execution| execution.applicability.clone()),
        }),
        None,
    )
}

pub(super) fn specification_deltas_for_task(
    history: Vec<(
        crate::core::lifecycle::SpecRevisionV1,
        Vec<crate::core::lifecycle::SpecDeltaRecordV1>,
    )>,
    task_uid: &str,
) -> Vec<(u64, crate::core::lifecycle::SpecDeltaRecordV1)> {
    history
        .into_iter()
        .flat_map(|(revision, deltas)| {
            deltas
                .into_iter()
                .filter(|delta| {
                    delta.affected_task_ids.iter().any(|uid| uid == task_uid)
                        || delta.followup_ids.iter().any(|uid| uid == task_uid)
                })
                .map(move |delta| (revision.seq, delta))
        })
        .collect()
}
