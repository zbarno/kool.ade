//! Recomputed local setup blockers surfaced on the project board.
//!
//! These are machine configuration facts, so the stable board item is derived
//! on connection and retry instead of being written into shared project files.

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
    None
}

#[cfg(test)]
pub fn detect() -> Option<SetupIssue> {
    detect_sandbox().or_else(detect_provider)
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
