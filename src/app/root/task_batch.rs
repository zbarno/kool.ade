use super::Project;

pub(super) fn has_current_task_batch(project: &Project) -> bool {
    if let Some((feature_id, feature)) = &project.state.active_feature {
        return project.state.workflow.task_batches.iter().any(|batch| {
            crate::core::contract_snapshot::batch_contract_matches_feature(
                &project.state.repo_root,
                &batch.directory,
                feature_id,
                feature,
            )
        });
    }
    project
        .task_documents
        .iter()
        .any(|doc| !doc.path.ends_with("/README.md"))
        && project.state.workflow.brief.as_ref().is_none_or(|brief| {
            project
                .state
                .workflow
                .task_batches
                .last()
                .is_some_and(|batch| batch.feature == brief.feature_name)
        })
}
