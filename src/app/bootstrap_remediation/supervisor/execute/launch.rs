use crate::{
    app::bootstrap_remediation::{
        approval::{ApprovedRemediation, now_unix_ms},
        identity::HostIdentity,
        plan::RemediationPlan,
    },
    persistence::setup_tasks::{FailureCode, RemediationProcessStatusV1},
};

use super::{
    journal::RemediationJournal,
    outcome::{apply_failure, ensure_active_authorization, record_not_started},
};

pub(super) fn prepare_launch(
    journal: &mut dyn RemediationJournal,
    approved: &ApprovedRemediation,
    plan: &RemediationPlan,
    identity: &HostIdentity,
    require_current_identity: bool,
) -> anyhow::Result<u64> {
    let launched_at_unix_ms = now_unix_ms()?;
    if require_current_identity && !identity.matches_current().unwrap_or(false) {
        record_not_started(
            journal,
            approved,
            plan,
            launched_at_unix_ms,
            RemediationProcessStatusV1::NotStarted,
        )?;
        apply_failure(
            journal,
            approved,
            FailureCode::PermissionDenied,
            launched_at_unix_ms,
        )?;
        anyhow::bail!("machine or account identity changed before remediation launch");
    }
    if launched_at_unix_ms > approved.expires_at_unix_ms {
        record_not_started(
            journal,
            approved,
            plan,
            launched_at_unix_ms,
            RemediationProcessStatusV1::NotStarted,
        )?;
        apply_failure(
            journal,
            approved,
            FailureCode::PermissionDenied,
            launched_at_unix_ms,
        )?;
        anyhow::bail!("remediation approval expired before process launch");
    }
    ensure_active_authorization(journal, approved, plan)?;
    Ok(launched_at_unix_ms)
}
