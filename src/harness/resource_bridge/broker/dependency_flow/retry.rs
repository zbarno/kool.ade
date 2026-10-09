use super::{BrokerContext, set_dependency_request};
use crate::harness::resource_bridge::ResourceResponse;

pub(super) fn record_retry_result(
    context: &BrokerContext<'_>,
    request_id: Option<&str>,
    succeeded: Option<bool>,
) -> anyhow::Result<ResourceResponse> {
    let Some(request_id) = request_id.filter(|id| id.len() <= 128 && id.starts_with("dependency-"))
    else {
        return Ok(ResourceResponse::needs_attention(
            "The dependency retry result did not include a valid request ID.".into(),
        ));
    };
    let Some(succeeded) = succeeded else {
        return Ok(ResourceResponse::needs_attention(
            "The dependency retry result did not include a success state.".into(),
        ));
    };
    let Some(mut request) = context
        .dependency
        .lock()
        .map_err(|_| anyhow::anyhow!("Dependency activity is unavailable"))?
        .clone()
        .into_iter()
        .find(|request| request.id == request_id)
    else {
        return Ok(ResourceResponse::needs_attention(
            "The dependency retry result did not match the active request.".into(),
        ));
    };
    if request.status != crate::harness::DependencyRequestStatus::Prepared
        || request.preparation.as_ref().is_none_or(|preparation| {
            preparation.retry_result.is_some()
                || !matches!(
                    preparation.status,
                    Some(
                        crate::harness::DependencyPreparationStatus::Prepared
                            | crate::harness::DependencyPreparationStatus::AlreadyAvailable
                    )
                )
        })
    {
        return Ok(ResourceResponse::needs_attention(
            "The dependency retry result did not match a prepared request or was already recorded."
                .into(),
        ));
    }
    let preparation = request
        .preparation
        .get_or_insert_with(crate::harness::DependencyPreparationTelemetry::default);
    preparation.retry_result = Some(if succeeded {
        crate::harness::DependencyRetryResult::Succeeded
    } else {
        crate::harness::DependencyRetryResult::Failed
    });
    if succeeded {
        request.status = crate::harness::DependencyRequestStatus::Prepared;
        request.rationale = format!(
            "{} The bounded offline package operation retry succeeded.",
            request.rationale
        );
        let response = ResourceResponse::dependency_outcome("prepared", request.clone());
        set_dependency_request(context, &mut request);
        Ok(response)
    } else {
        preparation.status = Some(crate::harness::DependencyPreparationStatus::Error);
        request.status = crate::harness::DependencyRequestStatus::Failed;
        request.rationale = format!(
            "{} The bounded offline package operation retry failed.",
            request.rationale
        );
        set_dependency_request(context, &mut request);
        Ok(ResourceResponse::dependency_outcome(
            "needs_attention",
            request,
        ))
    }
}
