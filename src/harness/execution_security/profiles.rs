use super::CliProvider;

const CLAUDE_TASK_EXECUTION_BLOCK: &str = "Claude Code execution is unavailable in Kool.ad/e until organization-managed hooks can be contained; the CLI does not provide a supported way to disable them while keeping normal sign-in.";
const CODEX_TASK_EXECUTION_BLOCK: &str = "Codex execution is unavailable in Kool.ad/e until organization-managed hooks can be contained; feature-disable flags do not override admin-enforced hook policy.";

pub(crate) fn task_execution_block_reason(id: &str) -> Option<&'static str> {
    let id = id.trim();
    if ["claude", "claude code"]
        .iter()
        .any(|name| id.eq_ignore_ascii_case(name))
    {
        Some(CLAUDE_TASK_EXECUTION_BLOCK)
    } else if ["codex", "codex cli"]
        .iter()
        .any(|name| id.eq_ignore_ascii_case(name))
    {
        Some(CODEX_TASK_EXECUTION_BLOCK)
    } else {
        None
    }
}

pub(crate) fn profile_block_reason(
    id: &str,
    profile: crate::harness::ExecutionProfile,
) -> Option<&'static str> {
    use crate::harness::ExecutionProfile::{Manager, Worker};
    let id = canonical_provider_id(id);
    let provider = CliProvider::from_id(&id);
    match (profile, provider) {
        (Manager, Some(CliProvider::Pi)) => None,
        (Manager, Some(CliProvider::Antigravity)) => Some(
            "Antigravity is available for worker use, but not for Man.ager: Kool.ad/e cannot verify suppression of its ambient instruction and managed-policy sources.",
        ),
        (Manager, Some(CliProvider::OpenCode)) => Some(
            "OpenCode is available for worker use, but not for Man.ager until its installed CLI is verified to suppress project instructions, skills, plugins, and managed policy.",
        ),
        (Manager, Some(CliProvider::Copilot)) => Some(
            "GitHub Copilot CLI is available for worker use, but not for Man.ager until its installed CLI and managed policy are verified to suppress ambient instructions and extensions.",
        ),
        (Manager, Some(CliProvider::Codex | CliProvider::Claude)) => {
            task_execution_block_reason(&id)
        }
        (_, None) => Some("This CLI has no verified application-owned execution profile."),
        (Worker, _) => task_execution_block_reason(&id),
    }
}

/// Persisted profile IDs are provider-specific. The containing capability
/// snapshot binds this identity to the probed CLI version and auth-source class.
/// A blocked profile has no eligibility ID and can never route.
pub(crate) fn profile_evidence_id(id: &str, profile: crate::harness::ExecutionProfile) -> String {
    let id = canonical_provider_id(id);
    let Some(provider) = CliProvider::from_id(&id) else {
        return String::new();
    };
    if profile_block_reason(provider.telemetry_id(), profile).is_some() {
        return String::new();
    }
    let profile = match profile {
        crate::harness::ExecutionProfile::Manager => "manager",
        crate::harness::ExecutionProfile::Worker => "worker",
    };
    format!("{}-{profile}-cli-v1", provider.telemetry_id())
}

fn canonical_provider_id(id: &str) -> String {
    match id.trim().to_ascii_lowercase().as_str() {
        "codex cli" => "codex".into(),
        "claude code" => "claude".into(),
        "open code" => "opencode".into(),
        "copilot cli" => "copilot".into(),
        "agy" => "antigravity".into(),
        normalized => normalized.into(),
    }
}
