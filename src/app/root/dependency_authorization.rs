use super::{KooladeApp, Screen};

impl KooladeApp {
    pub(super) fn authorize_dependency_request(
        &mut self,
        ticket: &str,
        request_id: &str,
        scope: crate::harness::DependencyAuthorizationScope,
    ) {
        let mut resume = false;
        if let Screen::Connected(project) = &mut self.screen {
            let request = project
                .activity
                .tasks
                .get(ticket)
                .and_then(|progress| {
                    progress
                        .dependency_requests
                        .iter()
                        .find(|request| request.id == request_id)
                })
                .cloned();
            let Some(request) = request else {
                project.queue.last_error = "This dependency request is no longer available.".into();
                return;
            };
            if request.status != crate::harness::DependencyRequestStatus::AwaitingUser {
                project.queue.last_error =
                    "Man.ager has not requested a user authorization decision for this package."
                        .into();
                return;
            }
            let decision = match scope {
                crate::harness::DependencyAuthorizationScope::Once => {
                    crate::harness::DependencyDecision::UserAuthorizeForTask
                }
                crate::harness::DependencyAuthorizationScope::Project => {
                    crate::harness::DependencyDecision::UserAuthorizeForProject
                }
            };
            if !crate::harness::dependency_decision_allowed(&request.need, decision) {
                let rationale = "Kool.ad/e's current dependency broker cannot safely prepare this source or ecosystem, so authorization cannot enable it. No permission was granted.";
                if let Some(progress) = project.activity.tasks.get_mut(ticket)
                    && let Some(stored) = progress
                        .dependency_requests
                        .iter_mut()
                        .find(|stored| stored.id == request_id)
                {
                    stored.status = crate::harness::DependencyRequestStatus::Failed;
                    stored.rationale = rationale.into();
                }
                project.save_task_activity(ticket);
                project.activity.mark_ticket_dirty(ticket);
                let _ = crate::harness::dependency_authorization::answer(
                    request_id,
                    crate::harness::dependency_authorization::DependencyResolution {
                        decision: crate::harness::DependencyDecision::Reject,
                        scope: None,
                        rationale: rationale.into(),
                    },
                );
                project.queue.last_error = rationale.into();
                return;
            }
            match scope {
                crate::harness::DependencyAuthorizationScope::Project => {
                    if let Err(error) = crate::persistence::dependency_authorization::save(
                        &project.state.repo_root,
                        &request,
                        scope,
                    ) {
                        project.queue.last_error =
                            format!("Could not store this authorization privately: {error:#}");
                        self.toasts.danger(project.queue.last_error.clone());
                        return;
                    }
                }
                crate::harness::DependencyAuthorizationScope::Once => {
                    // If the broker is still waiting, the scope travels with
                    // its answer and is consumed by this exact request. A
                    // transient grant is only needed when the worker must be
                    // resumed after a restart.
                }
            }
            if let Some(progress) = project.activity.tasks.get_mut(ticket)
                && let Some(stored) = progress
                    .dependency_requests
                    .iter_mut()
                    .find(|stored| stored.id == request_id)
            {
                stored.decision = decision;
                stored.status = crate::harness::DependencyRequestStatus::Authorized;
                stored.rationale = match scope {
                    crate::harness::DependencyAuthorizationScope::Once => {
                        "You authorized this exact dependency request for the current task.".into()
                    }
                    crate::harness::DependencyAuthorizationScope::Project => {
                        "You authorized this exact dependency request for this project.".into()
                    }
                };
            }
            project.save_task_activity(ticket);
            project.activity.mark_ticket_dirty(ticket);
            let delivered = crate::harness::dependency_authorization::answer(
                request_id,
                crate::harness::dependency_authorization::DependencyResolution {
                    decision,
                    scope: Some(scope),
                    rationale: match scope {
                        crate::harness::DependencyAuthorizationScope::Once => {
                            "The user authorized this exact dependency request for the current task.".into()
                        }
                        crate::harness::DependencyAuthorizationScope::Project => {
                            "The user authorized this exact dependency request for this project.".into()
                        }
                    },
                },
            );
            if scope == crate::harness::DependencyAuthorizationScope::Once && !delivered {
                let project_id = match crate::persistence::dependency_authorization::project_id(
                    &project.state.repo_root,
                ) {
                    Ok(project_id) => project_id,
                    Err(error) => {
                        project.queue.last_error = format!(
                            "Could not bind this one-time authorization to the project: {error:#}"
                        );
                        if let Some(stored) =
                            project.activity.tasks.get_mut(ticket).and_then(|progress| {
                                progress
                                    .dependency_requests
                                    .iter_mut()
                                    .find(|stored| stored.id == request_id)
                            })
                        {
                            stored.decision =
                                crate::harness::DependencyDecision::RequiresUserAuthorization;
                            stored.status = crate::harness::DependencyRequestStatus::AwaitingUser;
                            stored.rationale = project.queue.last_error.clone();
                        }
                        project.save_task_activity(ticket);
                        project.activity.mark_ticket_dirty(ticket);
                        self.toasts.danger(project.queue.last_error.clone());
                        return;
                    }
                };
                if !crate::harness::dependency_authorization::remember_once_for_request(
                    &project_id,
                    &request,
                ) {
                    project.queue.last_error =
                        "Could not keep this one-time authorization in memory.".into();
                    if let Some(stored) =
                        project.activity.tasks.get_mut(ticket).and_then(|progress| {
                            progress
                                .dependency_requests
                                .iter_mut()
                                .find(|stored| stored.id == request_id)
                        })
                    {
                        stored.decision =
                            crate::harness::DependencyDecision::RequiresUserAuthorization;
                        stored.status = crate::harness::DependencyRequestStatus::AwaitingUser;
                        stored.rationale = project.queue.last_error.clone();
                    }
                    project.save_task_activity(ticket);
                    project.activity.mark_ticket_dirty(ticket);
                    self.toasts.danger(project.queue.last_error.clone());
                    return;
                }
            }
            resume = !delivered && !project.active_implementations.contains_key(ticket);
        }
        if resume {
            self.start_implementation(ticket.to_owned(), true);
        }
    }

    pub(super) fn deny_dependency_request(&mut self, ticket: &str, request_id: &str) {
        if let Screen::Connected(project) = &mut self.screen {
            let Some(progress) = project.activity.tasks.get_mut(ticket) else {
                return;
            };
            let Some(request) = progress
                .dependency_requests
                .iter_mut()
                .find(|request| request.id == request_id)
            else {
                return;
            };
            if request.status != crate::harness::DependencyRequestStatus::AwaitingUser {
                return;
            }
            request.decision = crate::harness::DependencyDecision::Reject;
            request.category = crate::harness::DependencyFailureCategory::DependencyPolicyDenied;
            request.status = crate::harness::DependencyRequestStatus::Denied;
            request.rationale = "You denied this dependency request in Task Details.".into();
            project.save_task_activity(ticket);
            project.activity.mark_ticket_dirty(ticket);
            let _ = crate::harness::dependency_authorization::answer(
                request_id,
                crate::harness::dependency_authorization::DependencyResolution {
                    decision: crate::harness::DependencyDecision::Reject,
                    scope: None,
                    rationale: "The user denied this dependency request in Task Details.".into(),
                },
            );
        }
    }
}
