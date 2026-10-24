//! Recomputed local setup blockers surfaced on the project board.
//!
//! These are machine configuration facts, so the stable board item is derived
//! on connection and retry instead of being written into shared project files.

mod bootstrap;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SetupIssue {
    pub id: &'static str,
    pub title: &'static str,
    pub issue: String,
    pub why: &'static str,
    pub recommendation: &'static str,
    pub impact: &'static str,
    pub next_action: &'static str,
}

impl SetupIssue {
    pub fn docker(detail: impl Into<String>) -> Self {
        Self {
            id: "setup:docker",
            title: "Kool.ad/e needs Docker",
            issue: detail.into(),
            why: "Kool.ad/e uses an application-owned Docker execution session to restrict repository access, networking, and credentials.",
            recommendation: "Start Docker and make the approved Kool.ad/e runtime images available.",
            impact: "Repository planning and implementation cannot start safely until the execution environment is ready. Context-only planning remains available.",
            next_action: "Start Docker, check the runtime image readiness details, then retry this setup check.",
        }
    }

    pub fn cli_install() -> Self {
        Self {
            id: "setup:cli:install",
            title: "Install a coding agent",
            issue: "No supported coding-agent CLI was detected on this computer.".into(),
            why: "A coding agent can guide you through the remaining setup tasks, but Kool.ad/e can also provide non-AI setup instructions.".into(),
            recommendation: "Open Coding Tools to choose and install Pi, Codex, Claude Code, OpenCode, Copilot, or Antigravity.",
            impact: "Agent-guided setup and model-backed work are unavailable until a coding CLI is installed and configured.",
            next_action: "Choose a coding agent, install it, then retry the setup check.",
        }
    }

    pub fn cli_authenticate() -> Self {
        Self {
            id: "setup:cli:authenticate",
            title: "Connect your coding agent",
            issue: "A coding-agent CLI is installed, but its account or model access needs attention.".into(),
            why: "Finding a command on your machine does not prove that it can answer prompts.".into(),
            recommendation: "Open Coding Tools, inspect each agent's authentication result, and sign in using the provider's supported flow.",
            impact: "Agent-guided setup cannot start until a usable agent is independently verified.",
            next_action: "Authenticate an installed agent, then retry the setup check.",
        }
    }

    pub fn cli_verify() -> Self {
        Self {
            id: "setup:cli:verify",
            title: "Verify coding agent readiness",
            issue: "A coding agent was detected, but its authentication or model access could not be verified.".into(),
            why: "Kool.ad/e must not mistake an installed CLI or unknown authentication status for a working agent.".into(),
            recommendation: "Open Coding Tools and review the CLI, account and available-model diagnostics.",
            impact: "Bootstrap assistance stays unavailable until at least one agent is verified.",
            next_action: "Confirm your model and account configuration, then retry the setup check.",
        }
    }

    pub fn cli_select() -> Self {
        Self {
            id: "setup:cli:select",
            title: "Choose your bootstrap agent",
            issue: "At least one coding agent is ready, but none has been selected as your default.".into(),
            why: "Kool.ad/e won't silently choose a different provider or charge an unexpected account.".into(),
            recommendation: "Open Coding Tools and select your preferred verified agent as the default.",
            impact: "Your agent-assisted setup route has not been chosen yet; deterministic setup remains available.",
            next_action: "Choose your default coding agent, then retry the setup check.",
        }
    }

    pub fn provider(detail: impl Into<String>) -> Self {
        Self {
            id: "setup:provider",
            title: "Kool.ad/e cannot use this model provider safely",
            issue: detail.into(),
            why: "Provider credentials remain with the selected provider CLI on the controller; they are not exposed to execution containers.",
            recommendation: "Review the provider CLI setup and authentication diagnostics.",
            impact: "Provider-backed work cannot run until the selected CLI is configured and authenticated.",
            next_action: "Update the selected provider settings, then retry this setup check.",
        }
    }
}

pub fn detect_sandbox() -> Option<SetupIssue> {
    let capabilities = crate::harness::runtime_capabilities::RuntimeCapabilities::detect();
    if !capabilities.repository_planning_available() {
        return Some(SetupIssue::docker(
            capabilities.planning_unavailable_message(),
        ));
    }
    None
}

pub fn detect_provider() -> Option<SetupIssue> {
    let (settings, _) = crate::persistence::harness_settings::load();
    bootstrap::assess(
        &crate::app::dialogs::probe_bootstrap_clis(),
        settings.default_harness.as_deref(),
    )
}

/// CLI discovery must precede Docker: a broken sandbox cannot prevent bootstrap
/// agent detection or force a fallback to host repository execution.
pub fn detect() -> Option<SetupIssue> {
    detect_provider().or_else(detect_sandbox)
}

#[cfg(test)]
fn from_prerequisites(
    sandbox_ready: bool,
    sandbox_detail: &str,
    provider_error: Option<String>,
) -> Option<SetupIssue> {
    if !sandbox_ready {
        return Some(SetupIssue::docker(sandbox_detail));
    }
    provider_error.map(SetupIssue::provider)
}

pub fn from_harness_failure(detail: &str) -> Option<SetupIssue> {
    let lower = detail.to_ascii_lowercase();
    if lower.contains("docker")
        || lower.contains("approved runtime image")
        || lower.contains("rootful linux")
        || lower.contains("docker engine")
    {
        Some(SetupIssue::docker(detail))
    } else if lower.contains("planning provider")
        || lower.contains("provider relay")
        || lower.contains("configured local planning provider")
        || lower.contains("pi has no default model provider")
        || lower.contains("pi has no saved api key")
    {
        Some(SetupIssue::provider(detail))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn setup_issues_explain_cause_impact_and_next_action() {
        let docker = SetupIssue::docker("Docker unavailable");
        assert_eq!(docker.id, "setup:docker");
        assert!(docker.why.contains("restrict repository access"));
        assert!(docker.impact.contains("cannot start safely"));
        assert!(docker.next_action.contains("retry"));

        let provider = SetupIssue::provider("HTTPS providers are unsupported");
        assert_eq!(provider.id, "setup:provider");
        assert!(provider.recommendation.contains("provider CLI"));
        assert!(provider.impact.contains("cannot run"));
        assert!(provider.next_action.contains("provider settings"));
    }

    #[test]
    fn sandbox_failures_are_promoted_to_the_setup_item() {
        assert_eq!(
            from_harness_failure("Docker Engine is unavailable")
                .unwrap()
                .id,
            "setup:docker"
        );
        assert!(from_harness_failure("Pi returned an invalid planning response").is_none());
    }

    #[test]
    fn unsupported_provider_is_a_separate_setup_item_after_sandbox_is_ready() {
        let issue = from_prerequisites(
            true,
            "Docker unavailable",
            Some("The selected provider CLI is not authenticated".into()),
        )
        .unwrap();
        assert_eq!(issue.id, "setup:provider");
        assert!(issue.issue.contains("authenticated"));
        assert!(from_prerequisites(true, "", None).is_none());
        assert_eq!(
            from_prerequisites(false, "Docker missing", Some("provider config".into()))
                .unwrap()
                .id,
            "setup:docker",
            "the execution prerequisite must be fixed first"
        );
    }
}
