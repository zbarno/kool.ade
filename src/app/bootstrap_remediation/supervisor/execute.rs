#[cfg(test)]
use crate::persistence::setup_tasks::SetupTaskStoreV1;
use crate::{
    app::bootstrap_remediation::{
        approval::{ApprovedRemediation, now_unix_ms},
        identity::HostIdentity,
        plan::{PrivilegeLevel, RemediationPlan},
        supervisor::{
            Cancellation, NativeCommandRunner, NativeElevation, OperationRegistry,
            ReadinessVerifier, RemediationResult,
        },
    },
    persistence::setup_tasks::{FailureCode, RemediationProcessStatusV1},
};

mod journal;
mod launch;
mod outcome;
mod verification;

#[cfg(test)]
use journal::InMemoryJournal;
use journal::{DurableJournal, RemediationJournal};
use launch::prepare_launch;
use outcome::{
    apply_cancel, apply_failure, ensure_active_authorization, finish_launched, record_not_started,
};
use verification::sanitize_diagnostic;

use super::ProcessRunFailure;

pub(crate) struct RemediationSupervisor;

impl RemediationSupervisor {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn execute(
        approved: ApprovedRemediation,
        plan: &RemediationPlan,
        registry: &dyn OperationRegistry,
        runner: &dyn NativeCommandRunner,
        elevation: &dyn NativeElevation,
        verifier: &dyn ReadinessVerifier,
        cancellation: &Cancellation,
    ) -> anyhow::Result<RemediationResult> {
        plan.validate_current_target()?;
        let identity = HostIdentity::current()?;
        let mut journal = DurableJournal;
        Self::execute_with_journal(
            approved,
            plan,
            &identity,
            &mut journal,
            registry,
            runner,
            elevation,
            verifier,
            cancellation,
            now_unix_ms()?,
            true,
        )
    }

    #[cfg(test)]
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn execute_for_test(
        approved: ApprovedRemediation,
        plan: &RemediationPlan,
        identity: &HostIdentity,
        store: &mut SetupTaskStoreV1,
        registry: &dyn OperationRegistry,
        runner: &dyn NativeCommandRunner,
        elevation: &dyn NativeElevation,
        verifier: &dyn ReadinessVerifier,
        cancellation: &Cancellation,
        now_unix_ms: u64,
    ) -> anyhow::Result<RemediationResult> {
        let owner_instance_id = approved.owner_instance_id.clone();
        let mut journal = InMemoryJournal::new(store, &owner_instance_id);
        Self::execute_with_journal(
            approved,
            plan,
            identity,
            &mut journal,
            registry,
            runner,
            elevation,
            verifier,
            cancellation,
            now_unix_ms,
            false,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn execute_with_journal(
        approved: ApprovedRemediation,
        plan: &RemediationPlan,
        identity: &HostIdentity,
        journal: &mut dyn RemediationJournal,
        registry: &dyn OperationRegistry,
        runner: &dyn NativeCommandRunner,
        elevation: &dyn NativeElevation,
        verifier: &dyn ReadinessVerifier,
        cancellation: &Cancellation,
        now: u64,
        require_current_identity: bool,
    ) -> anyhow::Result<RemediationResult> {
        plan.validate()?;
        identity.validate()?;
        anyhow::ensure!(
            plan.host == identity.host() && plan.os_version == identity.os_version(),
            "remediation plan targets another host OS"
        );
        if require_current_identity {
            anyhow::ensure!(
                identity.matches_current()?,
                "host identity changed before remediation launch"
            );
        }
        anyhow::ensure!(
            plan.requires_consent,
            "remediation operation is not approved for execution"
        );
        anyhow::ensure!(
            approved.matches(plan, identity)?,
            "approved plan or host changed"
        );
        anyhow::ensure!(
            now <= approved.expires_at_unix_ms,
            "remediation approval expired before execution"
        );
        ensure_active_authorization(journal, &approved, plan)?;
        if cancellation.is_cancelled() {
            record_not_started(
                journal,
                &approved,
                plan,
                now,
                RemediationProcessStatusV1::NotStarted,
            )?;
            apply_cancel(journal, &approved, now)?;
            anyhow::bail!("remediation was cancelled before launch");
        }

        let invocation = match registry.compile(plan) {
            Ok(invocation) => invocation,
            Err(error) => {
                record_not_started(
                    journal,
                    &approved,
                    plan,
                    now,
                    RemediationProcessStatusV1::NotStarted,
                )?;
                apply_failure(journal, &approved, FailureCode::OperationFailed, now)?;
                return Err(error);
            }
        };
        anyhow::ensure!(
            invocation.host() == plan.host,
            "operation registry returned a command for another host"
        );

        let elevation_receipt = if plan.privilege == PrivilegeLevel::Administrator {
            match elevation.request(plan, identity, &invocation, cancellation) {
                Ok(Some(receipt)) if receipt.matches(plan, identity, &invocation)? => Some(receipt),
                Ok(Some(_)) => {
                    record_not_started(
                        journal,
                        &approved,
                        plan,
                        now,
                        RemediationProcessStatusV1::NotStarted,
                    )?;
                    apply_failure(journal, &approved, FailureCode::PermissionDenied, now)?;
                    anyhow::bail!("native elevation receipt did not match the approved invocation");
                }
                Ok(None) => {
                    record_not_started(
                        journal,
                        &approved,
                        plan,
                        now,
                        RemediationProcessStatusV1::NotStarted,
                    )?;
                    apply_cancel(journal, &approved, now)?;
                    anyhow::bail!("OS-native elevation was declined; no remediation was run");
                }
                Err(error) => {
                    record_not_started(
                        journal,
                        &approved,
                        plan,
                        now,
                        RemediationProcessStatusV1::NotStarted,
                    )?;
                    apply_failure(journal, &approved, FailureCode::PermissionDenied, now)?;
                    return Err(error);
                }
            }
        } else {
            None
        };
        if cancellation.is_cancelled() {
            record_not_started(
                journal,
                &approved,
                plan,
                now,
                RemediationProcessStatusV1::NotStarted,
            )?;
            apply_cancel(journal, &approved, now)?;
            anyhow::bail!("remediation was cancelled before launch");
        }

        let launched_at_unix_ms =
            prepare_launch(journal, &approved, plan, identity, require_current_identity)?;
        let process_result = match elevation_receipt {
            Some(receipt) => runner.run_elevated(invocation, receipt, cancellation),
            None => runner.run(invocation, cancellation),
        };
        match process_result {
            Ok(output) => {
                let status = if output.exit_code == 0 {
                    RemediationProcessStatusV1::Succeeded
                } else {
                    RemediationProcessStatusV1::Failed
                };
                finish_launched(
                    journal,
                    &approved,
                    plan,
                    verifier,
                    status,
                    Some(output.exit_code),
                    sanitize_diagnostic(&output.stderr),
                    Some(output),
                    launched_at_unix_ms,
                    now,
                )
            }
            Err(error) if !error.launched => {
                record_not_started(
                    journal,
                    &approved,
                    plan,
                    now,
                    RemediationProcessStatusV1::NotStarted,
                )?;
                if error.failure == ProcessRunFailure::Cancelled {
                    apply_cancel(journal, &approved, now)?;
                    anyhow::bail!("remediation was cancelled before launch");
                }
                apply_failure(journal, &approved, FailureCode::OperationFailed, now)?;
                Err(error.into())
            }
            Err(error) => {
                let status = if error.failure == ProcessRunFailure::Cancelled
                    || error.failure == ProcessRunFailure::TimedOut
                {
                    RemediationProcessStatusV1::InterruptedAfterLaunch
                } else {
                    RemediationProcessStatusV1::UnknownAfterLaunch
                };
                finish_launched(
                    journal,
                    &approved,
                    plan,
                    verifier,
                    status,
                    None,
                    None,
                    None,
                    launched_at_unix_ms,
                    now,
                )?;
                anyhow::bail!("{error}; fresh readiness probes were recorded")
            }
        }
    }
}
