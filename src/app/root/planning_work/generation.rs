use super::super::{KooladeApp, Screen};

mod repository_intents;
use repository_intents::feature_git_intents;

pub(super) fn has_approved_git_intents(
    project: &crate::app::session::Project,
    feature_id: &str,
    feature_uid: Option<&str>,
) -> bool {
    if !project.has_selected_code_repository() {
        return true;
    }
    feature_git_intents(&project.planning_work, feature_id, feature_uid)
        .ok()
        .flatten()
        .or_else(|| {
            project
                .state
                .workflow
                .feature_git_intents
                .get(feature_id)
                .filter(|intents| !intents.is_empty())
                .cloned()
        })
        .is_some()
}

impl KooladeApp {
    pub(crate) fn start_task_generation(&mut self, work_key: &str, feature_id: &str) {
        let (contract, repo_root, ready, work_key) = {
            let Screen::Connected(project) = &mut self.screen else {
                return;
            };
            let current_feature_uid = project
                .state
                .active_features
                .iter()
                .find(|(id, _)| id == feature_id)
                .and_then(|(_, markdown)| {
                    crate::domain::ArtifactIdentity::from_markdown(markdown)
                        .ok()
                        .flatten()
                })
                .map(|identity| identity.uid);
            let Some(index) = project.planning_work.iter().position(|work| {
                work.key == work_key
                    && work.kind == crate::core::planning_work::WorkKind::TaskGeneration
                    && crate::core::planning_work::belongs_to_feature(
                        work,
                        feature_id,
                        current_feature_uid.as_deref(),
                    )
            }) else {
                return;
            };
            if crate::app::manager::has_pending_plan_repair(project, feature_id) {
                project.planning_work[index].status =
                    crate::core::planning_work::WorkStatus::NeedsAttention;
                project.planning_work[index].detail = "Blocked: Manager-directed plan repair has not been verified by a fresh independent review.".into();
                if let Err(error) = project.save_planning_work() {
                    self.toasts.danger(format!(
                        "Manager plan repair remains unresolved, but the blocked task status could not be saved: {error}"
                    ));
                } else {
                    self.toasts
                        .warning(project.planning_work[index].detail.clone());
                }
                return;
            }
            if crate::app::manager::has_pending_plan_contest(project, feature_id) {
                project.planning_work[index].status =
                    crate::core::planning_work::WorkStatus::NeedsAttention;
                project.planning_work[index].detail = "Blocked: a Manager-contested plan finding has not completed independent arbitration.".into();
                if let Err(error) = project.save_planning_work() {
                    self.toasts.danger(format!(
                        "Manager plan arbitration remains unresolved, but the blocked task status could not be saved: {error}"
                    ));
                } else {
                    self.toasts
                        .warning(project.planning_work[index].detail.clone());
                }
                return;
            }
            let readiness_current = project
                .state
                .active_features
                .iter()
                .find(|(id, _)| id == feature_id)
                .and_then(|(_, markdown)| {
                    let identity = crate::domain::ArtifactIdentity::from_markdown(markdown)
                        .ok()
                        .flatten()?;
                    let contract = crate::core::workflow::feature_contract(markdown);
                    crate::core::lifecycle::plan_snapshot::from_feature_with_author(
                        &project.state.planning_store,
                        feature_id,
                        &identity,
                        markdown,
                        &contract,
                    )
                    .ok()
                })
                .is_some_and(|snapshot| {
                    crate::app::root::feature_approval::readiness::current(
                        &project.state.planning_store,
                        &snapshot,
                    )
                    .is_ok()
                });
            if !readiness_current {
                project.planning_work[index].status =
                    crate::core::planning_work::WorkStatus::NeedsAttention;
                project.planning_work[index].detail = "Blocked: current readiness evidence is missing or stale. Reassess readiness before generating tasks.".into();
                if let Err(error) = project.save_planning_work() {
                    self.toasts.danger(format!(
                        "Current plan readiness is required, but the blocked task status could not be saved: {error}"
                    ));
                } else {
                    self.toasts
                        .warning(project.planning_work[index].detail.clone());
                }
                return;
            }
            if !crate::core::workflow::feature_approved(
                &project.state.planning_store,
                &project.state.workflow,
                feature_id,
            ) {
                project.planning_work[index].status =
                    crate::core::planning_work::WorkStatus::NeedsAttention;
                project.planning_work[index].detail = "Blocked: an independent PlanReviewer must approve the exact current plan snapshot before task generation.".into();
                if let Err(error) = project.save_planning_work() {
                    self.toasts.danger(format!(
                    "Independent plan review is required, but the blocked task status could not be saved: {error}"
                ));
                } else {
                    self.toasts
                        .warning(project.planning_work[index].detail.clone());
                }
                return;
            }
            let published_feature = project
                .state
                .active_features
                .iter()
                .find(|(id, _)| id == feature_id)
                .and_then(|(_, markdown)| {
                    let identity = crate::domain::ArtifactIdentity::from_markdown(markdown)
                        .ok()
                        .flatten()?;
                    Some(
                        crate::app::root::feature_approval::publication::pin_is_published(
                            project,
                            &identity.uid,
                            markdown,
                        ),
                    )
                })
                .unwrap_or(false);
            if !published_feature {
                project.planning_work[index].status =
                    crate::core::planning_work::WorkStatus::NeedsAttention;
                project.planning_work[index].detail = "Blocked: merge the consented feature documentation PR and refresh the published-spec pin before generating tasks.".into();
                if let Err(error) = project.save_planning_work() {
                    self.toasts.danger(format!(
                        "Published feature documentation is required, but the blocked task status could not be saved: {error}"
                    ));
                } else {
                    self.toasts
                        .warning(project.planning_work[index].detail.clone());
                }
                return;
            }
            if project.has_selected_code_repository() {
                let intent_result = feature_git_intents(
                    &project.planning_work,
                    feature_id,
                    project.planning_work[index].feature_uid.as_deref(),
                );
                let intents = match intent_result {
                    Ok(intents) => intents.or_else(|| {
                        project
                            .state
                            .workflow
                            .feature_git_intents
                            .get(feature_id)
                            .filter(|intents| !intents.is_empty())
                            .cloned()
                    }),
                    Err(error) => {
                        project.planning_work[index].status =
                            crate::core::planning_work::WorkStatus::NeedsAttention;
                        project.planning_work[index].detail = format!(
                            "Conflicting task-level repository Git approvals prevent generation: {error}"
                        );
                        if let Err(save_error) = project.save_planning_work() {
                            self.toasts.danger(format!(
                                "Git intent conflict remains unresolved, but the blocked status could not be saved: {save_error}"
                            ));
                        } else {
                            self.toasts
                                .warning(project.planning_work[index].detail.clone());
                        }
                        return;
                    }
                };
                let Some(intents) = intents else {
                    project.planning_work[index].status =
                        crate::core::planning_work::WorkStatus::NeedsAttention;
                    project.planning_work[index].detail = "Select each implementation repository and approve its source commit, destination, and publication mode before generating tasks.".into();
                    if let Err(error) = project.save_planning_work() {
                        self.toasts.danger(format!(
                            "Explicit branch intent is required, but the blocked task status could not be saved: {error}"
                        ));
                    } else {
                        self.toasts
                            .warning(project.planning_work[index].detail.clone());
                    }
                    return;
                };
                let validation = intents.iter().try_for_each(|(repository_id, intent)| {
                    super::git_intent::validate_approved_intent(
                        project,
                        project.planning_work[index].feature_uid.as_deref(),
                        repository_id,
                        intent,
                    )
                });
                if let Err(error) = validation {
                    project.planning_work[index].status =
                        crate::core::planning_work::WorkStatus::NeedsAttention;
                    project.planning_work[index].detail = format!(
                        "Approved repository Git intent is no longer valid: {error}. Review and approve the affected repository again before generating tasks."
                    );
                    if let Err(error) = project.save_planning_work() {
                        self.toasts
                            .danger(format!("Could not save branch attention state: {error}"));
                    }
                    return;
                }
                let mut workflow = project.state.workflow.clone();
                workflow
                    .feature_git_intents
                    .insert(feature_id.to_owned(), intents);
                let planning_store = project.state.planning_store.clone();
                let (changes, checks) = match crate::artifacts::task_docs::workflow_record_changes(
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
            }
            if project.active_turn.is_some() {
                self.toasts
                    .warning("Wait for the current planning turn before generating tasks.");
                return;
            }
            if !project
                .state
                .active_features
                .iter()
                .any(|(id, _)| id == feature_id)
            {
                project.planning_work[index].status =
                    crate::core::planning_work::WorkStatus::NeedsAttention;
                project.planning_work[index].detail = "The approved specification is no longer available. Reopen planning and review the feature.".into();
                if let Err(error) = project.save_planning_work() {
                    self.toasts
                        .danger(format!("Could not save task generation status: {error}"));
                }
                return;
            };
            let (_, feature_markdown) = project
                .state
                .active_features
                .iter()
                .find(|(id, _)| id == feature_id)
                .expect("active feature checked above");
            let feature_markdown = feature_markdown.clone();
            project.state.active_feature = Some((feature_id.to_owned(), feature_markdown.clone()));
            let contract = crate::core::workflow::feature_contract(&feature_markdown);
            let repo_root = project.state.repo_root.clone();
            let ready = project.state.workflow.ready(Some(&feature_markdown));
            let work_key = project.planning_work[index].key.clone();
            (contract, repo_root, ready, work_key)
        };
        if ready {
            self.start_turn_for_work(
                &format!("Generate task stories for approved feature {feature_id}."),
                crate::core::workflow::TurnPurpose::GenerateTasks,
                None,
                Some(work_key),
            );
        } else {
            self.pending_feature_generation =
                Some((repo_root, feature_id.to_owned(), contract, work_key.clone()));
            self.start_turn_for_work(
                &format!(
                    "Prepare task generation for approved feature {feature_id}. Read the current feature and settled decisions, and return a complete interview brief naming {feature_id}, ready_for_tasks=true only when no blocking questions remain. Do not generate stories in this turn. If blocked, record the concrete blocking item."
                ),
                crate::core::workflow::TurnPurpose::ReviewForGeneration,
                None,
                Some(work_key),
            );
            if !matches!(&self.screen, Screen::Connected(project) if project.active_turn.is_some())
            {
                self.pending_feature_generation = None;
            }
        }
    }
}
