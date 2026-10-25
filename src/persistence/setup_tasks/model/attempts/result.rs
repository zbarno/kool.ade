use serde::{Deserialize, Serialize};

use super::super::{AttemptState, SetupTaskStoreV1};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RemediationProcessStatusV1 {
    NotStarted,
    Succeeded,
    Failed,
    InterruptedAfterLaunch,
    UnknownAfterLaunch,
    InterruptedByRestart,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RemediationVerificationStatusV1 {
    Ready,
    NotReady,
    Unavailable,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct RemediationVerificationV1 {
    pub status: RemediationVerificationStatusV1,
    pub probe_ids: Vec<String>,
    pub observed_at_unix_ms: u64,
    pub evidence_digest: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct RemediationAttemptResultV1 {
    pub process_status: RemediationProcessStatusV1,
    pub exit_code: Option<i32>,
    /// Bounded, sanitized diagnostics only. Credentials and raw host data are excluded.
    pub sanitized_diagnostic: Option<String>,
    pub verification: Option<RemediationVerificationV1>,
    pub rollback_guidance: String,
}

impl SetupTaskStoreV1 {
    pub(crate) fn record_remediation_result(
        &mut self,
        task_id: &str,
        operation_id: &str,
        result: RemediationAttemptResultV1,
        owner_instance_id: &str,
        at_unix_ms: u64,
    ) -> anyhow::Result<()> {
        validate_remediation_result(&result)?;
        let task = self
            .tasks
            .get_mut(task_id)
            .ok_or_else(|| anyhow::anyhow!("setup task does not exist"))?;
        let attempt = task
            .attempts
            .iter_mut()
            .find(|attempt| attempt.operation_id == operation_id)
            .ok_or_else(|| anyhow::anyhow!("setup operation does not exist"))?;
        anyhow::ensure!(
            attempt.state == AttemptState::InProgress
                && attempt.owner_instance_id.as_deref() == Some(owner_instance_id),
            "remediation result belongs to an inactive attempt"
        );
        attempt.remediation_result = Some(result);
        attempt.updated_at_unix_ms = at_unix_ms;
        Ok(())
    }
}

pub(crate) fn validate_remediation_result(
    result: &RemediationAttemptResultV1,
) -> anyhow::Result<()> {
    anyhow::ensure!(
        result.rollback_guidance.len() <= 512
            && result
                .rollback_guidance
                .chars()
                .all(|character| !character.is_control()),
        "invalid remediation recovery guidance"
    );
    if let Some(diagnostic) = &result.sanitized_diagnostic {
        anyhow::ensure!(
            diagnostic.len() <= 4096
                && diagnostic.chars().all(|character| !character.is_control()
                    || character == '\n'
                    || character == '\t'),
            "invalid remediation diagnostic"
        );
    }
    if let Some(receipt) = &result.verification {
        anyhow::ensure!(
            !receipt.probe_ids.is_empty() && receipt.probe_ids.len() <= 24,
            "invalid remediation verification probes"
        );
        let unique = receipt
            .probe_ids
            .iter()
            .collect::<std::collections::BTreeSet<_>>();
        anyhow::ensure!(
            unique.len() == receipt.probe_ids.len(),
            "duplicate verification probe"
        );
        anyhow::ensure!(
            receipt.probe_ids.iter().all(|probe| {
                !probe.is_empty()
                    && probe.len() <= 180
                    && probe.bytes().all(|byte| {
                        byte.is_ascii_alphanumeric() || matches!(byte, b':' | b'_' | b'-')
                    })
            }),
            "invalid remediation verification probe ID"
        );
        if let Some(digest) = &receipt.evidence_digest {
            anyhow::ensure!(
                digest.len() == 64 && digest.bytes().all(|byte| byte.is_ascii_hexdigit()),
                "invalid remediation verification digest"
            );
        }
        anyhow::ensure!(
            (receipt.status == RemediationVerificationStatusV1::Unavailable)
                == receipt.evidence_digest.is_none(),
            "verification status and evidence digest disagree"
        );
    }
    let recovery_interruption =
        result.process_status == RemediationProcessStatusV1::InterruptedByRestart;
    anyhow::ensure!(
        result.verification.is_none()
            == (result.process_status == RemediationProcessStatusV1::NotStarted
                || recovery_interruption),
        "launched remediation lacks its fresh verification outcome"
    );
    anyhow::ensure!(
        result.exit_code.is_some()
            == matches!(
                result.process_status,
                RemediationProcessStatusV1::Succeeded | RemediationProcessStatusV1::Failed
            ),
        "process exit code does not match the recorded execution state"
    );
    Ok(())
}
