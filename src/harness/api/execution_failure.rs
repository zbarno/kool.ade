use crate::{
    error::AppError,
    execution::contract::HarnessRole,
    harness::{ExecutionMode, PlanningRequest},
};
use serde::{Deserialize, Serialize};

const MAX_FAILURE_MESSAGE_BYTES: usize = 4096;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HarnessFailureCategory {
    ProviderUnavailable,
    Authentication,
    HostCodeModeDisabled,
    ToolBridgeUnavailable,
    UnsupportedProviderCapability,
    ProviderRejectedRequest,
    Cancelled,
    TimedOut,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HarnessExecutionFailure {
    pub provider_id: String,
    pub role: HarnessRole,
    pub mode: ExecutionMode,
    /// Task workspace identity, when the application bound this turn to one.
    pub target: Option<String>,
    pub category: HarnessFailureCategory,
    pub provider_code: Option<String>,
    pub sanitized_message: String,
    pub actionable_fix: Option<String>,
    /// False unless the application can prove a retry cannot duplicate work.
    pub retryable: bool,
}

pub(crate) fn normalize_provider_error(
    provider_id: &str,
    role: HarnessRole,
    request: &PlanningRequest,
    error: AppError,
) -> AppError {
    let (category, message, provider_code) = match error {
        AppError::HarnessNotFound { detail } => {
            (HarnessFailureCategory::ProviderUnavailable, detail, None)
        }
        AppError::Other(message) if is_provider_executable_failure(&message) => {
            (HarnessFailureCategory::ProviderUnavailable, message, None)
        }
        AppError::HarnessFailed {
            reason,
            stderr_tail,
        } => {
            let combined = if stderr_tail.trim().is_empty() {
                reason
            } else {
                format!("{reason}\n{}", stderr_tail.trim())
            };
            let category = if combined
                .to_ascii_lowercase()
                .contains("no configured application-owned docker execution policy")
            {
                HarnessFailureCategory::UnsupportedProviderCapability
            } else {
                classify(&combined)
            };
            let provider_code = extract_provider_code(&combined);
            (category, combined, provider_code)
        }
        AppError::HarnessTimedOut { secs } => (
            HarnessFailureCategory::TimedOut,
            format!("provider operation exceeded its {secs}s time limit"),
            None,
        ),
        AppError::ProviderCapabilityMismatch { provider, missing } => (
            HarnessFailureCategory::UnsupportedProviderCapability,
            format!(
                "{provider} is missing required capabilities: {}",
                missing.join(", ")
            ),
            None,
        ),
        AppError::Other(message) if is_execution_target_failure(&message) => {
            (HarnessFailureCategory::ToolBridgeUnavailable, message, None)
        }
        other => return other,
    };
    let sanitized_message = bounded_redacted(&message);
    let actionable_fix = remediation(category, request.mode);
    AppError::HarnessExecutionFailed {
        failure: Box::new(HarnessExecutionFailure {
            provider_id: provider_id.to_owned(),
            role,
            mode: request.mode,
            target: request
                .execution_workspace
                .as_ref()
                .map(|workspace| workspace.task_id.clone()),
            category,
            provider_code,
            sanitized_message,
            actionable_fix: Some(actionable_fix),
            retryable: false,
        }),
    }
}

fn extract_provider_code(message: &str) -> Option<String> {
    message.lines().find_map(|line| {
        let value = serde_json::from_str::<serde_json::Value>(line).ok()?;
        extract_code_from_value(&value)
    })
}

fn extract_code_from_value(value: &serde_json::Value) -> Option<String> {
    match value {
        serde_json::Value::Object(object) => {
            for key in ["provider_code", "error_code", "code"] {
                if let Some(value) = object.get(key) {
                    if let Some(code) = value.as_str() {
                        if valid_provider_code(code) {
                            return Some(code.to_owned());
                        }
                    } else if let Some(code) = value.as_i64() {
                        return Some(code.to_string());
                    }
                }
            }
            object.values().find_map(extract_code_from_value)
        }
        serde_json::Value::Array(values) => values.iter().find_map(extract_code_from_value),
        _ => None,
    }
}

fn valid_provider_code(code: &str) -> bool {
    let code = code.trim();
    !code.is_empty()
        && code.len() <= 128
        && code
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
}

fn is_execution_target_failure(message: &str) -> bool {
    let normalized = message.to_ascii_lowercase();
    normalized.starts_with("cannot start kool.ad/e sandbox:")
        && normalized.contains("docker")
        && [
            "execution session",
            "implementation session",
            "docker engine",
            "execution target",
        ]
        .iter()
        .any(|needle| normalized.contains(needle))
}

fn is_provider_executable_failure(message: &str) -> bool {
    let normalized = message.to_ascii_lowercase();
    normalized.contains("cannot resolve ")
        && normalized.contains("executable")
        && (normalized.contains("cli") || normalized.contains("opencode"))
}

fn classify(message: &str) -> HarnessFailureCategory {
    let normalized = message.to_ascii_lowercase();
    if normalized.contains("cancelled by user") || normalized.contains("canceled by user") {
        HarnessFailureCategory::Cancelled
    } else if normalized.contains("code mode")
        && normalized.contains("host")
        && (normalized.contains("disabled") || normalized.contains("not available"))
    {
        HarnessFailureCategory::HostCodeModeDisabled
    } else if normalized.contains("mcp")
        && (normalized.contains("unavailable")
            || normalized.contains("not configured")
            || normalized.contains("not advertised"))
    {
        HarnessFailureCategory::ToolBridgeUnavailable
    } else if [
        "authentication",
        "unauthorized",
        "not logged in",
        "login required",
    ]
    .iter()
    .any(|needle| normalized.contains(needle))
    {
        HarnessFailureCategory::Authentication
    } else if [
        "unsupported",
        "unknown option",
        "unknown flag",
        "requires version",
    ]
    .iter()
    .any(|needle| normalized.contains(needle))
    {
        HarnessFailureCategory::UnsupportedProviderCapability
    } else {
        HarnessFailureCategory::ProviderRejectedRequest
    }
}

fn remediation(category: HarnessFailureCategory, mode: ExecutionMode) -> String {
    match category {
        HarnessFailureCategory::ProviderUnavailable => {
            "Install the selected CLI or choose a valid executable path in Coding tools, then refresh discovery. Kool.ad/e will not switch providers automatically.".into()
        }
        HarnessFailureCategory::Authentication => {
            "Refresh this provider's login in Coding tools, then retry the task.".into()
        }
        HarnessFailureCategory::HostCodeModeDisabled if mode.tool_access() == crate::harness::ToolAccess::None => {
            "This role has no repository tools; continue with the supplied context or choose a role that is authorized to inspect the project.".into()
        }
        HarnessFailureCategory::HostCodeModeDisabled => {
            "Host-native tools remain disabled. Confirm this task's approved Kool.ad/e Docker tool session is ready, then retry.".into()
        }
        HarnessFailureCategory::ToolBridgeUnavailable => {
            "Refresh Docker execution readiness and provider capabilities; no host command fallback is permitted.".into()
        }
        HarnessFailureCategory::UnsupportedProviderCapability => {
            "Refresh Coding tools discovery and update the provider CLI if the required capability remains unavailable.".into()
        }
        HarnessFailureCategory::Cancelled => {
            "No automatic retry was started. Review the task state before starting another attempt.".into()
        }
        HarnessFailureCategory::TimedOut => {
            "Review the task state and provider responsiveness before starting another attempt.".into()
        }
        HarnessFailureCategory::ProviderRejectedRequest => {
            "Review the provider message and selected model; retry only after confirming the task is safe to repeat.".into()
        }
    }
}

fn bounded_redacted(message: &str) -> String {
    let redacted = message
        .lines()
        .map(|line| {
            let structured_code = serde_json::from_str::<serde_json::Value>(line)
                .ok()
                .and_then(|value| extract_code_from_value(&value));
            structured_code.map_or_else(
                || crate::error::redact_secrets(line),
                |code| format!("provider returned structured error (code: {code})"),
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let mut bounded = String::new();
    for character in redacted.chars() {
        if bounded.len() + character.len_utf8() > MAX_FAILURE_MESSAGE_BYTES {
            break;
        }
        bounded.push(character);
    }
    bounded
}

#[cfg(test)]
#[path = "execution_failure/tests.rs"]
mod tests;
