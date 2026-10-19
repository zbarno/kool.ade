use super::{BrokerContext, DependencyRequest, set_dependency_request};
use crate::harness::resource_bridge::ResourceResponse;

pub(super) fn prepare(
    context: &BrokerContext<'_>,
    request: &mut DependencyRequest,
    scope: Option<crate::harness::DependencyAuthorizationScope>,
) -> anyhow::Result<ResourceResponse> {
    if request.decision != crate::harness::DependencyDecision::UserAuthorizeForTask
        || scope != Some(crate::harness::DependencyAuthorizationScope::Once)
    {
        return failed(
            context,
            request,
            "System images require a one-time task-scoped user grant.",
        );
    }
    let Some(session) = context.execution_session.as_ref() else {
        return failed(
            context,
            request,
            "The controller-owned execution session is unavailable.",
        );
    };
    match session.prepare_authorized_system_dependency(&request.need, request.decision) {
        Ok(image) => {
            request.status = crate::harness::DependencyRequestStatus::Prepared;
            request.category =
                crate::harness::DependencyFailureCategory::DependencySystemPackageRequired;
            request.rationale = format!(
                "Built and selected verified offline image {} with {} Debian packages.",
                image.image_id,
                image.package_manifest.len()
            );
            request.preparation = Some(crate::harness::DependencyPreparationTelemetry {
                status: Some(crate::harness::DependencyPreparationStatus::Prepared),
                package_count: image.package_manifest.len() as u64,
                authorization_source: Some(crate::harness::DependencyAuthorizationSource::User),
                ..Default::default()
            });
            set_dependency_request(context, request)?;
            Ok(ResourceResponse::dependency_outcome(
                "prepared",
                request.clone(),
            ))
        }
        Err(error) => failed(
            context,
            request,
            &format!("Offline Debian image derivation failed: {error}"),
        ),
    }
}

fn failed(
    context: &BrokerContext<'_>,
    request: &mut DependencyRequest,
    reason: &str,
) -> anyhow::Result<ResourceResponse> {
    request.status = crate::harness::DependencyRequestStatus::Failed;
    request.category = crate::harness::DependencyFailureCategory::DependencySystemPackageRequired;
    request.rationale = reason.to_owned();
    request.preparation = Some(crate::harness::DependencyPreparationTelemetry {
        status: Some(crate::harness::DependencyPreparationStatus::Error),
        authorization_source: super::super::activity::authorization_source(request.decision),
        ..Default::default()
    });
    set_dependency_request(context, request)?;
    Ok(ResourceResponse::dependency_outcome(
        "needs_attention",
        request.clone(),
    ))
}
