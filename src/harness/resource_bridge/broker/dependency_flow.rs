mod activity;
mod preparation;
mod retry;

use activity::set_dependency_request;
use retry::record_retry_result;

use super::super::{
    MAX_REQUESTS, MAX_SESSION_BYTES, ResourceAction, ResourceRequest, ResourceResponse, dependency,
    fetch,
};
use super::{BrokerContext, DependencyRequest};
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

pub(super) fn prepare_request(
    context: &BrokerContext<'_>,
    request: &ResourceRequest,
) -> anyhow::Result<ResourceResponse> {
    // Admission accounting is atomic. Never hold the cache-operation mutex
    // while Man.ager or a user is deciding whether to authorize a dependency.
    let count = context.request_count.fetch_add(1, Ordering::Relaxed);
    anyhow::ensure!(
        count < MAX_REQUESTS,
        "Resource request limit reached for this task run"
    );
    let used = context.downloaded_bytes.load(Ordering::Relaxed);
    if request.action != ResourceAction::DependencyRetryResult {
        anyhow::ensure!(
            used < MAX_SESSION_BYTES,
            "Resource download budget reached for this task run"
        );
    }
    let response = match request.action {
        // Direct resource retrieval is intentionally separate from dependency
        // acquisition. Only the latter can use an allowlisted ecosystem adapter
        // with cleared host environment and isolated package-manager config.
        ResourceAction::Fetch if context.private_configuration => private_configuration_attention(),
        ResourceAction::Fetch => {
            // The lock serializes cache/budget mutations only, not pending reviews.
            let _gate = lock_cache(context)?;
            let used = context.downloaded_bytes.load(Ordering::Relaxed);
            anyhow::ensure!(
                used < MAX_SESSION_BYTES,
                "Resource download budget reached for this task run"
            );
            let url = request
                .url
                .as_deref()
                .ok_or_else(|| anyhow::anyhow!("Resource URL is required"))?;
            let reservation = (MAX_SESSION_BYTES - used).min(fetch::MAX_RESOURCE_BYTES as usize);
            context
                .downloaded_bytes
                .fetch_add(reservation, Ordering::Relaxed);
            let response = fetch::retrieve(
                context.resource_dir,
                url,
                &request.purpose,
                reservation as u64,
            )?;
            context.downloaded_bytes.fetch_sub(
                reservation.saturating_sub(response.bytes),
                Ordering::Relaxed,
            );
            response
        }
        ResourceAction::PrepareNpm => dependency_request(
            context,
            crate::harness::DependencyNeed {
                ecosystem: crate::harness::PackageEcosystem::Npm,
                package: None,
                version: None,
                source: Some("https://registry.npmjs.org".into()),
                command: "npm install".into(),
                reason: request.purpose.clone(),
                kind: crate::harness::DependencyKind::ExistingRestore,
                lockfile_identity: None,
                introduced_packages: Vec::new(),
            },
        )?,
        ResourceAction::PrepareNugetAudit => {
            let _gate = lock_cache(context)?;
            prepare_nuget_audit()?
        }
        ResourceAction::UnsupportedManager => {
            let need =
                dependency::from_unsupported_manager(request.manager.as_deref(), &request.purpose);
            dependency_request(context, need)?
        }
        ResourceAction::DependencyRequest => {
            let need = request
                .dependency
                .clone()
                .ok_or_else(|| anyhow::anyhow!("Structured dependency details are required"))?;
            dependency_request(context, need)?
        }
        ResourceAction::DependencyRetryResult => {
            let _gate = lock_cache(context)?;
            record_retry_result(
                context,
                request.dependency_request_id.as_deref(),
                request.retry_succeeded,
            )?
        }
    };
    if response.status == "needs_attention"
        && let Ok(mut pending) = context.attention.lock()
    {
        *pending = Some(response.summary.clone());
    }
    Ok(response)
}

fn lock_cache(context: &BrokerContext<'_>) -> anyhow::Result<std::sync::MutexGuard<'_, ()>> {
    context
        .request_gate
        .lock()
        .map_err(|_| anyhow::anyhow!("Resource request coordinator is unavailable"))
}

/// Begin package acquisition only after approval. This deliberately reacquires
/// the cache lock *after* waiting for a decision, so another resource request
/// can proceed while this one is under review.
fn prepare_authorized(
    context: &BrokerContext<'_>,
    request: &mut DependencyRequest,
    scope: Option<crate::harness::DependencyAuthorizationScope>,
) -> anyhow::Result<ResourceResponse> {
    let _gate = lock_cache(context)?;
    if context.cancel.load(Ordering::SeqCst) || context.stop.load(Ordering::Relaxed) {
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

fn private_configuration_attention() -> ResourceResponse {
    ResourceResponse::needs_attention(
        "Direct resource retrieval is unavailable while private project configuration is mounted. Existing verified package caches may be used through the mediated broker; fresh downloads are disabled.".into(),
    )
}

fn prepare_nuget_audit() -> anyhow::Result<ResourceResponse> {
    crate::harness::refresh_nuget_audit_cache(Duration::from_secs(90))?;
    Ok(ResourceResponse::prepared(
        "Kool.ad/e refreshed public NuGet vulnerability data for the verification retry".into(),
    ))
}

fn dependency_request(
    context: &BrokerContext<'_>,
    need: crate::harness::DependencyNeed,
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
            return prepare_authorized(context, &mut request, Some(scope));
        }
    }
    let receiver = crate::harness::dependency_authorization::register(&request.id)?;
    request.status = crate::harness::DependencyRequestStatus::ManagerReviewing;
    request.rationale =
        "Man.ager is checking whether this dependency is required and safe for the task.".into();
    set_dependency_request(context, &mut request);
    let deadline = Instant::now() + Duration::from_secs(20 * 60);
    let response = loop {
        if context.cancel.load(Ordering::SeqCst) || context.stop.load(Ordering::Relaxed) {
            request.status = crate::harness::DependencyRequestStatus::Failed;
            request.rationale =
                "The dependency review ended because the task was cancelled or stopped.".into();
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
        match receiver.recv_timeout(Duration::from_millis(200)) {
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
                break prepare_authorized(context, &mut request, answer.scope)?;
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
    crate::harness::dependency_authorization::unregister(&request.id);
    Ok(response)
}
