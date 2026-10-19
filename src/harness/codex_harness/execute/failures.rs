use crate::error::AppError;
use crate::harness::ToolAccess;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CodexFailureCategory {
    HostCodeModeDisabled,
    ProviderFailure,
}

fn classify_provider_failure(reason: &str) -> CodexFailureCategory {
    let normalized = reason.to_ascii_lowercase();
    if normalized.contains("code mode")
        && normalized.contains("host")
        && (normalized.contains("disabled") || normalized.contains("not available"))
    {
        CodexFailureCategory::HostCodeModeDisabled
    } else {
        CodexFailureCategory::ProviderFailure
    }
}

pub(super) fn is_host_code_mode_disabled(reason: &str) -> bool {
    classify_provider_failure(reason) == CodexFailureCategory::HostCodeModeDisabled
}

pub(super) fn diagnose_provider_failure(reason: &str, access: ToolAccess) -> String {
    match classify_provider_failure(reason) {
        CodexFailureCategory::HostCodeModeDisabled if access != ToolAccess::None => format!(
            "Codex host code mode is disabled by Kool.ad/e policy. The application MCP tools are configured for this turn; use those tools for repository access. Provider message: {reason}"
        ),
        CodexFailureCategory::HostCodeModeDisabled => format!(
            "Codex host code mode is disabled by Kool.ad/e policy, and this turn has no repository tools. Provider message: {reason}"
        ),
        CodexFailureCategory::ProviderFailure => reason.to_owned(),
    }
}

pub(super) fn provider_failure(reason: &str, access: ToolAccess, stderr_tail: String) -> AppError {
    AppError::HarnessFailed {
        reason: diagnose_provider_failure(reason, access),
        stderr_tail,
    }
}

#[cfg(test)]
mod tests {
    use super::{CodexFailureCategory, classify_provider_failure, diagnose_provider_failure};
    use crate::harness::ToolAccess;

    #[test]
    fn host_code_mode_error_points_to_the_configured_application_tools() {
        let provider_message = "Code mode is disabled on host";
        assert_eq!(
            classify_provider_failure(provider_message),
            CodexFailureCategory::HostCodeModeDisabled
        );
        let diagnostic =
            diagnose_provider_failure(provider_message, ToolAccess::BoundedImplementation);
        assert!(diagnostic.contains("host code mode is disabled by Kool.ad/e policy"));
        assert!(diagnostic.contains("application MCP tools are configured"));
        assert!(diagnostic.contains("use those tools for repository access"));
    }

    #[test]
    fn host_code_mode_event_is_recognized_as_recoverable_when_a_turn_continues() {
        let Some(crate::harness::codex_harness::CodexEvent::Failure(provider_message)) =
            crate::harness::codex_harness::parse_event(
                r#"{"type":"turn.failed","error":{"message":"code mode is disabled on host"}}"#,
            )
        else {
            panic!("Codex failure event was not parsed");
        };
        assert!(super::super::is_host_code_mode_disabled(&provider_message));
        assert!(
            diagnose_provider_failure(&provider_message, ToolAccess::BoundedImplementation)
                .contains("application MCP tools are configured")
        );
    }

    #[test]
    fn host_code_mode_error_explains_when_the_turn_has_no_repository_tools() {
        let diagnostic =
            diagnose_provider_failure("Host code mode is not available", ToolAccess::None);
        assert!(diagnostic.contains("this turn has no repository tools"));
    }

    #[test]
    fn host_code_mode_denial_is_actionable_for_each_harness_mode() {
        for mode in crate::harness::ExecutionMode::ALL {
            let diagnostic =
                diagnose_provider_failure("Host code mode is disabled", mode.tool_access());
            assert!(
                diagnostic.contains("disabled by Kool.ad/e policy"),
                "{mode:?}"
            );
            if mode.tool_access() == ToolAccess::None {
                assert!(
                    diagnostic.contains("this turn has no repository tools"),
                    "{mode:?}"
                );
            } else {
                assert!(
                    diagnostic.contains("application MCP tools are configured"),
                    "{mode:?}"
                );
            }
        }
    }

    #[test]
    fn unrelated_provider_failures_keep_the_original_message() {
        let diagnostic = diagnose_provider_failure("authentication expired", ToolAccess::None);
        assert_eq!(diagnostic, "authentication expired");
    }
}
