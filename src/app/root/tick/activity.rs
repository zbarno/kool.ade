use super::*;

#[cfg(test)]
pub(crate) fn synchronize_self_improvement_run_for_test(
    project: &mut crate::app::session::Project,
    lifecycle: &crate::core::lifecycle::LifecycleStore<'_>,
    runs: &crate::core::self_improvement::SelfImprovementStore<'_>,
    run_uid: &str,
) -> Result<(), String> {
    self_improvement::synchronize_run(project, lifecycle, runs, run_uid)
}

impl KooladeApp {
    pub(super) fn advance_activity(&mut self) {
        self.retry_pending_task_reply();
        let runtime_qa_computer_use = self.runtime_qa_computer_use_context();
        let mut plan_repair = None;
        let mut code_review_repair = None;
        let mut planner_generation_readiness = None;
        let mut plan_confirmation_evaluation = None;
        if let Screen::Connected(project) = &mut self.screen {
            crate::app::manager::operational_recovery::advance(project);
            reconcile_inactive_planning_work(project);
            let effects = crate::app::root::plan_confirmation_sync::sync(project);
            plan_confirmation_evaluation = effects.evaluations_wanted.into_iter().next();
            for event in effects.announcements {
                self.toasts.info(event);
            }
            planner_generation_readiness = advance_clarification_planner(project);
            advance_plan_review(project);
            advance_code_review_arbitration(project);
            advance_dependency_review(project, &mut self.task_harness);
            runtime_qa::advance_runtime_qa(
                project,
                &mut self.task_harness,
                runtime_qa_computer_use,
            );
            spec_reconciliation::advance_spec_reconciliation(project, &mut self.task_harness);
            self_improvement::synchronize(project);
            opportunities::schedule_due(project);
            // Persist only tickets whose activity actually moved since the
            // last flush, on a 2 s cadence (was: every active ticket, every
            // 2 s, whether changed or not).
            if !project.activity.dirty_tickets.is_empty()
                && project
                    .activity
                    .last_save
                    .is_none_or(|last| last.elapsed() >= Duration::from_secs(2))
            {
                for ticket in project.activity.take_dirty_tickets() {
                    project.save_task_activity(&ticket);
                }
                project.activity.last_save = Some(Instant::now());
            }
            manager::advance(project, &mut self.task_harness);
            plan_repair = crate::app::manager::next_plan_repair(project);
            if plan_repair.is_none() {
                code_review_repair = crate::app::manager::next_code_review_repair(project);
            }
            if code_review_repair.is_none() {
                code_review_repair = crate::app::manager::next_qa_finding_repair(project);
            }
        }
        if let Some((key, request, feature_id)) = plan_repair {
            self.start_turn_for_work(
                &request,
                crate::core::workflow::TurnPurpose::Interview,
                Some(&feature_id),
                Some(key),
            );
        }
        if let Some(evaluation) = plan_confirmation_evaluation {
            // One evaluation at a time; the reconciler queues the rest
            // (prompt-once per spec revision).
            self.start_turn_for_work(
                &format!(
                    "Evaluate planning readiness for \"{}\": recompute the persisted readiness assessment strictly from the current plan document and record every unresolved blocker verbatim.",
                    evaluation.feature_title
                ),
                crate::core::workflow::TurnPurpose::ReadinessEvaluation,
                None,
                Some(evaluation.work_key),
            );
        }
        if let Some((ticket, work_id)) = code_review_repair {
            self.start_implementation(ticket.clone(), false);
            if let Screen::Connected(project) = &mut self.screen
                && !project.active_implementations.contains_key(&ticket)
            {
                let start_error = project.queue.last_error.clone();
                if let Some(work) = project
                    .planning_work
                    .iter_mut()
                    .find(|work| work.uid == work_id)
                {
                    work.status = crate::core::planning_work::WorkStatus::NeedsAttention;
                    work.detail.push_str(&format!(
                        "\n\nBlocked: the application could not start the scoped Manager repair worker. {}",
                        if start_error.trim().is_empty() {
                            "Check implementation capability and queue state before retrying."
                                .into()
                        } else {
                            start_error
                        }
                    ));
                    if let Err(error) = project.save_planning_work() {
                        project.activity.pending.push(format!(
                            "The scoped Manager repair could not start, and its attention state could not be saved: {error}"
                        ));
                    }
                }
            }
        }
        if let Screen::Connected(project) = &mut self.screen
            && let Some((feature_id, arbitration)) =
                crate::app::manager::next_plan_arbitration(project)
        {
            let harness = crate::app::root::configured_harness_for_project(
                &mut self.task_harness,
                Some(crate::persistence::harness_settings::PLAN_REVIEW),
                &project.state.harness_routes.routes,
            );
            match crate::app::root::plan_review::PlanReview::start_arbitration(
                project,
                feature_id.clone(),
                arbitration,
                harness,
            ) {
                Ok(worker) => {
                    project.activity.plan_review = Some(worker);
                    project.activity.plan_review_feature = Some(feature_id.clone());
                    project.activity.plan_review_revision =
                        Some(project.state.baseline_planning_revision.clone());
                    project.live_progress = crate::harness::LiveProgress {
                        activity: Some(format!(
                            "Independently arbitrating a contested plan finding for {feature_id}"
                        )),
                        ..Default::default()
                    };
                }
                Err(error) => {
                    plan_review::lifecycle::mark_arbitration_failed(
                        project,
                        &feature_id,
                        &error.to_string(),
                    );
                    project.activity.pending.push(format!(
                        "Independent arbitration could not start: {error}. The plan remains blocked."
                    ));
                }
            }
        }
        if let Screen::Connected(project) = &mut self.screen
            && let Some(request) = crate::app::manager::next_code_review_arbitration(project)
        {
            let work_uid = request.work_uid.clone();
            let harness = crate::app::root::configured_harness_for_project(
                &mut self.task_harness,
                Some(crate::persistence::harness_settings::PLAN_REVIEW),
                &project.state.harness_routes.routes,
            );
            match crate::app::root::code_review_arbitration::CodeReviewArbitration::start(
                project,
                request.clone(),
                harness,
            ) {
                Ok(worker) => {
                    project.activity.code_review_arbitration = Some(worker);
                    project.live_progress = crate::harness::LiveProgress {
                        activity: Some(format!(
                            "Independently arbitrating code-review finding for {}",
                            request.context.task_uid
                        )),
                        ..Default::default()
                    };
                }
                Err(error) => {
                    crate::app::manager::finish_code_review_arbitration(
                        project,
                        &request,
                        Err(error.clone()),
                    );
                    project.activity.pending.push(format!(
                        "Independent code-review arbitration for {work_uid} could not start: {error}"
                    ));
                }
            }
        }
        if let Some(ready) = planner_generation_readiness {
            self.continue_feature_generation(ready);
        }
    }
}

impl KooladeApp {
    fn runtime_qa_computer_use_context(&self) -> Option<runtime_qa::ComputerQaRuntimeContextV1> {
        #[cfg(target_os = "linux")]
        {
            let project_id = match &self.screen {
                Screen::Connected(project) => project.state.planning_store.project_id,
                _ => return None,
            };
            let context = self.computer_use_run_context(project_id)?;
            Some(runtime_qa::ComputerQaRuntimeContextV1 {
                host: context.host,
                controller: self.shared_computer_use_controller(),
                project_id: context.project_id,
                user_id: context.user_id,
            })
        }
        #[cfg(not(target_os = "linux"))]
        {
            None
        }
    }
}

mod clarification;
mod code_review_arbitration;
mod dependency_review;
mod manager;
mod opportunities;
pub(crate) mod plan_review;
mod reconcile;
pub(crate) mod runtime_qa;
mod self_improvement;
pub(crate) mod spec_reconciliation;

use clarification::advance_clarification_planner;
use code_review_arbitration::advance_code_review_arbitration;
use dependency_review::advance_dependency_review;
use plan_review::advance_plan_review;
pub(crate) use reconcile::reconcile_inactive_planning_work;
