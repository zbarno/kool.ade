use super::{BrokerContext, DependencyRequest, set_dependency_request};
use crate::harness::resource_bridge::{ResourceResponse, adapter};

pub(super) fn authorized_dependency(
    context: &BrokerContext<'_>,
    request: &mut DependencyRequest,
    scope: Option<crate::harness::DependencyAuthorizationScope>,
) -> ResourceResponse {
    if !crate::harness::dependency_decision_allowed(&request.need, request.decision) {
        request.category = crate::harness::DependencyFailureCategory::DependencyPolicyDenied;
        request.status = crate::harness::DependencyRequestStatus::Failed;
        request.rationale = "The requested authorization failed deterministic source, package identity, ecosystem, version, or scope validation.".into();
        set_dependency_request(context, request);
        return ResourceResponse::dependency_outcome("rejected", request.clone());
    }
    if let Some(crate::harness::DependencyAuthorizationScope::Project) = scope
        && let Err(error) = crate::persistence::dependency_authorization::save(
            context.worktree,
            request,
            crate::harness::DependencyAuthorizationScope::Project,
        )
    {
        request.status = crate::harness::DependencyRequestStatus::Failed;
        request.rationale =
            format!("Project authorization could not be stored privately: {error:#}");
        set_dependency_request(context, request);
        return ResourceResponse::dependency_outcome("error", request.clone());
    }
    request.status = crate::harness::DependencyRequestStatus::Authorized;
    set_dependency_request(context, request);

    let Some(package_adapter) = adapter::for_ecosystem(request.need.ecosystem) else {
        request.category = crate::harness::DependencyFailureCategory::DependencyManagerUnsupported;
        request.status = crate::harness::DependencyRequestStatus::Failed;
        request.rationale = "Man.ager authorized the request, but Kool.ad/e has no acquisition adapter for this package manager. The worker remains offline.".into();
        set_dependency_request(context, request);
        return ResourceResponse::dependency_outcome("unsupported", request.clone());
    };
    if package_adapter.ecosystem() != request.need.ecosystem {
        request.category = crate::harness::DependencyFailureCategory::DependencyPolicyDenied;
        request.status = crate::harness::DependencyRequestStatus::Failed;
        request.rationale =
            "The package adapter did not match the requested ecosystem; no package was prepared."
                .into();
        set_dependency_request(context, request);
        return ResourceResponse::dependency_outcome("rejected", request.clone());
    }
    let preparation = adapter::PreparationContext {
        decision: request.decision,
        worktree: context.worktree,
        resource_dir: context.resource_dir,
        npm_cache: context.npm_cache,
        npm_snapshot: context.npm_snapshot,
        cargo_cache: context.cargo_cache,
        downloaded_bytes: context.downloaded_bytes,
        npm_operations: context.npm_operations,
    };
    match package_adapter.prepare(&preparation, &request.need) {
        Ok(prepared) if prepared.status == "prepared" => {
            request.status = crate::harness::DependencyRequestStatus::Prepared;
            request.rationale = format!(
                "Man.ager authorized this {} dependency request and its adapter prepared the offline package cache. {}",
                package_adapter.ecosystem_label(),
                prepared.summary
            );
            set_dependency_request(context, request);
            ResourceResponse::dependency_outcome("prepared", request.clone())
        }
        Ok(prepared) => {
            classify_preparation_failure(request, &prepared.summary);
            request.status = crate::harness::DependencyRequestStatus::Failed;
            request.rationale = prepared.summary;
            set_dependency_request(context, request);
            ResourceResponse::dependency_outcome("needs_attention", request.clone())
        }
        Err(error) => {
            classify_preparation_failure(request, &format!("{error:#}"));
            request.status = crate::harness::DependencyRequestStatus::Failed;
            request.rationale = format!(
                "The authorized {} dependency request could not be prepared safely: {error:#}",
                package_adapter.ecosystem_label()
            );
            set_dependency_request(context, request);
            ResourceResponse::dependency_outcome("needs_attention", request.clone())
        }
    }
}

fn classify_preparation_failure(request: &mut DependencyRequest, detail: &str) {
    let detail = detail.to_ascii_lowercase();
    request.category = if ["integrity", "checksum", "digest", "hash"]
        .iter()
        .any(|marker| detail.contains(marker))
    {
        crate::harness::DependencyFailureCategory::DependencyIntegrityFailure
    } else if detail.contains("lockfile") {
        crate::harness::DependencyFailureCategory::DependencyRestoreRequired
    } else if detail.contains("private registry") || detail.contains("credentials") {
        crate::harness::DependencyFailureCategory::DependencyPrivateRegistry
    } else if ["registry", "source", "url", "host", "redirect"]
        .iter()
        .any(|marker| detail.contains(marker))
    {
        crate::harness::DependencyFailureCategory::DependencySourceNotAuthorized
    } else if detail.contains("policy") || detail.contains("configuration") {
        crate::harness::DependencyFailureCategory::DependencyPolicyDenied
    } else {
        request.category
    };
}
