use super::*;

pub(super) fn invocation_outcome(
    result: &Result<HarnessOutcome, crate::error::AppError>,
) -> &'static str {
    match result {
        Ok(_) => "completed",
        Err(crate::error::AppError::HarnessTimedOut { .. }) => "timed-out",
        Err(crate::error::AppError::HarnessExecutionFailed { failure })
            if failure.category == crate::harness::HarnessFailureCategory::TimedOut =>
        {
            "timed-out"
        }
        Err(crate::error::AppError::HarnessExecutionFailed { failure })
            if failure.category == crate::harness::HarnessFailureCategory::Cancelled =>
        {
            "cancelled"
        }
        Err(crate::error::AppError::HarnessFailed { reason, .. })
            if reason.to_ascii_lowercase().contains("cancel") =>
        {
            "cancelled"
        }
        Err(crate::error::AppError::Other(reason))
            if reason.to_ascii_lowercase().contains("needs attention") =>
        {
            "user-action-required"
        }
        Err(_) => "failed",
    }
}

pub(super) fn split_harness(label: &str) -> (&str, Option<String>) {
    label
        .split_once(' ')
        .map_or((label, None), |(name, version)| {
            (name, Some(version.into()))
        })
}
