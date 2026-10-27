//! Recomputed local setup blockers surfaced on the project board.
//!
//! These are machine configuration facts, so the stable board item is derived
//! on connection and retry instead of being written into shared project files.

mod bootstrap;
pub(crate) mod catalog;
mod runner;
mod scan;
pub(crate) use runner::run_scan;
#[cfg(not(test))]
pub(crate) use scan::verify_before_commit;
#[cfg(test)]
pub(crate) use scan::verify_before_commit_with_state_path;
pub use scan::{SetupScanResult, SetupScanVerification, scan};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SetupIssue {
    pub id: &'static str,
    pub title: &'static str,
    pub issue: String,
    pub why: &'static str,
    pub recommendation: &'static str,
    pub impact: &'static str,
    pub next_action: &'static str,
    /// A task family that represents the same transient readiness failure.
    /// `None` keeps this notice visible beside machine-local task cards.
    pub related_task_prefix: Option<String>,
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
            related_task_prefix: None,
        }
    }

    pub fn cli_install() -> Self {
        Self {
            id: "setup:cli:install",
            title: "Install a coding agent",
            issue: "No supported coding-agent CLI was detected on this computer.".into(),
            why: "A coding agent enables AI-assisted planning and model-backed work; Kool.ad/e still provides deterministic setup instructions without one.",
            recommendation: "Choose a provider in Coding Tools, follow its official installation guide, then retry discovery.",
            impact: "AI-assisted planning and model-backed work are unavailable until a coding CLI is installed and configured. Deterministic setup guidance remains available.",
            next_action: "Choose a coding agent, follow its installation guide, then retry the setup check.",
            related_task_prefix: None,
        }
    }

    pub fn cli_authenticate() -> Self {
        Self {
            id: "setup:cli:authenticate",
            title: "Connect your coding agent",
            issue:
                "A coding-agent CLI is installed, but its account or model access needs attention."
                    .into(),
            why: "Finding a command on your machine does not prove that it can answer prompts.",
            recommendation: "Open Coding Tools, inspect each agent's authentication result, and sign in using the provider's supported flow.",
            impact: "AI-assisted planning and model-backed work stay unavailable until a usable agent is independently verified. Deterministic setup guidance remains available.",
            next_action: "Authenticate an installed agent, then retry the setup check.",
            related_task_prefix: None,
        }
    }

    pub fn cli_verify() -> Self {
        Self {
            id: "setup:cli:verify",
            title: "Verify coding agent readiness",
            issue: "A coding agent was detected, but its authentication or model access could not be verified.".into(),
            why: "Kool.ad/e must not mistake an installed CLI or unknown authentication status for a working agent.",
            recommendation: "Open Coding Tools and review the CLI, account and available-model diagnostics.",
            impact: "AI-assisted planning and model-backed work stay unavailable until an agent is verified. Deterministic setup guidance remains available.",
            next_action: "Confirm your model and account configuration, then retry the setup check.",
            related_task_prefix: None,
        }
    }

    pub fn cli_select() -> Self {
        Self {
            id: "setup:cli:select",
            title: "Choose a default coding agent",
            issue:
                "At least one coding agent is ready, but none has been selected as your default."
                    .into(),
            why: "Kool.ad/e won't silently choose a different provider or charge an unexpected account.",
            recommendation: "Open Coding Tools and set Default tool on your preferred verified provider.",
            impact: "A default conversation route has not been chosen. Deterministic setup guidance remains available.",
            next_action: "Choose your default coding agent, then retry the setup check.",
            related_task_prefix: None,
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
            related_task_prefix: None,
        }
    }

    pub fn bootstrap(detail: impl Into<String>) -> Self {
        Self {
            id: "setup:bootstrap",
            title: "Machine setup needs attention",
            issue: detail.into(),
            why: "Kool.ad/e checks the execution environment and its approved runtime images before coding tasks can start.",
            recommendation: "Review the specific machine setup task and complete its verified next step.",
            impact: "Coding tasks remain blocked while a required machine prerequisite is unresolved.",
            next_action: "Complete the listed prerequisite, then retry this setup check.",
            related_task_prefix: None,
        }
    }
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
        let mut issue = SetupIssue::docker(detail);
        issue.related_task_prefix = docker_task_prefix(&lower);
        Some(issue)
    } else if lower.contains("planning provider")
        || lower.contains("provider relay")
        || lower.contains("configured local planning provider")
        || lower.contains("pi has no default model provider")
        || lower.contains("pi has no saved api key")
    {
        let mut issue = SetupIssue::provider(detail);
        let provider = if lower.contains("pi has no default model provider")
            || lower.contains("pi has no saved api key")
        {
            Some("pi")
        } else if lower.contains("codex") {
            Some("codex")
        } else if lower.contains("claude code") {
            Some("claude")
        } else if lower.contains("opencode") {
            Some("opencode")
        } else if lower.contains("copilot") {
            Some("copilot")
        } else if lower.contains("antigravity") {
            Some("antigravity")
        } else {
            None
        };
        issue.related_task_prefix = provider.map(|provider| format!("setup:cli:{provider}:"));
        Some(issue)
    } else {
        None
    }
}

fn docker_task_prefix(lower: &str) -> Option<String> {
    let task_id = if lower.contains("rootful linux") || lower.contains("rootless docker") {
        "setup:docker:rootless"
    } else if lower.contains("docker not installed") || lower.contains("docker is not installed") {
        "setup:docker:install"
    } else if lower.contains("docker not running") || lower.contains("docker is not running") {
        "setup:docker:start"
    } else if lower.contains("permission denied") || lower.contains("docker permission") {
        "setup:docker:permission"
    } else if lower.contains("untrusted endpoint") {
        "setup:docker:endpoint"
    } else if lower.contains("docker desktop") || lower.contains("desktop backend") {
        "setup:docker:desktop"
    } else if lower.contains("capabilities incompatible") || lower.contains("resource limits") {
        "setup:docker:capabilities"
    } else if lower.contains("approved runtime image") {
        "setup:images:"
    } else {
        return None;
    };
    Some(task_id.to_owned())
}

pub(crate) fn has_specific_task(
    tasks: &[&crate::persistence::setup_tasks::LocalTask],
    issue: &SetupIssue,
) -> bool {
    let Some(prefix) = issue.related_task_prefix.as_deref() else {
        return false;
    };
    tasks.iter().any(|task| {
        task.id.starts_with(prefix)
            && task.state != crate::persistence::setup_tasks::State::Unsupported
            && task.id != "setup:docker:desktop-transport"
    })
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
    fn provider_failure_matches_only_a_provider_named_in_its_diagnostic() {
        let pi = from_harness_failure("Pi has no saved API key").unwrap();
        assert_eq!(pi.related_task_prefix.as_deref(), Some("setup:cli:pi:"));

        let generic = from_harness_failure("The planning provider relay is unavailable.").unwrap();
        assert!(generic.related_task_prefix.is_none());
    }

    #[test]
    fn generic_docker_failure_does_not_claim_a_specific_machine_task() {
        let generic = from_harness_failure("Docker Engine is unavailable").unwrap();
        assert!(generic.related_task_prefix.is_none());

        let rootful = from_harness_failure("Rootful Linux Docker is not supported").unwrap();
        assert_eq!(
            rootful.related_task_prefix.as_deref(),
            Some("setup:docker:rootless")
        );
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
