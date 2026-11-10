use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::computer_use::artifacts::{COMPUTER_ARTIFACT_MAX_BYTES_V1, ComputerArtifactReceiptV1};

use super::GuestAppArchitectureV1;

mod build;
pub use build::{StagedTaskApplicationBuildV1, StagedTaskBuildFileV1};

const MAX_STAGED_FILES: usize = 64;
const MAX_TOTAL_STAGED_BYTES: u64 = COMPUTER_ARTIFACT_MAX_BYTES_V1;

/// One file receipt in a task-approved build. Paths are relative to the
/// approved build root and never become host paths or package-manager inputs.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct StagedTaskArtifactV1 {
    pub relative_path: String,
    pub transfer_grant_id: String,
    pub receipt: ComputerArtifactReceiptV1,
}

/// Private host-side bytes loaded only through the P15 grant-aware bridge.
#[derive(Clone, Eq, PartialEq)]
pub struct StagedTaskArtifactContentV1 {
    pub artifact: StagedTaskArtifactV1,
    pub bytes: Vec<u8>,
}

/// Typed identity for launching an explicitly approved task build in a guest.
/// The build hash uses Runtime QA's canonical sorted JSON tuple manifest.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct StagedTaskApplicationV1 {
    pub schema_version: u16,
    pub app_id: String,
    pub executable_id: String,
    pub executable_relative_path: String,
    pub task_revision: String,
    pub app_target_id: String,
    pub execution_recipe_id: String,
    /// Bare lowercase SHA-256 hex of sorted `(relative_path, file_sha256)` JSON tuples.
    pub build_receipt_sha256: String,
    /// Hash of the persisted, target-specific human approval receipt.
    pub build_approval_sha256: String,
    pub guest_image_digest: String,
    pub guest_architecture: GuestAppArchitectureV1,
    pub artifacts: Vec<StagedTaskArtifactV1>,
}

impl StagedTaskApplicationV1 {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema_version != 1
            || self.app_id != crate::computer_use::mcp::COMPUTER_USE_RUNTIME_QA_APP_ID_V1
            || !valid_id(&self.executable_id)
            || !valid_path(&self.executable_relative_path)
            || !valid_id(&self.task_revision)
            || !valid_id(&self.app_target_id)
            || !valid_id(&self.execution_recipe_id)
            || !valid_hex_sha256(&self.build_receipt_sha256)
            || !valid_hex_sha256(&self.build_approval_sha256)
            || !valid_tagged_sha256(&self.guest_image_digest)
            || self.artifacts.is_empty()
            || self.artifacts.len() > MAX_STAGED_FILES
        {
            return Err("staged task application identity or bounds are invalid");
        }
        let mut previous_path: Option<&str> = None;
        let mut total_bytes = 0_u64;
        let mut manifest = Vec::with_capacity(self.artifacts.len());
        let mut executable_found = false;
        for artifact in &self.artifacts {
            if !valid_path(&artifact.relative_path)
                || !canonical_uuid(&artifact.transfer_grant_id)
                || !canonical_uuid(&artifact.receipt.artifact_id)
                || !valid_tagged_sha256(&artifact.receipt.sha256)
                || artifact.receipt.size_bytes == 0
                || artifact.receipt.size_bytes > COMPUTER_ARTIFACT_MAX_BYTES_V1
                || artifact.receipt.expires_at_unix_ms == 0
                || !valid_display_name(&artifact.receipt.display_name)
            {
                return Err("staged task artifact receipt is invalid");
            }
            if previous_path.is_some_and(|previous| previous >= artifact.relative_path.as_str()) {
                return Err("staged task artifact paths must be unique and sorted");
            }
            let file_name = artifact
                .relative_path
                .rsplit('/')
                .next()
                .unwrap_or_default();
            if file_name != artifact.receipt.display_name {
                return Err("artifact receipt name does not match its relative path");
            }
            executable_found |= artifact.relative_path == self.executable_relative_path;
            previous_path = Some(&artifact.relative_path);
            total_bytes = total_bytes.saturating_add(artifact.receipt.size_bytes);
            if total_bytes > MAX_TOTAL_STAGED_BYTES {
                return Err("staged task artifacts exceed the aggregate transfer bound");
            }
            let bare_digest = artifact
                .receipt
                .sha256
                .strip_prefix("sha256:")
                .ok_or("artifact receipt digest is malformed")?;
            manifest.push((&artifact.relative_path, bare_digest));
        }
        let encoded = serde_json::to_vec(&manifest)
            .map_err(|_| "staged task build manifest could not be encoded")?;
        let digest = hex_digest(&encoded);
        if !executable_found || digest != self.build_receipt_sha256 {
            return Err("staged task files do not match the approved build receipt");
        }
        Ok(())
    }

    pub fn validate_at(&self, now_unix_ms: u64) -> Result<(), &'static str> {
        self.validate()?;
        if now_unix_ms == 0
            || self
                .artifacts
                .iter()
                .any(|artifact| artifact.receipt.expires_at_unix_ms <= now_unix_ms)
        {
            return Err("staged task artifact receipt has expired");
        }
        Ok(())
    }
}

fn valid_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':'))
}

fn valid_path(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 1024
        && !value.starts_with('/')
        && !value.contains(['\\', ':', '\0'])
        && value.split('/').all(|part| {
            !part.is_empty()
                && part != "."
                && part != ".."
                && !part.ends_with([' ', '.'])
                && !part.chars().any(char::is_control)
        })
}

fn valid_display_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 255
        && !value.contains(['/', '\\', '\0'])
        && value != "."
        && value != ".."
        && !value.chars().any(char::is_control)
}

fn valid_hex_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn valid_tagged_sha256(value: &str) -> bool {
    value.strip_prefix("sha256:").is_some_and(valid_hex_sha256)
}

fn canonical_uuid(value: &str) -> bool {
    uuid::Uuid::parse_str(value).is_ok_and(|id| id.to_string() == value)
}

fn hex_digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

impl StagedTaskApplicationV1 {
    pub fn validate_contents(
        &self,
        contents: &[StagedTaskArtifactContentV1],
    ) -> Result<(), &'static str> {
        self.validate()?;
        if contents.len() != self.artifacts.len()
            || contents
                .iter()
                .zip(&self.artifacts)
                .any(|(content, expected)| {
                    content.artifact != *expected
                        || content.bytes.len() as u64 != expected.receipt.size_bytes
                        || format!("sha256:{}", hex_digest(&content.bytes))
                            != expected.receipt.sha256
                })
        {
            return Err("staged task artifact bytes do not match their grant receipts");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> StagedTaskApplicationV1 {
        let artifacts = vec![
            StagedTaskArtifactV1 {
                relative_path: "bin/editor".into(),
                transfer_grant_id: "00000000-0000-4000-8000-000000000001".into(),
                receipt: ComputerArtifactReceiptV1 {
                    artifact_id: "00000000-0000-4000-8000-000000000002".into(),
                    display_name: "editor".into(),
                    size_bytes: 12,
                    sha256: format!("sha256:{}", "1".repeat(64)),
                    media_type: "application/octet-stream".into(),
                    expires_at_unix_ms: 20_000,
                },
            },
            StagedTaskArtifactV1 {
                relative_path: "data/config.json".into(),
                transfer_grant_id: "00000000-0000-4000-8000-000000000003".into(),
                receipt: ComputerArtifactReceiptV1 {
                    artifact_id: "00000000-0000-4000-8000-000000000004".into(),
                    display_name: "config.json".into(),
                    size_bytes: 7,
                    sha256: format!("sha256:{}", "a".repeat(64)),
                    media_type: "application/json".into(),
                    expires_at_unix_ms: 20_000,
                },
            },
        ];
        let manifest = vec![
            ("bin/editor", "1".repeat(64)),
            ("data/config.json", "a".repeat(64)),
        ];
        let build_receipt_sha256 = hex_digest(&serde_json::to_vec(&manifest).unwrap());
        StagedTaskApplicationV1 {
            schema_version: 1,
            app_id: crate::computer_use::mcp::COMPUTER_USE_RUNTIME_QA_APP_ID_V1.into(),
            executable_id: "editor".into(),
            executable_relative_path: "bin/editor".into(),
            task_revision: "revision-9".into(),
            app_target_id: "target-4".into(),
            execution_recipe_id: "recipe-2".into(),
            build_receipt_sha256,
            build_approval_sha256: "c".repeat(64),
            guest_image_digest: format!("sha256:{}", "b".repeat(64)),
            guest_architecture: GuestAppArchitectureV1::X86_64,
            artifacts,
        }
    }

    #[test]
    fn staged_manifest_checks_sorted_file_receipts_and_approved_build_hash() {
        assert_eq!(fixture().validate(), Ok(()));

        let mut unsorted = fixture();
        unsorted.artifacts.swap(0, 1);
        assert_eq!(
            unsorted.validate(),
            Err("staged task artifact paths must be unique and sorted")
        );

        let mut tampered = fixture();
        tampered.artifacts[0].receipt.sha256 = format!("sha256:{}", "c".repeat(64));
        assert_eq!(
            tampered.validate(),
            Err("staged task files do not match the approved build receipt")
        );

        let mut unsafe_path = fixture();
        unsafe_path.artifacts[0].relative_path = "../editor".into();
        assert_eq!(
            unsafe_path.validate(),
            Err("staged task artifact receipt is invalid")
        );
    }
}
