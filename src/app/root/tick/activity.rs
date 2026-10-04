use super::*;

impl KooladeApp {
    pub(super) fn advance_activity(&mut self) {
        if let Screen::Connected(project) = &mut self.screen {
            reconcile_inactive_planning_work(project);
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
                    Ok(text) => project.remember_chat(vec![ChatMessage::new(ChatRole::Agent, text, None)]),
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
                let harness = configured_harness(&mut self.task_harness);
                project.activity.manager = Some(crate::app::manager::Manager::start(
                    project, &events, harness,
                ));
                project.activity.last_update = Some(Instant::now());
                project.live_progress = crate::harness::LiveProgress {
                    activity: Some("Kool.ad/e Man is checking the board…".into()),
                    ..Default::default()
                };
            }
        }
    }
}

fn reconcile_inactive_planning_work(project: &mut crate::app::session::Project) {
    let active_key = project
        .active_turn
        .as_ref()
        .and(project.active_planning_work.as_deref());
    let retry = std::mem::take(&mut project.activity.pending_planning_work);
    let mut updated = project.planning_work.clone();
    if retry
        && let Some(active_key) = active_key
        && let Some(active) = updated.iter_mut().find(|work| work.key == active_key)
    {
        active.status = crate::core::planning_work::WorkStatus::InProgress;
        active.detail = "Planning in progress".into();
    }
    let mut changes =
        crate::core::planning_work::reconcile_inactive(&project.state, &mut updated, active_key);
    if retry {
        changes.push("previously reconciled planning work".to_owned());
    }
    if changes.is_empty() {
        return;
    }

    match crate::core::planning_work::save(&project.state.repo_root, &updated) {
        Ok(()) => {
            project.planning_work = updated;
            if retry {
                project
                    .activity
                    .pending
                    .retain(|event| !event.starts_with("Planning work could not be saved:"));
                project
                    .activity
                    .pending
                    .push("Previously reconciled planning status was saved.".into());
            } else {
                project.activity.pending.push(format!(
                    "Kool.ad/e reconciled planning work with no active turn: {}.",
                    changes.join("; ")
                ));
            }
        }
        Err(error) => {
            project.planning_work = updated.clone();
            project.activity.pending_planning_work = true;
            if !project
                .activity
                .pending
                .iter()
                .any(|event| event.starts_with("Planning work could not be saved:"))
            {
                project.activity.pending.push(format!(
                    "Planning work could not be saved: {error}. Kool.ad/e will retry automatically."
                ));
            }
        }
    }
}
