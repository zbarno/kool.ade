use super::activity::set_dependency_request;
use super::{BrokerContext, preparation};
use crate::harness::DependencyRequest;
use crate::harness::resource_bridge::{ResourceResponse, dependency};
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

pub(super) fn dependency_request(
    context: &BrokerContext<'_>,
    need: crate::harness::DependencyNeed,
    disconnected: &std::sync::atomic::AtomicBool,
) -> anyhow::Result<ResourceResponse> {
    let mut request = dependency::triage(context.task_id, need);
    if let Err(error) = dependency::enrich_lock_identity(
        context.worktree,
        context.baseline_commit,
        &mut request.need,
    ) {
        let detail = format!("{error:#}");
        request.category = if detail.contains("changed before authorization")
            || detail.contains("changed without matching lockfile")
        {
            crate::harness::DependencyFailureCategory::DependencyPolicyDenied
        } else {
            crate::harness::DependencyFailureCategory::DependencyIntegrityFailure
        };
        request.decision = crate::harness::DependencyDecision::Reject;
        request.status = crate::harness::DependencyRequestStatus::Failed;
        request.rationale =
            format!("Kool.ad/e could not safely authorize these dependency inputs: {detail}");
        set_dependency_request(context, &mut request);
        let status = if request.category
            == crate::harness::DependencyFailureCategory::DependencyIntegrityFailure
        {
            "integrity_failure"
        } else {
            "rejected"
        };
        return Ok(ResourceResponse::dependency_outcome(status, request));
    }
    if !request.need.introduced_packages.is_empty() {
        request.category = crate::harness::DependencyFailureCategory::DependencyNewPackageRequested;
        request.rationale = format!(
            "The app found {} package identities in the lockfile that are absent from the task's starting commit. Man.ager must review the listed packages before the broker restores them.",
            request.need.introduced_packages.len()
        );
        request.risk = "A new or changed lockfile package set was introduced during this task. Any grant is tied to this exact lockfile identity.".into();
    }
    if request.decision == crate::harness::DependencyDecision::Reject {
        request.status = crate::harness::DependencyRequestStatus::Failed;
        set_dependency_request(context, &mut request);
        return Ok(ResourceResponse::dependency_outcome("rejected", request));
    }
    if let Some(task_id) = context.task_id {
        if let Ok(project_id) =
            crate::persistence::dependency_authorization::project_id(context.worktree)
            && crate::harness::dependency_authorization::take_once(
                &project_id,
                task_id,
                &request.need,
            )
        {
            request.decision = crate::harness::DependencyDecision::UserAuthorizeForTask;
            request.status = crate::harness::DependencyRequestStatus::Authorized;
            request.rationale =
                "The user authorized this exact dependency request once for the current task."
                    .into();
            set_dependency_request(context, &mut request);
            return prepare_authorized(
                context,
                &mut request,
                Some(crate::harness::DependencyAuthorizationScope::Once),
                disconnected,
            );
        }
        let saved_scope = match crate::persistence::dependency_authorization::matching_scope(
            context.worktree,
            task_id,
            &request.need,
        ) {
            Ok(scope) => scope,
            Err(error) => {
                request.status = crate::harness::DependencyRequestStatus::Failed;
                request.rationale =
                    format!("Saved dependency authorization could not be read safely: {error:#}");
                set_dependency_request(context, &mut request);
                return Ok(ResourceResponse::dependency_outcome("error", request));
            }
        };
        if let Some(scope) = saved_scope {
            request.decision = match scope {
                crate::harness::DependencyAuthorizationScope::Once => {
                    crate::harness::DependencyDecision::UserAuthorizeForTask
                }
                crate::harness::DependencyAuthorizationScope::Project => {
                    crate::harness::DependencyDecision::UserAuthorizeForProject
                }
            };
            request.status = crate::harness::DependencyRequestStatus::Authorized;
            request.rationale = "A user-local grant matches this exact package, version, source, and task or project scope.".into();
            set_dependency_request(context, &mut request);
            return prepare_authorized(context, &mut request, Some(scope), disconnected);
        }
    }
    let registration = match crate::harness::dependency_authorization::register(
        &request.id,
        &request.task_id,
        &request.need,
    ) {
        Ok(registration) => registration,
        Err(error) => {
            request.status = crate::harness::DependencyRequestStatus::Failed;
            request.rationale =
                format!("Kool.ad/e could not queue this authorization safely: {error:#}");
            set_dependency_request(context, &mut request);
            return Ok(ResourceResponse::dependency_outcome(
                "needs_attention",
                request,
            ));
        }
    };
    request.status = crate::harness::DependencyRequestStatus::ManagerReviewing;
    request.rationale =
        "Man.ager is checking whether this dependency is required and safe for the task.".into();
    set_dependency_request(context, &mut request);
    let deadline = Instant::now() + Duration::from_secs(20 * 60);
    let response = loop {
        if context.cancel.load(Ordering::SeqCst) || context.stop.load(Ordering::Relaxed) {
            registration.unregister();
            request.status = crate::harness::DependencyRequestStatus::Failed;
            request.rationale =
                "The dependency review ended because the task was cancelled or stopped.".into();
            set_dependency_request(context, &mut request);
            break ResourceResponse::dependency_outcome("cancelled", request.clone());
        }
        if disconnected.load(Ordering::Acquire) {
            registration.unregister();
            request.status = crate::harness::DependencyRequestStatus::Failed;
            request.rationale =
                "The resource client disconnected while dependency authorization was pending; no permission was granted.".into();
            set_dependency_request(context, &mut request);
            break ResourceResponse::dependency_outcome("cancelled", request.clone());
        }
        if Instant::now() >= deadline {
            request.decision = crate::harness::DependencyDecision::RequiresUserAuthorization;
            request.status = crate::harness::DependencyRequestStatus::AwaitingUser;
            request.rationale = "Man.ager did not finish this review in time. No dependency permission was granted; review this request in Task Details.".into();
            set_dependency_request(context, &mut request);
            break ResourceResponse::dependency_outcome("authorization_required", request.clone());
        }
        match registration.recv_timeout(Duration::from_millis(200)) {
            Ok(answer) => {
                request.decision = answer.decision;
                request.rationale = answer.rationale;
                if answer.decision == crate::harness::DependencyDecision::RequiresUserAuthorization
                {
                    request.status = crate::harness::DependencyRequestStatus::AwaitingUser;
                    set_dependency_request(context, &mut request);
                    continue;
                }
                if answer.decision == crate::harness::DependencyDecision::Reject {
                    request.status = crate::harness::DependencyRequestStatus::Denied;
                    set_dependency_request(context, &mut request);
                    break ResourceResponse::dependency_outcome("denied", request.clone());
                }
                break prepare_authorized(context, &mut request, answer.scope, disconnected)?;
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                request.decision = crate::harness::DependencyDecision::RequiresUserAuthorization;
                request.status = crate::harness::DependencyRequestStatus::AwaitingUser;
                request.rationale = "The dependency review channel ended without authorization. No permission was granted.".into();
                set_dependency_request(context, &mut request);
                break ResourceResponse::dependency_outcome(
                    "authorization_required",
                    request.clone(),
                );
            }
        }
    };
    Ok(response)
}

fn prepare_authorized(
    context: &BrokerContext<'_>,
    request: &mut DependencyRequest,
    scope: Option<crate::harness::DependencyAuthorizationScope>,
    disconnected: &std::sync::atomic::AtomicBool,
) -> anyhow::Result<ResourceResponse> {
    if let Some(hook) = context.before_authorized {
        hook();
    }
    if context.cancel.load(Ordering::SeqCst)
        || context.stop.load(Ordering::Relaxed)
        || disconnected.load(Ordering::Acquire)
    {
        request.status = crate::harness::DependencyRequestStatus::Failed;
        request.rationale =
            "The dependency review ended because the task was cancelled or stopped.".into();
        set_dependency_request(context, request);
        return Ok(ResourceResponse::dependency_outcome(
            "cancelled",
            request.clone(),
        ));
    }
    Ok(preparation::authorized_dependency(context, request, scope))
}
