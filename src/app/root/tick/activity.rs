use super::*;

impl KooladeApp {
    pub(super) fn advance_activity(&mut self) {
        if let Screen::Connected(project) = &mut self.screen {
            reconcile_inactive_planning_work(project);
            advance_dependency_review(project, &mut self.task_harness);
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
    }
}

fn advance_dependency_review(
    project: &mut crate::app::session::Project,
    task_harness: &mut Option<Box<dyn crate::harness::AiHarness>>,
) {
    let result = project
        .activity
        .dependency_manager
        .as_ref()
        .and_then(crate::app::manager::DependencyReview::result);
    if let Some(result) = result {
        let Some(review) = project.activity.dependency_manager.take() else {
            return;
        };
        let ticket = review.ticket().to_owned();
        let request_id = review.request_id().to_owned();
        let request = project
            .activity
            .tasks
            .get(&ticket)
            .and_then(|progress| {
                progress
                    .dependency_requests
                    .iter()
                    .find(|request| request.id == request_id)
            })
            .cloned();
        let Some(request) = request else {
            crate::harness::dependency_authorization::unregister(&request_id);
            return;
        };
        let proposed = result.unwrap_or_else(|_| crate::app::manager::DependencyTriage {
            decision: crate::harness::DependencyDecision::RequiresUserAuthorization,
            rationale: "Man.ager could not complete its dependency review. The request remains limited to this task and needs your decision.".into(),
            risk: "No authorization was granted because the structured review did not complete.".into(),
        });
        let triage = crate::app::manager::enforce_policy(&request.need, proposed);
        let scope = match triage.decision {
            crate::harness::DependencyDecision::AuthorizeForTask => {
                Some(crate::harness::DependencyAuthorizationScope::Once)
            }
            crate::harness::DependencyDecision::AuthorizeForProject => {
                Some(crate::harness::DependencyAuthorizationScope::Project)
            }
            _ => None,
        };
        let status = match triage.decision {
            crate::harness::DependencyDecision::RequiresUserAuthorization => {
                crate::harness::DependencyRequestStatus::AwaitingUser
            }
            crate::harness::DependencyDecision::Reject => {
                crate::harness::DependencyRequestStatus::Denied
            }
            _ => crate::harness::DependencyRequestStatus::Authorized,
        };
        if let Some(progress) = project.activity.tasks.get_mut(&ticket)
            && let Some(request) = progress
                .dependency_requests
                .iter_mut()
                .find(|request| request.id == request_id)
        {
            request.decision = triage.decision;
            if triage.decision == crate::harness::DependencyDecision::Reject {
                request.category =
                    crate::harness::DependencyFailureCategory::DependencyPolicyDenied;
            }
            request.rationale = triage.rationale.clone();
            request.risk = triage.risk.clone();
            request.status = status;
        }
        let _ = crate::harness::dependency_authorization::answer(
            &request_id,
            &request.task_id,
            &request.need,
            crate::harness::dependency_authorization::DependencyResolution {
                decision: triage.decision,
                scope,
                rationale: triage.rationale,
            },
        );
        project.save_task_activity(&ticket);
        project.activity.mark_ticket_dirty(&ticket);
    }

    if project.activity.dependency_manager.is_some() {
        return;
    }
    while let Some((ticket, request)) = project.activity.pending_dependency_reviews.pop() {
        let request_for_review = {
            let Some(progress) = project.activity.tasks.get_mut(&ticket) else {
                continue;
            };
            let Some(stored) = progress
                .dependency_requests
                .iter_mut()
                .find(|stored| stored.id == request.id)
            else {
                continue;
            };
            if !matches!(
                stored.status,
                crate::harness::DependencyRequestStatus::Pending
                    | crate::harness::DependencyRequestStatus::ManagerReviewing
            ) {
                continue;
            }
            stored.status = crate::harness::DependencyRequestStatus::ManagerReviewing;
            stored.rationale =
                "Man.ager is checking whether this dependency is required and safe for the task."
                    .into();
            stored.clone()
        };
        project.save_task_activity(&ticket);
        let harness = crate::app::root::configured_harness_for(
            task_harness,
            Some(crate::persistence::harness_settings::MANAGER),
        );
        project.activity.dependency_manager = Some(crate::app::manager::DependencyReview::start(
            project,
            &ticket,
            &request_for_review,
            harness,
        ));
        break;
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
    let cancelled = updated
        .iter()
        .filter(|work| {
            project
                .cancelled_work
                .contains(&crate::persistence::cancelled_work::planning_id(&work.uid))
        })
        .map(|work| work.uid.clone())
        .collect();
    let mut changes = crate::core::planning_work::reconcile_inactive_excluding(
        &project.state,
        &mut updated,
        active_key,
        &cancelled,
    );
    if retry {
        changes.push("previously reconciled planning work".to_owned());
    }
    if changes.is_empty() {
        return;
    }

    project.planning_work = updated;
    match project.save_planning_work() {
        Ok(()) => {
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
