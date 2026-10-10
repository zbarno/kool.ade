use super::super::{KooladeApp, Screen};

impl KooladeApp {
    pub(crate) fn start_task_generation(&mut self, work_key: &str, feature_id: &str) {
        let Screen::Connected(project) = &mut self.screen else {
            return;
        };
        let Some(index) = project.planning_work.iter().position(|work| {
            work.key == work_key
                && work.kind == crate::core::planning_work::WorkKind::TaskGeneration
                && work.feature_id.as_deref() == Some(feature_id)
        }) else {
            return;
        };
        let default_branch = if !project.git.default_branch.is_empty() {
            project.git.default_branch.as_str()
        } else if !project.git.branch.is_empty() {
            project.git.branch.as_str()
        } else {
            "main"
        };
        let targets = feature_branch_targets(
            &project.planning_work,
            feature_id,
            project.planning_work[index].feature_uid.as_deref(),
            default_branch,
        );
        let destination_branches = if !project.git.has_origin {
            &project.git.branches
        } else {
            &project.git.remote_branches
        };
        let source_available = project.git.branches.contains(&targets.source)
            || (!project.git.has_origin
                && project.git.branches.is_empty()
                && targets.source == default_branch);
        let destination_available = destination_branches.contains(&targets.destination)
            || (!project.git.has_origin
                && destination_branches.is_empty()
                && targets.destination.as_str() == default_branch);
        if !source_available || !destination_available {
            project.planning_work[index].status =
                crate::core::planning_work::WorkStatus::NeedsAttention;
            project.planning_work[index].detail = format!(
                "The saved source or destination branch is no longer available. Select an existing branch before generating tasks. Source: {}; destination: {}.",
                targets.source, targets.destination
            );
            if let Err(error) = project.save_planning_work() {
                self.toasts
                    .danger(format!("Could not save branch attention state: {error}"));
            }
            return;
        }
        let mut workflow = project.state.workflow.clone();
        workflow
            .feature_branch_targets
            .insert(feature_id.to_owned(), targets);
        let planning_store = project.state.planning_store.clone();
        let (changes, checks, _) = match crate::artifacts::task_docs::workflow_record_changes(
            &planning_store,
            &workflow,
        ) {
            Ok(changes) => changes,
            Err(error) => {
                self.toasts.danger(format!(
                    "Cannot save the feature branch selection before task generation: {error}"
                ));
                return;
            }
        };
        match planning_store.transaction_with_revision_and_record_revisions(
            &changes,
            Some(&project.state.baseline_planning_revision),
            &checks,
        ) {
            Ok((_, revision)) => {
                project.state.baseline_planning_revision = revision;
                project.state.workflow =
                    match crate::artifacts::task_docs::load_workflow(&planning_store) {
                        Ok(workflow) => workflow,
                        Err(error) => {
                            self.toasts.danger(format!(
                                "Branch selection was saved but workflow reload failed: {error}"
                            ));
                            return;
                        }
                    };
            }
            Err(error) => {
                self.toasts.danger(format!(
                    "Cannot save the feature branch selection before task generation: {error}"
                ));
                return;
            }
        }
        if project.active_turn.is_some() {
            self.toasts
                .warning("Wait for the current planning turn before generating tasks.");
            return;
        }
        let Some((_, specification)) = project
            .state
            .active_features
            .iter()
            .find(|(id, _)| id == feature_id)
            .cloned()
        else {
            project.planning_work[index].status =
                crate::core::planning_work::WorkStatus::NeedsAttention;
            project.planning_work[index].detail = "The approved specification is no longer available. Reopen planning and review the feature.".into();
            if let Err(error) = project.save_planning_work() {
                self.toasts
                    .danger(format!("Could not save task generation status: {error}"));
            }
            return;
        };
        if !crate::core::workflow::feature_approved(
            &project.state.planning_store,
            &project.state.workflow,
            feature_id,
        ) {
            self.toasts
                .warning("This specification needs approval before tasks can be generated.");
            return;
        }
        project.state.active_feature = Some((feature_id.to_owned(), specification.clone()));
        if super::super::has_current_task_batch(project) {
            project.planning_work[index].status = crate::core::planning_work::WorkStatus::Done;
            project.planning_work[index].detail =
                "Implementation tasks already exist for this approved specification.".into();
            if let Err(error) = project.save_planning_work() {
                self.toasts
                    .danger(format!("Could not save task generation status: {error}"));
            }
            return;
        }
        project.planning_work[index].status = crate::core::planning_work::WorkStatus::InProgress;
        project.planning_work[index].detail =
            "Checking that the approved specification is ready for task generation.".into();
        let repo = project.state.repo_root.clone();
        let contract = crate::core::workflow::feature_contract(&specification);
        if let Err(error) = project.save_planning_work() {
            project.planning_work[index].status =
                crate::core::planning_work::WorkStatus::NeedsAttention;
            project.planning_work[index].detail =
                format!("Could not save task generation progress: {error}");
            project.activity.pending_planning_work = true;
            self.toasts
                .danger(project.planning_work[index].detail.clone());
            return;
        }
        self.pending_feature_generation =
            Some((repo, feature_id.to_owned(), contract, work_key.to_owned()));
        let prompt = format!(
            "Prepare the task-generation review for approved feature {feature_id}. Its current specification is approved; do not ask for approval again or change its contract. Read the current feature and settled decisions, and return a complete interview brief naming {feature_id}, ready_for_tasks=true when no blocking questions remain. Do not generate stories in this review turn. The application will generate them after a successful current review. If blocked, record the concrete blocking item."
        );
        self.start_turn_for_work(
            &prompt,
            crate::core::workflow::TurnPurpose::ReviewForGeneration,
            None,
            Some(work_key.to_owned()),
        );
    }
}

fn feature_branch_targets(
    works: &[crate::core::planning_work::Work],
    feature_id: &str,
    feature_uid: Option<&str>,
    default_branch: &str,
) -> crate::core::workflow::BranchTargets {
    let selected = works.iter().find(|work| {
        work.kind != crate::core::planning_work::WorkKind::TaskGeneration
            && (work.feature_uid.as_deref() == feature_uid && feature_uid.is_some()
                || work.feature_id.as_deref() == Some(feature_id))
    });
    crate::core::workflow::BranchTargets {
        source: selected
            .and_then(|work| work.source_branch.clone())
            .unwrap_or_else(|| default_branch.to_owned()),
        destination: selected
            .and_then(|work| work.destination_branch.clone())
            .unwrap_or_else(|| default_branch.to_owned()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generation_preserves_branch_intent_when_new_feature_uid_is_not_linked_yet() {
        let mut work = crate::core::planning_work::Work::new(
            "feature:export".into(),
            "Export feature".into(),
            "Export project data".into(),
            String::new(),
        );
        work.feature_id = Some("F-38".into());
        work.source_branch = Some("release/2.1".into());
        work.destination_branch = Some("integration".into());
        let targets = feature_branch_targets(&[work], "F-38", None, "main");
        assert_eq!(targets.source, "release/2.1");
        assert_eq!(targets.destination, "integration");
    }
}
