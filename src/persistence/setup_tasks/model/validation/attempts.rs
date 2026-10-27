use super::super::{
    AttemptState, RemediationProcessStatusV1, SetupAttemptV1, SetupTaskStoreV1, SetupTaskV1,
    VerifiedResultV1, attempts::valid_operation_id, attempts::validate_binding,
    attempts::validate_remediation_result, authorization_scope_digest,
};
use crate::persistence::setup_tasks::valid_id;

pub(super) fn validate_attempts(
    task_id: &str,
    task: &SetupTaskV1,
    store: &SetupTaskStoreV1,
    operation_ids: &mut std::collections::BTreeSet<String>,
    receipt_ids: &mut std::collections::BTreeSet<String>,
) -> anyhow::Result<()> {
    for attempt in &task.attempts {
        anyhow::ensure!(
            valid_operation_id(&attempt.operation_id),
            "invalid setup operation ID"
        );
        anyhow::ensure!(
            operation_ids.insert(attempt.operation_id.clone()),
            "duplicate setup operation ID"
        );
        anyhow::ensure!(attempt.version > 0, "invalid setup operation version");
        anyhow::ensure!(
            !attempt.requested_scopes.is_empty() && attempt.requested_scopes.len() <= 24,
            "invalid requested operation scopes"
        );
        anyhow::ensure!(
            attempt.requested_scopes.iter().all(|scope| valid_id(scope)),
            "unsafe operation scope"
        );
        anyhow::ensure!(
            attempt
                .requested_scopes
                .iter()
                .any(|scope| scope == task_id),
            "operation scope omits its task"
        );
        anyhow::ensure!(
            attempt
                .requested_scopes
                .iter()
                .collect::<std::collections::BTreeSet<_>>()
                .len()
                == attempt.requested_scopes.len(),
            "duplicate requested operation scope"
        );
        if let Some(history) = &attempt.authorization_history {
            validate_binding(history)?;
            anyhow::ensure!(
                receipt_ids.insert(history.receipt_id.clone()),
                "authorization receipt was reused"
            );
            anyhow::ensure!(
                history.scope_digest
                    == authorization_scope_digest(
                        task_id,
                        &attempt.operation_id,
                        attempt.version,
                        &attempt.requested_scopes
                    ),
                "authorization history does not match the typed operation"
            );
        }
        validate_attempt_state(attempt)?;
        if let Some(result) = &attempt.remediation_result {
            validate_remediation_result(result)?;
            if result.process_status == RemediationProcessStatusV1::InterruptedByRestart {
                anyhow::ensure!(
                    attempt.state == AttemptState::Interrupted
                        && attempt.failure
                            == Some(
                                crate::persistence::setup_tasks::FailureCode::InterruptedByRestart
                            ),
                    "restart interruption result is not attached to an interrupted attempt"
                );
            }
        }
        if let Some(receipt) = &attempt.image_preparation_receipt {
            super::super::image_receipt::validate_image_preparation_receipt(
                task_id, receipt, false,
            )?;
        }
        if let Some(result) = &attempt.verified_result {
            anyhow::ensure!(
                attempt.state == AttemptState::Verified,
                "unverified attempt retains positive evidence"
            );
            validate_result(task_id, result, store.latest_scan_generation)?;
        }
    }
    Ok(())
}

fn validate_attempt_state(attempt: &SetupAttemptV1) -> anyhow::Result<()> {
    match attempt.state {
        AttemptState::NeedsApproval => anyhow::ensure!(
            attempt.authorization_binding.is_none()
                && attempt.authorization_history.is_none()
                && attempt
                    .owner_instance_id
                    .as_deref()
                    .is_some_and(|id| uuid::Uuid::parse_str(id).is_ok()),
            "unapproved operation carries authorization metadata"
        ),
        AttemptState::InProgress | AttemptState::AwaitingVerification => {
            let binding = attempt
                .authorization_binding
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("active operation lacks authorization binding"))?;
            validate_binding(binding)?;
            anyhow::ensure!(
                attempt.authorization_history.as_ref() == Some(binding),
                "active authorization binding is not auditable"
            );
            anyhow::ensure!(
                attempt
                    .owner_instance_id
                    .as_deref()
                    .is_some_and(|id| uuid::Uuid::parse_str(id).is_ok()),
                "active operation has no valid owner identity"
            );
        }
        AttemptState::Verified
        | AttemptState::Failed
        | AttemptState::Cancelled
        | AttemptState::Paused
        | AttemptState::Interrupted => {
            anyhow::ensure!(
                attempt.authorization_binding.is_none(),
                "terminal operation retains active authorization binding"
            );
            anyhow::ensure!(
                attempt.owner_instance_id.is_none(),
                "terminal operation retains an active owner identity"
            );
        }
    }
    if attempt.state == AttemptState::Verified {
        anyhow::ensure!(
            attempt.verified_result.is_some(),
            "verified attempt lacks positive evidence"
        );
    }
    if attempt.state == AttemptState::Failed {
        anyhow::ensure!(
            attempt.failure.is_some(),
            "failed attempt lacks a redacted failure code"
        );
    }
    Ok(())
}

pub(super) fn validate_result(
    task_id: &str,
    result: &VerifiedResultV1,
    latest: u64,
) -> anyhow::Result<()> {
    anyhow::ensure!(
        expected_evidence_domain(task_id).as_deref() == Some(result.evidence_domain.as_str())
            || desktop_docker_evidence(task_id, &result.evidence_domain)
            || dynamic_general_image_evidence(task_id, &result.evidence_domain),
        "verified setup task has mismatched probe evidence"
    );
    anyhow::ensure!(
        result.scan_generation > 0 && result.scan_generation <= latest,
        "verified setup task refers to invalid scan evidence"
    );
    Ok(())
}

pub(crate) fn desktop_docker_evidence(task_id: &str, evidence_domain: &str) -> bool {
    evidence_domain == "docker:desktop"
        && matches!(
            task_id,
            "setup:docker:install"
                | "setup:docker:start"
                | "setup:docker:permission"
                | "setup:docker:windows-containers"
                | "setup:docker:api"
                | "setup:docker:os"
                | "setup:docker:architecture"
                | "setup:docker:desktop"
                | "setup:docker:endpoint"
                | "setup:docker:engine"
        )
}

pub(crate) fn expected_evidence_domain(id: &str) -> Option<String> {
    let rootless_domain = match id {
        "setup:linux:rootless:packages" => Some("host:linux:rootless_helpers"),
        "setup:linux:rootless:subids" => Some("host:linux:subordinate_ids"),
        "setup:linux:rootless:service" | "setup:docker:rootless" => Some("docker:rootless-smoke"),
        _ => None,
    };
    if let Some(domain) = rootless_domain {
        return Some(domain.to_owned());
    }
    match id.split(':').collect::<Vec<_>>().as_slice() {
        ["setup", "legacy", "docker"] => Some("docker".to_owned()),
        ["setup", "cli", "install"] => Some("cli:install".to_owned()),
        ["setup", "cli", "scan"] => Some("cli:scan".to_owned()),
        ["setup", "cli", "select"] => Some("cli:select".to_owned()),
        ["setup", "git", "install"] => Some("git:executable".to_owned()),
        ["setup", "git", "identity"] => Some("git:identity".to_owned()),
        ["setup", "git", "verify"] => Some("git".to_owned()),
        ["setup", "docker", "desktop"] => Some("docker:desktop".to_owned()),
        [
            "setup",
            "cli",
            provider,
            "auth" | "verify" | "models" | "capability",
        ] if crate::app::setup_attention::catalog::ProviderIdV1::from_probe_id(provider)
            .is_some() =>
        {
            Some(format!("cli:{provider}"))
        }
        [
            "setup",
            "docker",
            "install" | "start" | "permission" | "rootless" | "windows-containers" | "disk"
            | "limits" | "cgroups" | "seccomp" | "api" | "os" | "architecture" | "workspace"
            | "profile" | "endpoint" | "engine" | "capabilities",
        ] => Some("docker".to_owned()),
        ["setup", "images", profile, architecture]
            if approved_image_profile(profile) && matches!(*architecture, "amd64" | "arm64") =>
        {
            Some(format!("images:{profile}:{architecture}"))
        }
        ["setup", "images", profile, architecture, engine_identity]
            if approved_image_profile(profile)
                && matches!(*architecture, "amd64" | "arm64")
                && is_lower_sha256(engine_identity) =>
        {
            Some(format!("images:{profile}:{architecture}:{engine_identity}"))
        }
        _ => None,
    }
}

pub(crate) fn has_expected_evidence_domain(id: &str) -> bool {
    expected_evidence_domain(id).is_some()
        || matches!(
            id,
            "setup:docker:image-unverified"
                | "setup:docker:image-digest"
                | "setup:docker:image-architecture"
                | "setup:images:general:unverified"
        )
}

fn dynamic_general_image_evidence(task_id: &str, evidence_domain: &str) -> bool {
    matches!(
        task_id,
        "setup:docker:image-unverified"
            | "setup:docker:image-digest"
            | "setup:docker:image-architecture"
            | "setup:images:general:unverified"
    ) && matches!(
        evidence_domain,
        "images:general:amd64" | "images:general:arm64"
    )
}

fn approved_image_profile(profile: &str) -> bool {
    matches!(profile, "general" | "rust" | "node" | "python" | "go")
}

fn is_lower_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}
