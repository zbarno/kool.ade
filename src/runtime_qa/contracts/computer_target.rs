use serde::{Deserialize, Serialize};

use super::{QaRunRequestV1, is_sha256, is_uuid, valid_text};

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
    pub guest_image_sha256: String,
    pub session_attestation_sha256: String,
    pub task_revision: String,
    pub approved_spec_hash: String,
    pub staged_artifact: crate::computer_use::artifacts::ComputerArtifactReceiptV1,
}

impl QaComputerTargetV1 {
    /// Check that every staged guest input belongs to this exact approved QA
    /// subject. Host authorization and grant consumption remain driver duties.
    pub fn validate_for(&self, request: &QaRunRequestV1) -> Result<(), String> {
        let artifact = &self.staged_artifact;
        if self.schema_version != 1
            || self.identity.validate().is_err()
            || self.qa_run_id != request.qa_run_id
            || self.identity.task_id != request.parent_task_uid
            || !valid_application_id(&self.application_id)
            || !is_sha256(&self.guest_image_sha256)
            || !is_sha256(&self.session_attestation_sha256)
            || self.task_revision != request.code_head_sha
            || self.approved_spec_hash != request.approved_spec_hash
            || !is_uuid(&artifact.artifact_id)
            || artifact.size_bytes == 0
            || artifact.size_bytes > crate::computer_use::artifacts::COMPUTER_ARTIFACT_MAX_BYTES_V1
            || artifact.sha256 != request.build_artifact_sha256
            || !valid_text(&artifact.media_type, 128)
            || !valid_text(&artifact.display_name, 255)
            || !is_uuid(&request.parent_task_uid)
        {
            return Err(
                "Computer QA target is not bound to the approved task build and session".into(),
            );
        }
        Ok(())
    }
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
