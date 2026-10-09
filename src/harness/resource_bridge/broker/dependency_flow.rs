pub(super) mod activity;
mod authorization;
mod preparation;
mod retry;

use activity::set_dependency_request;
use authorization::dependency_request;
use retry::record_retry_result;

use super::super::{
    MAX_REQUESTS, MAX_SESSION_BYTES, ResourceAction, ResourceRequest, ResourceResponse, dependency,
    fetch,
};
use super::{BrokerContext, DependencyRequest};
use std::sync::atomic::Ordering;
use std::time::Duration;

pub(super) fn prepare_request(
    context: &BrokerContext<'_>,
    request: &ResourceRequest,
    disconnected: &std::sync::atomic::AtomicBool,
) -> anyhow::Result<ResourceResponse> {
    super::super::budget::admit_request(context.request_count, MAX_REQUESTS)?;
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
            let url = request
                .url
                .as_deref()
                .ok_or_else(|| anyhow::anyhow!("Resource URL is required"))?;
            let reservation = super::super::budget::reserve_downloads(
                context.downloaded_bytes,
                fetch::MAX_RESOURCE_BYTES as usize,
                MAX_SESSION_BYTES,
            )?;
            let result = fetch::retrieve(
                context.resource_dir,
                url,
                &request.purpose,
                reservation as u64,
            );
            match result {
                Ok(response) => {
                    super::super::budget::settle_downloads(
                        context.downloaded_bytes,
                        reservation,
                        response.bytes,
                    );
                    response
                }
                Err(error) => {
                    let received = fetch::bytes_from_failure(&error).unwrap_or_default();
                    super::super::budget::settle_downloads(
                        context.downloaded_bytes,
                        reservation,
                        received,
                    );
                    return Err(error);
                }
            }
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
            disconnected,
        )?,
        ResourceAction::PrepareNugetAudit => prepare_nuget_audit()?,
        ResourceAction::UnsupportedManager => {
            let need =
                dependency::from_unsupported_manager(request.manager.as_deref(), &request.purpose);
            dependency_request(context, need, disconnected)?
        }
        ResourceAction::DependencyRequest => {
            let need = request
                .dependency
                .clone()
                .ok_or_else(|| anyhow::anyhow!("Structured dependency details are required"))?;
            dependency_request(context, need, disconnected)?
        }
        ResourceAction::DependencyRetryResult => record_retry_result(
            context,
            request.dependency_request_id.as_deref(),
            request.retry_succeeded,
        )?,
    };
    if response.status == "needs_attention"
        && let Ok(mut pending) = context.attention.lock()
    {
        *pending = Some(response.summary.clone());
    }
    Ok(response)
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
