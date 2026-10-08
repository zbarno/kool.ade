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
        request.preparation = Some(failed_preparation(
            request,
            crate::harness::DependencyPreparationStatus::SourceRejected,
        ));
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
        request.preparation = Some(failed_preparation(
            request,
            crate::harness::DependencyPreparationStatus::Error,
        ));
        set_dependency_request(context, request);
        return ResourceResponse::dependency_outcome("error", request.clone());
    }
    request.status = crate::harness::DependencyRequestStatus::Authorized;
    set_dependency_request(context, request);

    let Some(package_adapter) = adapter::for_ecosystem(request.need.ecosystem) else {
        request.category = crate::harness::DependencyFailureCategory::DependencyManagerUnsupported;
        request.status = crate::harness::DependencyRequestStatus::Failed;
        request.rationale = "Man.ager authorized the request, but Kool.ad/e has no acquisition adapter for this package manager. The worker remains offline.".into();
        request.preparation = Some(failed_preparation(
            request,
            crate::harness::DependencyPreparationStatus::Unsupported,
        ));
        set_dependency_request(context, request);
        return ResourceResponse::dependency_outcome("unsupported", request.clone());
    };
    if package_adapter.ecosystem() != request.need.ecosystem {
        request.category = crate::harness::DependencyFailureCategory::DependencyPolicyDenied;
        request.status = crate::harness::DependencyRequestStatus::Failed;
        request.rationale =
            "The package adapter did not match the requested ecosystem; no package was prepared."
                .into();
        request.preparation = Some(failed_preparation(
            request,
            crate::harness::DependencyPreparationStatus::SourceRejected,
        ));
        set_dependency_request(context, request);
        return ResourceResponse::dependency_outcome("rejected", request.clone());
    }
    let preparation = adapter::PreparationContext {
        decision: request.decision,
        allow_downloads: !context.private_configuration,
        worktree: context.worktree,
        resource_dir: context.resource_dir,
        npm_cache: context.npm_cache,
        npm_snapshot: context.npm_snapshot,
        cargo_cache: context.cargo_cache,
        downloaded_bytes: context.downloaded_bytes,
        npm_operations: context.npm_operations,
    };
    let bytes_before = context
        .downloaded_bytes
        .load(std::sync::atomic::Ordering::Relaxed);
    match package_adapter.prepare(&preparation, &request.need) {
        Ok(mut prepared) if prepared.status == "prepared" => {
            request.preparation = Some(preparation_result(request, &mut prepared));
            request.status = crate::harness::DependencyRequestStatus::Prepared;
            request.rationale = format!(
                "Man.ager authorized this {} dependency request and its adapter prepared the offline package cache. {}",
                package_adapter.ecosystem_label(),
                prepared.summary
            );
            set_dependency_request(context, request);
            ResourceResponse::dependency_outcome("prepared", request.clone())
        }
        Ok(mut prepared) => {
            classify_preparation_failure(request, &prepared.summary);
            let telemetry = preparation_result(request, &mut prepared);
            request.status = crate::harness::DependencyRequestStatus::Failed;
            request.rationale = prepared.summary;
            request.preparation = Some(telemetry);
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
            let mut telemetry = failed_preparation(request, preparation_status(request.category));
            telemetry.bytes_downloaded = context
                .downloaded_bytes
                .load(std::sync::atomic::Ordering::Relaxed)
                .saturating_sub(bytes_before) as u64;
            request.preparation = Some(telemetry);
            set_dependency_request(context, request);
            ResourceResponse::dependency_outcome("needs_attention", request.clone())
        }
    }
}

fn preparation_result(
    request: &DependencyRequest,
    response: &mut ResourceResponse,
) -> crate::harness::DependencyPreparationTelemetry {
    let mut telemetry = response.preparation.take().unwrap_or_default();
    telemetry.authorization_source = super::activity::authorization_source(request.decision);
    if telemetry.bytes_downloaded == 0 {
        telemetry.bytes_downloaded = response.bytes as u64;
    }
    if telemetry.status.is_none() {
        telemetry.status = Some(if response.status == "prepared" {
            crate::harness::DependencyPreparationStatus::Prepared
        } else {
            preparation_status(request.category)
        });
    }
    telemetry
}

fn failed_preparation(
    request: &DependencyRequest,
    status: crate::harness::DependencyPreparationStatus,
) -> crate::harness::DependencyPreparationTelemetry {
    crate::harness::DependencyPreparationTelemetry {
        status: Some(status),
        authorization_source: super::activity::authorization_source(request.decision),
        ..Default::default()
    }
}

fn preparation_status(
    category: crate::harness::DependencyFailureCategory,
) -> crate::harness::DependencyPreparationStatus {
    match category {
        crate::harness::DependencyFailureCategory::DependencyPrivateRegistry => {
            crate::harness::DependencyPreparationStatus::CredentialsRequired
        }
        crate::harness::DependencyFailureCategory::DependencyIntegrityFailure => {
            crate::harness::DependencyPreparationStatus::IntegrityFailure
        }
        crate::harness::DependencyFailureCategory::DependencySourceNotAuthorized => {
            crate::harness::DependencyPreparationStatus::SourceRejected
        }
        crate::harness::DependencyFailureCategory::DependencyManagerUnsupported => {
            crate::harness::DependencyPreparationStatus::Unsupported
        }
        crate::harness::DependencyFailureCategory::DependencyPolicyDenied => {
            crate::harness::DependencyPreparationStatus::Denied
        }
        _ => crate::harness::DependencyPreparationStatus::Error,
    }
}

fn classify_preparation_failure(request: &mut DependencyRequest, detail: &str) {
    let detail = detail.to_ascii_lowercase();
    request.category = if detail.contains("changed after authorization")
        || ["integrity", "checksum", "digest", "hash"]
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
