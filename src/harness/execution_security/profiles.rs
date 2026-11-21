use super::CliProvider;
use super::provider_eligibility::{
    ProviderContainmentEvidence, TaskExecutionVerdict, evaluate_task_execution,
};

const CLAUDE_TASK_EXECUTION_BLOCK: &str = "Claude Code execution is unavailable in Kool.ad/e until organization-managed hooks can be contained; the CLI does not provide a supported way to disable them while keeping normal sign-in.";

pub(crate) fn task_execution_block_reason(id: &str) -> Option<&'static str> {
    let id = id.trim();
    if ["claude", "claude code"]
        .iter()
        .any(|name| id.eq_ignore_ascii_case(name))
    {
        Some(CLAUDE_TASK_EXECUTION_BLOCK)
    } else {
        None
    }
}

/// Evidence-aware successor of [`task_execution_block_reason`].
///
/// Claude is no longer *unconditionally* blocked: when the capability
/// evidences all positively (validated platform, verified rootless engine,
/// digest-pinned contained image, recorded managed-hook containment
/// attestation), execution is ADMITTED. Until those evidences exist —
/// which is the current state of every deployed configuration — the
/// evaluation is obstructed and this function returns the SAME legacy
/// notice, so users see zero behavior change. Other providers are out of
/// scope for this evaluator and keep their existing profile treatment.
/// Detail-level obstruction text is surfaced by dedicated diagnostics,
/// not by this terse notice channel.
pub(crate) fn task_execution_block_reason_ctx(
    id: &str,
    evidence: &ProviderContainmentEvidence,
) -> Option<String> {
    let id = canonical_provider_id(id);
    match (CliProvider::from_id(&id), task_execution_block_reason(&id)) {
        (Some(provider), Some(legacy)) => match evaluate_task_execution(provider, evidence) {
            TaskExecutionVerdict::OutOfScope | TaskExecutionVerdict::Runnable => None,
            TaskExecutionVerdict::Obstructed(_all_of_them) => Some(legacy.to_string()),
        },
        // No managed-hook bearing provider, or provider not governed here.
        _ => None,
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
        (Manager, Some(CliProvider::Pi | CliProvider::Codex)) => None,
        (Manager, Some(CliProvider::Antigravity)) => Some(
            "Antigravity is available for worker use, but not for Man.ager: Kool.ad/e cannot verify suppression of its ambient instruction and managed-policy sources.",
        ),
        (Manager, Some(CliProvider::OpenCode)) => Some(
            "OpenCode is available for worker use, but not for Man.ager until its installed CLI is verified to suppress project instructions, skills, plugins, and managed policy.",
        ),
        (Manager, Some(CliProvider::Copilot)) => Some(
            "GitHub Copilot CLI is available for worker use, but not for Man.ager until its installed CLI and managed policy are verified to suppress ambient instructions and extensions.",
        ),
        (Manager, Some(CliProvider::Claude)) => {
            // Manager stays legacy-static: #451 separates strict Manager
            // readiness from Worker containment; its evidence ladder is a
            // later slice. Worker adoption flows through the evaluator
            // below.
            task_execution_block_reason(&id)
        }
        (_, None) => Some("This CLI has no verified application-owned execution profile."),
        (Worker, _) => {
            // Live evaluator on the selection path, fed by the once-
            // initialized evidence snapshot (piece 3). Today's universally
            // unprovisioned deployments gather the default-obstructed
            // context, reproducing the exact legacy notice; a deployment
            // that provisions complete positive evidence (published pin +
            // engine/attestation facts) flips this arm to None without
            // touching callers. Manager readiness stays legacy-static per
            // #451.
            let obstructed =
                task_execution_block_reason_ctx(&id, &super::provider_evidence_feed::snapshot())
                    .is_some();
            if obstructed {
                task_execution_block_reason(&id)
            } else {
                None
            }
        }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::harness::execution_security::provider_eligibility::{
        ContainedImagePin, PlatformFact, ProviderContainmentEvidence,
    };

    fn full_evidence() -> ProviderContainmentEvidence {
        ProviderContainmentEvidence {
            platform_os: PlatformFact::Linux,
            engine_verified: true,
            image_pin: Some(ContainedImagePin {
                reference: "ghcr.io/zbarno/koolade-provider-cli".into(),
                digest: format!("sha256:{}", "cd".repeat(32)),
            }),
            hook_containment_attested: true,
        }
    }

    #[test]
    fn evidence_free_facade_preserves_the_exact_legacy_notices() {
        // Zero behavior change while no deployment carries evidence: the
        // evidence-aware facade reproduces the historic block byte-for-byte.
        let none = ProviderContainmentEvidence::default();
        for (id, legacy) in [
            ("claude", CLAUDE_TASK_EXECUTION_BLOCK),
            ("claude code", CLAUDE_TASK_EXECUTION_BLOCK),
        ] {
            assert_eq!(
                task_execution_block_reason_ctx(id, &none).as_deref(),
                Some(legacy),
                "{id}"
            );
        }
        assert_eq!(task_execution_block_reason_ctx("codex", &none), None);
        assert_eq!(task_execution_block_reason_ctx("Codex CLI", &none), None);
        assert_eq!(task_execution_block_reason_ctx("pi", &none), None);
    }

    #[test]
    fn complete_capability_evidence_admits_supported_providers() {
        for id in ["codex", "claude"] {
            assert_eq!(
                task_execution_block_reason_ctx(id, &full_evidence()),
                None,
                "{id} must be admitted with complete evidence"
            );
        }
    }

    #[test]
    fn single_missing_evidence_keeps_the_block_on_the_facade() {
        let mut partial = full_evidence();
        partial.hook_containment_attested = false;
        assert_eq!(
            task_execution_block_reason_ctx("claude", &partial).as_deref(),
            Some(CLAUDE_TASK_EXECUTION_BLOCK)
        );
        assert_eq!(task_execution_block_reason_ctx("codex", &partial), None);
    }

    #[test]
    fn profile_layer_matches_the_legacy_constants_while_no_evidence_is_provisioned() {
        use crate::harness::ExecutionProfile::{Manager, Worker};
        assert_eq!(profile_block_reason("codex", Worker), None);
        assert_eq!(
            profile_block_reason("claude", Worker),
            Some(CLAUDE_TASK_EXECUTION_BLOCK)
        );
        assert_eq!(profile_block_reason("codex", Manager), None);
        assert_eq!(
            profile_block_reason("claude", Manager),
            Some(CLAUDE_TASK_EXECUTION_BLOCK)
        );
    }
}
