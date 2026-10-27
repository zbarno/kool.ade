use std::time::SystemTime;

use crate::app::bootstrap_remediation::{
    plan::RemediationPlan,
    supervisor::{PreconditionReceipt, PreconditionVerifier},
};

const MAX_PRECONDITION_AGE_MS: u64 = 5 * 60 * 1000;

pub(super) fn check_preconditions(
    verifier: &dyn PreconditionVerifier,
    plan: &RemediationPlan,
) -> anyhow::Result<()> {
    let receipt = verifier
        .verify(&plan.precondition_probe_ids)
        .map_err(|error| anyhow::anyhow!("remediation precondition probes failed: {error:#}"))?;
    validate_receipt(&plan.precondition_probe_ids, &receipt, current_unix_ms()?)
}

fn validate_receipt(
    required_probe_ids: &[String],
    receipt: &PreconditionReceipt,
    now_unix_ms: u64,
) -> anyhow::Result<()> {
    anyhow::ensure!(
        receipt.probe_ids == required_probe_ids,
        "remediation precondition probe set is missing or changed"
    );
    anyhow::ensure!(receipt.ready, "remediation preconditions are not ready");
    anyhow::ensure!(
        receipt.observed_at_unix_ms <= now_unix_ms
            && now_unix_ms - receipt.observed_at_unix_ms <= MAX_PRECONDITION_AGE_MS,
        "remediation precondition evidence is stale or from the future"
    );
    anyhow::ensure!(
        receipt.evidence_digest.len() == 64
            && receipt
                .evidence_digest
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit()),
        "remediation precondition receipt is invalid"
    );
    Ok(())
}

fn current_unix_ms() -> anyhow::Result<u64> {
    Ok(SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)?
        .as_millis() as u64)
}
