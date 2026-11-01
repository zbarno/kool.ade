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
        let mut plan_repair = None;
        let mut code_review_repair = None;
        let mut planner_generation_readiness = None;
        if let Screen::Connected(project) = &mut self.screen {
            reconcile_inactive_planning_work(project);
            planner_generation_readiness = advance_clarification_planner(project);
            advance_plan_review(project);
            advance_code_review_arbitration(project);
            advance_dependency_review(project, &mut self.task_harness);
            runtime_qa::advance_runtime_qa(project, &mut self.task_harness);
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
            let mut result = None;
            if let Some(manager) = &project.activity.manager {
                for _ in 0..64 {
                    let Some(progress) = manager.progress() else {
                        break;
                    };
                    project.live_progress.update(progress);
                    project
                        .activity
                        .overall
                        .as_mut()
                        .unwrap()
                        .update(Default::default());
                    project
                        .activity
                        .conversations
                        .entry("__main".into())
                        .or_default()
                        .update(Default::default());
                }
                result = manager.result();
            }
            if let Some(result) = result {
                project.activity.manager = None;
                project.live_progress = Default::default();
                match result {
                    Ok(output) => {
                        let dispositions = crate::app::manager::persist_dispositions(
                            project,
                            &output.proposed_actions,
                            &output.evidence_hashes,
                            &output.planning_revision,
                            &output.code_review_targets,
                        );
                        let mut text = crate::app::manager::persist_output(project, &output);
                        match dispositions {
                            Ok(messages) => {
                                if !messages.is_empty() {
                                    text.push_str("\n\n");
                                    text.push_str(&messages.join("\n"));
                                }
                            }
                            Err(error) => text.push_str(&format!(
                                "\n\nManager decisions could not be saved to the Board: {error}"
                            )),
                        }
                        project.remember_chat(vec![ChatMessage::new(ChatRole::Agent, text, None)]);
                    }
                    Err(error) => {
                        let detail = crate::core::context_build::clip(
                            &crate::error::redact_secrets(&error),
                            1200,
                        );
                        project.remember_chat(vec![ChatMessage::new(
                            ChatRole::System,
                            format!(
                                "Project-manager update unavailable: {detail} Task work continues; you can still send a message."
                            ),
                            None,
                        )]);
                    }
                }
            }
            // Stalled-worker patrol: surfaced and acted upon at most once per
            // cooldown period, so a queue stuck on hung workers cannot drive
            // an endless stream of background manager LLM turns.
            if crate::app::manager::patrol_note_due(
                project.active_implementations.len(),
                project.activity.pending.len(),
                project.activity.last_update,
                project.activity.last_patrol_note,
                Instant::now(),
            ) {
                project.activity.pending.push("The worker is still running. No completion is confirmed; review the current task states and help the user with the next eligible planning decision without inventing progress.".into());
                project.activity.last_patrol_note = Some(Instant::now());
            }
            if crate::app::manager::patrol_manager_due(
                project.active_turn.is_some(),
                project.activity.manager.is_some(),
                project.activity.pending.len(),
                project.activity.last_update,
                Instant::now(),
            ) {
                let events = std::mem::take(&mut project.activity.pending);
                let harness = crate::app::root::configured_harness_for_project(
                    &mut self.task_harness,
                    Some(crate::persistence::harness_settings::MANAGER),
                    &project.state.harness_routes.routes,
                );
                let route_label = harness.label();
                project.activity.manager = Some(crate::app::manager::Manager::start(
                    project, &events, harness,
                ));
                project.activity.last_update = Some(Instant::now());
                project.live_progress = crate::harness::LiveProgress {
                    activity: Some(format!(
                        "Kool.ad/e Manager is checking the board with {route_label}…"
                    )),
                    selected_route: Some(route_label),
                    ..Default::default()
                };
            }
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

mod clarification;
mod code_review_arbitration;
mod dependency_review;
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
