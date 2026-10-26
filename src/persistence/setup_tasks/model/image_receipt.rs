use serde::{Deserialize, Serialize};

use crate::execution::docker::images::{ImageLock, RuntimeProfile};

use super::{AttemptState, SetupAttemptV1, SetupTaskStoreV1};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ImagePreparationReceiptV1 {
    pub profile: RuntimeProfile,
    pub architecture: String,
    pub image_digest: String,
    pub engine_identity_fingerprint: String,
    pub successful_smoke_at_unix_ms: u64,
}

#[derive(Clone, Copy)]
pub(super) struct ImageTaskIdentity<'a> {
    pub(super) profile: RuntimeProfile,
    pub(super) architecture: &'a str,
    pub(super) engine_identity_fingerprint: &'a str,
}

impl SetupTaskStoreV1 {
    pub fn has_valid_image_preparation_receipt(&self, task_id: &str) -> bool {
        let Some(task) = self.tasks.get(task_id) else {
            return false;
        };
        has_valid_image_preparation_receipt_for_task(task_id, &task.attempts)
    }

    pub(crate) fn record_image_preparation_receipt(
        &mut self,
        task_id: &str,
        operation_id: &str,
        owner_instance_id: &str,
        receipt: ImagePreparationReceiptV1,
        at_unix_ms: u64,
    ) -> anyhow::Result<()> {
        validate_image_preparation_receipt(task_id, &receipt, true)?;
        anyhow::ensure!(
            receipt.successful_smoke_at_unix_ms == at_unix_ms,
            "image smoke receipt time does not match its persistence time"
        );
        let task = self
            .tasks
            .get_mut(task_id)
            .ok_or_else(|| anyhow::anyhow!("image setup task does not exist"))?;
        let attempt = task
            .attempts
            .iter_mut()
            .find(|attempt| attempt.operation_id == operation_id)
            .ok_or_else(|| anyhow::anyhow!("image setup operation does not exist"))?;
        anyhow::ensure!(
            attempt.state == AttemptState::InProgress
                && attempt.owner_instance_id.as_deref() == Some(owner_instance_id),
            "image smoke receipt belongs to an inactive attempt"
        );
        attempt.image_preparation_receipt = Some(receipt);
        attempt.updated_at_unix_ms = at_unix_ms;
        Ok(())
    }
}

pub(super) fn has_valid_image_preparation_receipt_for_task(
    task_id: &str,
    attempts: &[SetupAttemptV1],
) -> bool {
    attempts.iter().any(|attempt| {
        matches!(
            attempt.state,
            AttemptState::AwaitingVerification | AttemptState::Verified
        ) && attempt
            .image_preparation_receipt
            .as_ref()
            .is_some_and(|receipt| {
                validate_image_preparation_receipt(task_id, receipt, true).is_ok()
            })
    })
}

pub(super) fn validate_image_preparation_receipt(
    task_id: &str,
    receipt: &ImagePreparationReceiptV1,
    require_current_lock: bool,
) -> anyhow::Result<()> {
    let identity = image_task_identity(task_id)
        .ok_or_else(|| anyhow::anyhow!("image receipt task ID is not canonical"))?;
    anyhow::ensure!(
        identity.profile == receipt.profile
            && identity.architecture == receipt.architecture
            && identity.engine_identity_fingerprint == receipt.engine_identity_fingerprint,
        "image smoke receipt does not match its task identity"
    );
    anyhow::ensure!(
        receipt.successful_smoke_at_unix_ms > 0,
        "image smoke receipt has no successful smoke time"
    );
    anyhow::ensure!(
        is_lower_sha256(identity.engine_identity_fingerprint),
        "image task has an invalid engine identity fingerprint"
    );
    anyhow::ensure!(
        receipt
            .image_digest
            .strip_prefix("sha256:")
            .is_some_and(is_lower_sha256),
        "image smoke receipt has an invalid image digest"
    );
    if require_current_lock {
        let selected = ImageLock::bundled()?.select(identity.profile, identity.architecture)?;
        anyhow::ensure!(
            selected.digest == receipt.image_digest,
            "image smoke receipt does not match the bundled digest lock"
        );
    }
    Ok(())
}

pub(in crate::persistence::setup_tasks) fn is_image_task_id(task_id: &str) -> bool {
    task_id.starts_with("setup:images:")
}

pub(super) fn image_task_identity(task_id: &str) -> Option<ImageTaskIdentity<'_>> {
    let mut segments = task_id.split(':');
    let prefix = segments.next()?;
    let domain = segments.next()?;
    let profile_name = segments.next()?;
    let architecture = segments.next()?;
    let engine_identity_fingerprint = segments.next()?;
    if segments.next().is_some() {
        return None;
    }
    if prefix != "setup" || domain != "images" {
        return None;
    }
    let profile = match profile_name {
        "general" => RuntimeProfile::General,
        "rust" => RuntimeProfile::Rust,
        "node" => RuntimeProfile::Node,
        "python" => RuntimeProfile::Python,
        "go" => RuntimeProfile::Go,
        _ => return None,
    };
    if !matches!(architecture, "amd64" | "arm64") || !is_lower_sha256(engine_identity_fingerprint) {
        return None;
    }
    Some(ImageTaskIdentity {
        profile,
        architecture,
        engine_identity_fingerprint,
    })
}

fn is_lower_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

#[cfg(test)]
#[path = "image_receipt/tests.rs"]
mod tests;
