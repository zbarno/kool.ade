use serde::{Deserialize, Serialize};
use sha2::Digest as _;

use super::{QaRunRequestV1, is_sha256, is_uuid, valid_text};

pub const QA_COMPUTER_APP_ID_V1: &str = "qa-task-app";

/// App-resolved identity for a future computer-controlled Runtime QA run.
/// This is evidence metadata, not proof of authorization: the selected
/// app-owned driver must still verify the live Computer Use session and its
/// one-use artifact grant before starting any guest action.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct QaComputerTargetV1 {
    pub schema_version: u16,
    pub qa_run_id: String,
    pub identity: crate::computer_use::ComputerIdentityV1,
    pub application_id: String,
    pub executable_id: String,
    pub app_target_id: String,
    pub execution_recipe_id: String,
    pub execution_recipe_sha256: String,
    pub relative_executable_path: String,
    pub guest_image_digest: String,
    pub guest_architecture: String,
    pub session_attestation_sha256: String,
    pub build_approval_sha256: String,
    pub task_revision: String,
    pub approved_spec_hash: String,
    pub display_width: u16,
    pub display_height: u16,
    pub display_dpi: u16,
    pub staged_artifacts: Vec<QaComputerStagedArtifactV1>,
}

/// One P15 grant-backed file in the approved build set.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct QaComputerStagedArtifactV1 {
    pub relative_path: String,
    pub transfer_grant_id: String,
    pub receipt: crate::computer_use::artifacts::ComputerArtifactReceiptV1,
}

impl QaComputerTargetV1 {
    /// Check that every staged guest input belongs to this exact approved QA
    /// subject. Host authorization and grant consumption remain driver duties.
    pub fn validate_for(&self, request: &QaRunRequestV1) -> Result<(), String> {
        let mut manifest = Vec::with_capacity(self.staged_artifacts.len());
        for artifact in &self.staged_artifacts {
            let receipt = &artifact.receipt;
            let path = artifact.relative_path.as_str();
            let file_name = std::path::Path::new(path)
                .file_name()
                .and_then(|name| name.to_str());
            if !valid_relative_artifact_path(path)
                || file_name != Some(receipt.display_name.as_str())
                || !is_uuid(&receipt.artifact_id)
                || !is_uuid(&artifact.transfer_grant_id)
                || receipt.size_bytes == 0
                || receipt.size_bytes
                    > crate::computer_use::artifacts::COMPUTER_ARTIFACT_MAX_BYTES_V1
                || !is_tagged_sha256(&receipt.sha256)
                || !valid_text(&receipt.media_type, 128)
                || !valid_text(&receipt.display_name, 255)
            {
                return Err("Computer QA staged artifact receipt is invalid".into());
            }
            let bare_digest = receipt
                .sha256
                .strip_prefix("sha256:")
                .ok_or_else(|| "Computer QA artifact digest is not canonical".to_owned())?;
            manifest.push((path.to_owned(), bare_digest.to_owned()));
        }
        if manifest.is_empty()
            || manifest.len() > 64
            || manifest.windows(2).any(|pair| pair[0].0 >= pair[1].0)
            || self
                .staged_artifacts
                .iter()
                .map(|artifact| artifact.receipt.size_bytes)
                .sum::<u64>()
                > crate::computer_use::artifacts::COMPUTER_ARTIFACT_MAX_BYTES_V1
        {
            return Err(
                "Computer QA staged artifact set is empty, duplicated, or exceeds its aggregate byte bound".into(),
            );
        }
        let manifest_bytes = serde_json::to_vec(&manifest)
            .map_err(|_| "Computer QA staged artifact manifest could not be encoded")?;
        let manifest_sha = format!("{:x}", sha2::Sha256::digest(manifest_bytes));
        if self.schema_version != 1
            || self.identity.validate().is_err()
            || self.qa_run_id != request.qa_run_id
            || self.identity.task_id != request.parent_task_uid
            || self.application_id != QA_COMPUTER_APP_ID_V1
            || self.app_target_id != request.target_id.as_deref().unwrap_or_default()
            || self.execution_recipe_id != request.startup_recipe_ref
            || self.execution_recipe_sha256 != request.startup_recipe_sha256
            || !is_sha256(&self.execution_recipe_sha256)
            || !valid_application_id(&self.executable_id)
            || !valid_relative_artifact_path(&self.relative_executable_path)
            || !manifest
                .iter()
                .any(|(path, _)| path == &self.relative_executable_path)
            || !is_tagged_sha256(&self.guest_image_digest)
            || !matches!(self.guest_architecture.as_str(), "x86_64" | "aarch64")
            || !is_sha256(&self.session_attestation_sha256)
            || !is_sha256(&self.build_approval_sha256)
            || self.task_revision != request.code_head_sha
            || self.approved_spec_hash != request.approved_spec_hash
            || manifest_sha != request.build_artifact_sha256
            || !(1024..=7680).contains(&self.display_width)
            || !(768..=4320).contains(&self.display_height)
            || !(48..=960).contains(&self.display_dpi)
            || !is_uuid(&request.parent_task_uid)
        {
            return Err(
                "Computer QA target is not bound to the approved task build and session".into(),
            );
        }
        Ok(())
    }
}

fn is_tagged_sha256(value: &str) -> bool {
    value.strip_prefix("sha256:").is_some_and(is_sha256)
}

fn valid_relative_artifact_path(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 1024
        && !value.contains('\\')
        && !value.contains('\0')
        && value
            .split('/')
            .all(|part| !part.is_empty() && !matches!(part, "." | ".."))
}

fn valid_application_id(value: &str) -> bool {
    let mut bytes = value.bytes();
    bytes
        .next()
        .is_some_and(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
        && value.len() <= 128
        && !value.contains("..")
        && bytes.all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"._-".contains(&byte)
        })
}
