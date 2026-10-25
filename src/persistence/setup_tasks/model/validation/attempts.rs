use super::super::{
    AttemptState, SetupAttemptV1, SetupTaskStoreV1, SetupTaskV1, VerifiedResultV1,
    attempts::valid_operation_id, attempts::validate_binding, authorization_scope_digest,
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
        expected_evidence_domain(task_id).as_deref() == Some(result.evidence_domain.as_str()),
        "verified setup task has mismatched probe evidence"
    );
    anyhow::ensure!(
        result.scan_generation > 0 && result.scan_generation <= latest,
        "verified setup task refers to invalid scan evidence"
    );
    Ok(())
}

pub(crate) fn expected_evidence_domain(id: &str) -> Option<String> {
    match id.split(':').collect::<Vec<_>>().as_slice() {
        ["setup", "legacy", "docker"] => Some("docker".to_owned()),
        ["setup", "cli", "install"] => Some("cli:install".to_owned()),
        ["setup", "cli", "scan"] => Some("cli:scan".to_owned()),
        ["setup", "cli", "select"] => Some("cli:select".to_owned()),
        ["setup", "cli", provider, "auth" | "verify"] => Some(format!("cli:{provider}:auth")),
        ["setup", "cli", provider, "models"] => Some(format!("cli:{provider}:models")),
        ["setup", "cli", provider, "capability"] => Some(format!("cli:{provider}:capability")),
        ["setup", "docker", "rootless"] => Some("docker:rootless".to_owned()),
        ["setup", "docker", "engine"] => Some("docker:engine".to_owned()),
        ["setup", "docker", "capabilities"] => Some("docker:capabilities".to_owned()),
        ["setup", "docker", "desktop-transport"] => Some("docker:desktop-transport".to_owned()),
        [
            "setup",
            "docker",
            "install" | "start" | "permission" | "desktop" | "endpoint",
        ] => Some("docker:engine".to_owned()),
        ["setup", "images", profile, architecture] => {
            Some(format!("images:{profile}:{architecture}"))
        }
        _ => None,
    }
}
