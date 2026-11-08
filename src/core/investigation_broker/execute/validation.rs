use super::super::{
    helpers::stale_receipt,
    types::{InvestigationOutcomeV1, InvestigationReceiptV1, InvestigationRequestV1},
};
use super::InvestigationContext;
use crate::core::repository_knowledge::verify_profile_freshness_with_deadline;
use std::sync::atomic::AtomicBool;

pub(super) fn validate_context(
    context: &InvestigationContext,
    request: &InvestigationRequestV1,
    deadline: std::time::Instant,
    cancelled: &AtomicBool,
) -> Result<bool, String> {
    context.profile.validate()?;
    if context.source.repository_id != request.repository_id
        || context.source.source_ref != request.source_ref
        || context.profile.key.commit_sha != request.source_sha
        || context.workspace_id != request.workspace_id
        || context.task_id != request.task_id
        || context.cancellation_id != request.cancellation_id
        || context.task_revision != request.expected_task_revision
    {
        return Err(
            "investigation authorization does not match the selected repository or task revision"
                .into(),
        );
    }
    match (&request.comparison, &context.comparison) {
        (None, None) => {}
        (Some(requested), Some(selected)) => {
            if selected.source.repository_id != request.repository_id
                || selected.source.source_ref != requested.source_ref
                || selected.profile.key.source_ref != requested.source_ref
            {
                return Err(
                    "comparison authorization does not match the selected repository".into(),
                );
            }
            if selected.profile.key.commit_sha != requested.source_sha {
                return Ok(false);
            }
            selected.profile.validate()?;
        }
        _ => return Err("comparison revision is not app-authorized".into()),
    }
    let task_revision = context
        .source
        .planning_store
        .revision()
        .map_err(|_| "planning revision is unavailable".to_owned())?;
    if task_revision != request.expected_task_revision {
        return Ok(false);
    }
    match verify_profile_freshness_with_deadline(
        &context.source,
        &context.profile,
        deadline,
        cancelled,
    ) {
        Ok(()) => {
            if let Some(selected) = &context.comparison {
                match verify_profile_freshness_with_deadline(
                    &selected.source,
                    &selected.profile,
                    deadline,
                    cancelled,
                ) {
                    Ok(()) => Ok(true),
                    Err(error)
                        if error.contains("cancelled") || error.contains("wall-time budget") =>
                    {
                        Err(error)
                    }
                    Err(_) => Ok(false),
                }
            } else {
                Ok(true)
            }
        }
        Err(error) if error.contains("cancelled") || error.contains("wall-time budget") => {
            Err(error)
        }
        Err(_) => Ok(false),
    }
}

pub(super) fn status_receipt(
    request: &InvestigationRequestV1,
    outcome: InvestigationOutcomeV1,
    detail: String,
) -> InvestigationReceiptV1 {
    let mut receipt = stale_receipt(request);
    receipt.outcome = outcome;
    receipt.uncertainty = vec![detail];
    receipt
}
