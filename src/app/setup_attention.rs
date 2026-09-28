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
    pub fn bubblewrap(detail: impl Into<String>) -> Self {
        Self {
            id: "setup:bubblewrap",
            title: "Packet needs Bubblewrap",
            issue: detail.into(),
            why: "Packet uses Bubblewrap to prevent agents from accessing unrelated files or the host network.",
            recommendation: "Install Bubblewrap and enable Linux user namespaces.",
            impact: "Planning, repository investigation, and implementation cannot start safely until the sandbox works.",
            next_action: "Install the bubblewrap package, check user namespace support, then retry this setup check.",
        }
    }

    pub fn provider(detail: impl Into<String>) -> Self {
        Self {
            id: "setup:provider",
            title: "Packet cannot use this model provider safely",
            issue: detail.into(),
            why: "Packet relays provider requests so Pi never receives the provider credential or host network access.",
            recommendation: "Configure Pi to use a private/local HTTP OpenAI-compatible provider.",
            impact: "Planning, task generation, and investigation cannot run until the provider configuration passes the relay checks.",
            next_action: "Update Pi's default provider settings and saved key, then retry this setup check.",
        }
    }
}

pub fn detect_sandbox() -> Option<SetupIssue> {
    let capabilities = crate::harness::runtime_capabilities::RuntimeCapabilities::detect();
    if !capabilities.repository_planning_available() {
        return Some(SetupIssue::bubblewrap(
            capabilities.planning_unavailable_message(),
        ));
    }
    None
}

pub fn detect_provider() -> Option<SetupIssue> {
    crate::harness::pi_sandbox::provider_configuration_error().map(SetupIssue::provider)
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
        return Some(SetupIssue::bubblewrap(sandbox_detail));
    }
    provider_error.map(SetupIssue::provider)
}

pub fn from_harness_failure(detail: &str) -> Option<SetupIssue> {
    let lower = detail.to_ascii_lowercase();
    if lower.contains("bubblewrap")
        || lower.contains("bwrap:")
        || lower.contains("user namespace")
        || lower.contains("unshare-user")
    {
        Some(SetupIssue::bubblewrap(detail))
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
        let bubblewrap = SetupIssue::bubblewrap("bwrap unavailable");
        assert_eq!(bubblewrap.id, "setup:bubblewrap");
        assert!(bubblewrap.why.contains("unrelated files"));
        assert!(bubblewrap.impact.contains("cannot start safely"));
        assert!(bubblewrap.next_action.contains("retry"));

        let provider = SetupIssue::provider("HTTPS providers are unsupported");
        assert_eq!(provider.id, "setup:provider");
        assert!(provider.recommendation.contains("OpenAI-compatible"));
        assert!(provider.impact.contains("cannot run"));
        assert!(provider.next_action.contains("saved key"));
    }

    #[test]
    fn sandbox_failures_are_promoted_to_the_setup_item() {
        assert_eq!(
            from_harness_failure("bwrap: Creating new namespace: Operation not permitted")
                .unwrap()
                .id,
            "setup:bubblewrap"
        );
        assert!(from_harness_failure("Pi returned an invalid planning response").is_none());
    }

    #[test]
    fn unsupported_provider_is_a_separate_setup_item_after_sandbox_is_ready() {
        let issue = from_prerequisites(
            true,
            "Bubblewrap unavailable",
            Some("Only private/local HTTP OpenAI-compatible providers are supported".into()),
        )
        .unwrap();
        assert_eq!(issue.id, "setup:provider");
        assert!(issue.issue.contains("OpenAI-compatible"));
        assert!(from_prerequisites(true, "", None).is_none());
        assert_eq!(
            from_prerequisites(false, "Bubblewrap missing", Some("provider config".into()))
                .unwrap()
                .id,
            "setup:bubblewrap",
            "the sandbox prerequisite must be fixed first"
        );
    }
}
