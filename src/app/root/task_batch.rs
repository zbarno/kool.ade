use super::Project;

pub(super) fn has_current_task_batch(project: &Project) -> bool {
    if let Some((feature_id, feature)) = &project.state.active_feature {
        return project.state.workflow.task_batches.iter().any(|batch| {
            batch_targets_feature(
                &batch.feature,
                feature_id,
                feature,
                project
                    .state
                    .workflow
                    .brief
                    .as_ref()
                    .map(|brief| brief.feature_name.as_str()),
            ) && crate::core::contract_snapshot::batch_contract_matches_feature(
                &project.state.planning_store,
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

fn batch_targets_feature(
    batch_feature: &str,
    feature_id: &str,
    feature: &str,
    brief_name: Option<&str>,
) -> bool {
    crate::core::workflow::feature_ids_in(batch_feature)
        .iter()
        .any(|id| id == feature_id)
        || brief_name.is_some_and(|name| name == batch_feature)
        || feature
            .lines()
            .next()
            .and_then(|heading| heading.split_once(':'))
            .is_some_and(|(_, title)| title.trim().eq_ignore_ascii_case(batch_feature))
}

#[cfg(test)]
mod tests {
    use super::batch_targets_feature;

    #[test]
    fn unrelated_legacy_batch_does_not_shadow_current_feature() {
        assert!(!batch_targets_feature(
            "Readable Chat Replies (CHG-003)",
            "F7",
            "# F7: Comparative Feature-Plan Comparison Before Approval",
            Some("Comparative Feature-Plan Comparison Before Approval (F7)"),
        ));
        assert!(batch_targets_feature(
            "Comparative Feature-Plan Comparison Before Approval (F7)",
            "F7",
            "# F7: Comparative Feature-Plan Comparison Before Approval",
            Some("Comparative Feature-Plan Comparison Before Approval (F7)"),
        ));
        assert!(batch_targets_feature(
            "Fixture",
            "CHG-001",
            "# CHG-001: Fixture",
            None
        ));
    }
}
