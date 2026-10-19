use super::*;

impl KooladeApp {
    pub(super) fn advance_activity(&mut self) {
        let mut planner_generation_readiness = None;
        if let Screen::Connected(project) = &mut self.screen {
            reconcile_inactive_planning_work(project);
            planner_generation_readiness = advance_clarification_planner(project);
            advance_plan_review(project);
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
                        let text = crate::app::manager::persist_output(project, &output);
                        project.remember_chat(vec![ChatMessage::new(ChatRole::Agent, text, None)]);
                    },
                    Err(_) => project.remember_chat(vec![ChatMessage::new(ChatRole::System, "Project-manager update unavailable after retry; task work continues. You can still send a message.", None)]),
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
                let harness = configured_harness_for(
                    &mut self.task_harness,
                    Some(crate::persistence::harness_settings::MANAGER),
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
        }
        if let Some(ready) = planner_generation_readiness {
            self.continue_feature_generation(ready);
        }
    }
}

mod clarification;
mod dependency_review;
mod opportunities;
mod plan_review;
mod reconcile;
mod runtime_qa;
mod self_improvement;
pub(crate) mod spec_reconciliation;

use clarification::advance_clarification_planner;
use dependency_review::advance_dependency_review;
use plan_review::advance_plan_review;
use reconcile::reconcile_inactive_planning_work;
