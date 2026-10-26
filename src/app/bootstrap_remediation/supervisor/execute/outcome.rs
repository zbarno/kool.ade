use crate::{
    app::bootstrap_remediation::{
        approval::ApprovedRemediation,
        plan::RemediationPlan,
        supervisor::{ProcessOutput, ReadinessVerifier, RemediationResult},
    },
    persistence::setup_tasks::{
        AttemptState, FailureCode, RemediationAttemptResultV1, RemediationProcessStatusV1,
        RemediationVerificationStatusV1, SetupTaskStoreV1, SetupTaskTransitionV1,
    },
};

use super::{journal::RemediationJournal, verification::check_verification};

#[allow(clippy::too_many_arguments)]
pub(super) fn finish_launched(
    journal: &mut dyn RemediationJournal,
    approved: &ApprovedRemediation,
    plan: &RemediationPlan,
    verifier: &dyn ReadinessVerifier,
    process_status: RemediationProcessStatusV1,
    exit_code: Option<i32>,
    diagnostic: Option<String>,
    output: Option<ProcessOutput>,
    launched_at_unix_ms: u64,
    now: u64,
) -> anyhow::Result<RemediationResult> {
    let check = check_verification(verifier, plan, launched_at_unix_ms, now);
    let result = RemediationAttemptResultV1 {
        process_status,
        exit_code,
        sanitized_diagnostic: diagnostic,
        verification: Some(check.persisted.clone()),
        rollback_guidance: plan.rollback.description().to_owned(),
    };
    journal.apply(SetupTaskTransitionV1::RecordRemediationResult {
        task_id: approved.task_id.clone(),
        operation_id: approved.operation_id.clone(),
        result,
        at_unix_ms: check.persisted.observed_at_unix_ms,
    })?;
    if check.persisted.status == RemediationVerificationStatusV1::Ready {
        journal.apply(SetupTaskTransitionV1::AwaitVerification {
            task_id: approved.task_id.clone(),
            operation_id: approved.operation_id.clone(),
            at_unix_ms: check.persisted.observed_at_unix_ms,
        })?;
        if let (Some(output), Some(receipt)) = (output, check.receipt) {
            return Ok(RemediationResult {
                exit_code: output.exit_code,
                stdout: output.stdout,
                stderr: output.stderr,
                stdout_truncated: output.stdout_truncated,
                stderr_truncated: output.stderr_truncated,
                verification: receipt,
            });
        }
        anyhow::bail!(
            "remediation ended after launch; fresh probes are ready and await setup verification"
        )
    }

    let failure = if check.persisted.status == RemediationVerificationStatusV1::Unavailable
        || process_status != RemediationProcessStatusV1::Succeeded
    {
        FailureCode::Unknown
    } else {
        FailureCode::VerificationFailed
    };
    journal.apply(SetupTaskTransitionV1::FailAttempt {
        task_id: approved.task_id.clone(),
        operation_id: approved.operation_id.clone(),
        failure,
        at_unix_ms: check.persisted.observed_at_unix_ms,
    })?;
    anyhow::bail!(
        "remediation result and fresh readiness outcome were recorded; recovery guidance: {}",
        plan.rollback.description()
    )
}

pub(super) fn record_not_started(
    journal: &mut dyn RemediationJournal,
    approved: &ApprovedRemediation,
    plan: &RemediationPlan,
    at_unix_ms: u64,
    process_status: RemediationProcessStatusV1,
) -> anyhow::Result<()> {
    journal.apply(SetupTaskTransitionV1::RecordRemediationResult {
        task_id: approved.task_id.clone(),
        operation_id: approved.operation_id.clone(),
        result: RemediationAttemptResultV1 {
            process_status,
            exit_code: None,
            sanitized_diagnostic: None,
            verification: None,
            rollback_guidance: plan.rollback.description().to_owned(),
        },
        at_unix_ms,
    })?;
    Ok(())
}

pub(super) fn apply_failure(
    journal: &mut dyn RemediationJournal,
    approved: &ApprovedRemediation,
    failure: FailureCode,
    at_unix_ms: u64,
) -> anyhow::Result<()> {
    journal.apply(SetupTaskTransitionV1::FailAttempt {
        task_id: approved.task_id.clone(),
        operation_id: approved.operation_id.clone(),
        failure,
        at_unix_ms,
    })?;
    Ok(())
}

pub(super) fn apply_cancel(
    journal: &mut dyn RemediationJournal,
    approved: &ApprovedRemediation,
    at_unix_ms: u64,
) -> anyhow::Result<()> {
    journal.apply(SetupTaskTransitionV1::CancelAttempt {
        task_id: approved.task_id.clone(),
        operation_id: approved.operation_id.clone(),
        at_unix_ms,
    })?;
    Ok(())
}

pub(super) fn ensure_active_authorization(
    journal: &mut dyn RemediationJournal,
    approved: &ApprovedRemediation,
    plan: &RemediationPlan,
) -> anyhow::Result<()> {
    let store = journal.load()?;
    let active = active_attempt(&store, approved)?;
    anyhow::ensure!(
        active.version == plan.operation_code_version
            && active
                .authorization_binding
                .as_ref()
                .is_some_and(|binding| {
                    binding.receipt_id == approved.receipt_id
                        && binding.scope_digest == approved.scope_digest
                }),
        "persisted approval does not match the remediation capability"
    );
    Ok(())
}

pub(super) fn active_attempt<'a>(
    store: &'a SetupTaskStoreV1,
    approved: &ApprovedRemediation,
) -> anyhow::Result<&'a crate::persistence::setup_tasks::SetupAttemptV1> {
    let task = store
        .tasks
        .get(&approved.task_id)
        .ok_or_else(|| anyhow::anyhow!("setup task for remediation no longer exists"))?;
    let attempt = task
        .attempts
        .iter()
        .find(|attempt| attempt.operation_id == approved.operation_id)
        .ok_or_else(|| anyhow::anyhow!("setup attempt for remediation no longer exists"))?;
    anyhow::ensure!(
        attempt.state == AttemptState::InProgress
            && attempt.owner_instance_id.as_deref() == Some(approved.owner_instance_id.as_str()),
        "setup attempt is no longer owned by this remediation controller"
    );
    Ok(attempt)
}
