pub(super) fn advance_dependency_review(
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
        let harness = crate::app::root::configured_harness_for_project(
            task_harness,
            Some(crate::persistence::harness_settings::MANAGER),
            &project.state.harness_routes.routes,
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
