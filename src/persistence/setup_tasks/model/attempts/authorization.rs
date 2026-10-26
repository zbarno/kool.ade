use super::super::*;
use crate::persistence::setup_tasks::{MAX_ATTEMPTS, valid_id};

impl SetupTaskStoreV1 {
    pub(crate) fn request_attempt(
        &mut self,
        task_id: &str,
        operation_id: String,
        version: u16,
        requested_scopes: Vec<String>,
        owner_instance_id: &str,
        at_unix_ms: u64,
    ) -> anyhow::Result<()> {
        anyhow::ensure!(
            !self
                .tasks
                .values()
                .flat_map(|item| &item.attempts)
                .any(|attempt| attempt.operation_id == operation_id),
            "duplicate setup operation ID"
        );
        let task = self
            .tasks
            .get_mut(task_id)
            .ok_or_else(|| anyhow::anyhow!("setup task does not exist"))?;
        anyhow::ensure!(
            matches!(
                task.state,
                SetupTaskStateV1::ReadyToFix
                    | SetupTaskStateV1::Paused
                    | SetupTaskStateV1::Interrupted
                    | SetupTaskStateV1::Failed
                    | SetupTaskStateV1::Cancelled
            ),
            "setup task is not ready for an operation"
        );
        anyhow::ensure!(
            self.latest_scan_generation > 0,
            "setup task needs a fresh readiness scan"
        );
        anyhow::ensure!(
            task.scan_generation == self.latest_scan_generation,
            "setup task needs a fresh readiness scan"
        );
        anyhow::ensure!(
            task.blocked_by.is_empty(),
            "setup task has unresolved prerequisites"
        );
        anyhow::ensure!(
            version > 0 && valid_operation_id(&operation_id),
            "invalid typed operation identity"
        );
        anyhow::ensure!(
            uuid::Uuid::parse_str(owner_instance_id).is_ok(),
            "invalid setup owner identity"
        );
        anyhow::ensure!(
            !requested_scopes.is_empty() && requested_scopes.len() <= 24,
            "invalid requested operation scopes"
        );
        anyhow::ensure!(
            requested_scopes.iter().any(|scope| scope == task_id),
            "operation scope must include the task prerequisite"
        );
        anyhow::ensure!(
            requested_scopes.iter().all(|scope| valid_id(scope)),
            "unsafe operation scope"
        );
        let unique_scopes = requested_scopes
            .iter()
            .collect::<std::collections::BTreeSet<_>>();
        anyhow::ensure!(
            unique_scopes.len() == requested_scopes.len(),
            "duplicate requested operation scope"
        );
        task.attempts.push(SetupAttemptV1 {
            operation_id,
            version,
            owner_instance_id: Some(owner_instance_id.to_owned()),
            requested_scopes,
            authorization_binding: None,
            authorization_history: None,
            state: AttemptState::NeedsApproval,
            started_at_unix_ms: at_unix_ms,
            updated_at_unix_ms: at_unix_ms,
            verified_result: None,
            remediation_result: None,
            image_preparation_receipt: None,
            failure: None,
        });
        if task.attempts.len() > MAX_ATTEMPTS {
            task.attempts.drain(..task.attempts.len() - MAX_ATTEMPTS);
        }
        task.state = SetupTaskStateV1::NeedsApproval;
        record(task, at_unix_ms, self.latest_scan_generation);
        Ok(())
    }

    /// Record an externally validated consent receipt as non-authoritative
    /// bookkeeping. Callers must verify its authority before proceeding; this
    /// persistence layer cannot mint consent or execute a host operation.
    pub(crate) fn authorize_attempt(
        &mut self,
        task_id: &str,
        operation_id: &str,
        binding: AuthorizationBindingV1,
        owner_instance_id: &str,
        at_unix_ms: u64,
    ) -> anyhow::Result<()> {
        validate_binding(&binding)?;
        anyhow::ensure!(
            !self
                .tasks
                .values()
                .flat_map(|task| &task.attempts)
                .any(|attempt| {
                    attempt
                        .authorization_history
                        .as_ref()
                        .is_some_and(|history| history.receipt_id == binding.receipt_id)
                }),
            "authorization receipt was already used"
        );
        let task = self
            .tasks
            .get_mut(task_id)
            .ok_or_else(|| anyhow::anyhow!("setup task does not exist"))?;
        anyhow::ensure!(
            task.state == SetupTaskStateV1::NeedsApproval,
            "setup task is not awaiting approval"
        );
        anyhow::ensure!(
            task.scan_generation == self.latest_scan_generation,
            "setup approval needs a fresh readiness scan"
        );
        anyhow::ensure!(
            task.blocked_by.is_empty(),
            "setup task has unresolved prerequisites"
        );
        let attempt = task
            .attempts
            .iter_mut()
            .find(|attempt| attempt.operation_id == operation_id)
            .ok_or_else(|| anyhow::anyhow!("setup operation does not exist"))?;
        anyhow::ensure!(
            attempt.owner_instance_id.as_deref() == Some(owner_instance_id),
            "setup operation belongs to another app instance"
        );
        anyhow::ensure!(
            attempt.state == AttemptState::NeedsApproval,
            "setup operation is not awaiting approval"
        );
        anyhow::ensure!(
            binding.scope_digest
                == authorization_scope_digest(
                    task_id,
                    operation_id,
                    attempt.version,
                    &attempt.requested_scopes
                ),
            "authorization does not match the requested setup scopes"
        );
        attempt.authorization_binding = Some(binding.clone());
        attempt.authorization_history = Some(binding);
        attempt.state = AttemptState::InProgress;
        attempt.updated_at_unix_ms = at_unix_ms;
        task.state = SetupTaskStateV1::InProgress;
        record(task, at_unix_ms, self.latest_scan_generation);
        Ok(())
    }
}

pub(crate) fn valid_operation_id(value: &str) -> bool {
    value.len() == 36
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() || byte == b'-')
}

pub(crate) fn validate_binding(binding: &AuthorizationBindingV1) -> anyhow::Result<()> {
    anyhow::ensure!(
        valid_operation_id(&binding.receipt_id),
        "invalid authorization receipt ID"
    );
    anyhow::ensure!(
        binding.scope_digest.len() == 64
            && binding.scope_digest.bytes().all(|b| b.is_ascii_hexdigit()),
        "invalid authorization scope digest"
    );
    Ok(())
}
