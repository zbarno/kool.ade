use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::computer_use::ComputerSessionClassV1;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct GuestAppHelperConfigV1 {
    pub schema_version: u16,
    pub session_id: String,
    pub worker_id: String,
    pub image_digest: String,
    pub session_class: ComputerSessionClassV1,
    pub lease_generation: u64,
    pub approval_generation: u64,
    pub cancellation_generation: u64,
    pub initial_action_sequence: u64,
    pub expires_at_unix_ms: u64,
    pub boot_id: String,
    pub allowed_apps: Vec<String>,
    pub controller_key_id: String,
    pub controller_verifying_key_hex: String,
}

impl GuestAppHelperConfigV1 {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema_version != 1
            || !canonical_uuid(&self.session_id)
            || !canonical_uuid(&self.worker_id)
            || !canonical_uuid(&self.boot_id)
            || !canonical_uuid(&self.controller_key_id)
            || self.controller_verifying_key_hex.len() != 64
            || !self
                .controller_verifying_key_hex
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
            || !valid_sha256(&self.image_digest)
            || self.lease_generation == 0
            || self.approval_generation == 0
            || self.cancellation_generation == 0
            || self.initial_action_sequence > 10_000
            || self.expires_at_unix_ms == 0
            || self.allowed_apps.len() > 32
            || self.allowed_apps.iter().any(|app| !valid_id(app))
        {
            return Err("guest app helper context is invalid");
        }
        Ok(())
    }

    pub(crate) fn verifies_action(
        &self,
        action: &crate::computer_use::ComputerActionV1,
        phase: crate::computer_use::ComputerActionHelperPhaseV1,
    ) -> bool {
        let Ok(bytes) = decode_hex(&self.controller_verifying_key_hex) else {
            return false;
        };
        let Ok(key_bytes) = <[u8; 32]>::try_from(bytes.as_slice()) else {
            return false;
        };
        let Ok(key) = ed25519_dalek::VerifyingKey::from_bytes(&key_bytes) else {
            return false;
        };
        action.verify_guest_helper_authorization(&self.controller_key_id, phase, &key)
    }
}

fn decode_hex(value: &str) -> Result<Vec<u8>, ()> {
    let (pairs, remainder) = value.as_bytes().as_chunks::<2>();
    if !remainder.is_empty() {
        return Err(());
    }
    pairs
        .iter()
        .map(|pair| {
            let digit = |byte| match byte {
                b'0'..=b'9' => Some(byte - b'0'),
                b'a'..=b'f' => Some(byte - b'a' + 10),
                _ => None,
            };
            Ok(digit(pair[0]).ok_or(())? * 16 + digit(pair[1]).ok_or(())?)
        })
        .collect()
}

fn canonical_uuid(value: &str) -> bool {
    Uuid::parse_str(value).is_ok_and(|id| id.to_string() == value)
}

fn valid_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"-.".contains(&byte))
}

fn valid_sha256(value: &str) -> bool {
    value.len() == 71
        && value.starts_with("sha256:")
        && value[7..]
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

#[cfg(test)]
mod tests {
    use super::GuestAppHelperConfigV1;
    use crate::computer_use::ComputerSessionClassV1;

    #[test]
    fn helper_context_rejects_bad_generation_digest_and_app_ids() {
        let valid = GuestAppHelperConfigV1 {
            schema_version: 1,
            session_id: "00000000-0000-4000-8000-000000000001".into(),
            worker_id: "00000000-0000-4000-8000-000000000002".into(),
            image_digest: format!("sha256:{}", "a".repeat(64)),
            session_class: ComputerSessionClassV1::Disposable,
            lease_generation: 1,
            approval_generation: 1,
            cancellation_generation: 1,
            initial_action_sequence: 0,
            expires_at_unix_ms: 1000,
            boot_id: "00000000-0000-4000-8000-000000000003".into(),
            allowed_apps: vec!["text-editor".into()],
            controller_key_id: "00000000-0000-4000-8000-000000000004".into(),
            controller_verifying_key_hex: "1a".repeat(32),
        };
        assert!(valid.validate().is_ok());
        let mut invalid = valid.clone();
        invalid.approval_generation = 0;
        assert!(invalid.validate().is_err());
        let mut invalid = valid.clone();
        invalid.image_digest = "sha256:../etc/passwd".into();
        assert!(invalid.validate().is_err());
        let mut invalid = valid;
        invalid.allowed_apps = vec!["/usr/bin/sh".into()];
        assert!(invalid.validate().is_err());
    }

    #[test]
    fn helper_rejects_forged_local_stage_action_and_tampered_manifest() {
        use crate::computer_use::{
            ComputerActionActorV1, ComputerActionPayloadV1, ComputerActionV1, ComputerOperationV1,
            apps::{GuestAppArchitectureV1, StagedTaskApplicationV1, StagedTaskArtifactV1},
            artifacts::ComputerArtifactReceiptV1,
        };

        let controller = ed25519_dalek::SigningKey::from_bytes(&[41; 32]);
        let agent = ed25519_dalek::SigningKey::from_bytes(&[42; 32]);
        let key_id = "00000000-0000-4000-8000-000000000004";
        let mut config = GuestAppHelperConfigV1 {
            schema_version: 1,
            session_id: "00000000-0000-4000-8000-000000000001".into(),
            worker_id: "00000000-0000-4000-8000-000000000002".into(),
            image_digest: format!("sha256:{}", "a".repeat(64)),
            session_class: ComputerSessionClassV1::Disposable,
            lease_generation: 1,
            approval_generation: 1,
            cancellation_generation: 1,
            initial_action_sequence: 0,
            expires_at_unix_ms: 60_000,
            boot_id: "00000000-0000-4000-8000-000000000003".into(),
            allowed_apps: vec!["qa-task-app".into()],
            controller_key_id: key_id.into(),
            controller_verifying_key_hex: controller
                .verifying_key()
                .to_bytes()
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect(),
        };
        assert!(config.validate().is_ok());

        let application = StagedTaskApplicationV1 {
            schema_version: 1,
            app_id: "qa-task-app".into(),
            executable_id: "editor".into(),
            executable_relative_path: "bin/editor".into(),
            task_revision: "revision-1".into(),
            app_target_id: "target-1".into(),
            execution_recipe_id: "recipe-1".into(),
            build_receipt_sha256: "b".repeat(64),
            build_approval_sha256: "c".repeat(64),
            guest_image_digest: config.image_digest.clone(),
            guest_architecture: GuestAppArchitectureV1::X86_64,
            artifacts: vec![StagedTaskArtifactV1 {
                relative_path: "bin/editor".into(),
                transfer_grant_id: "00000000-0000-4000-8000-000000000005".into(),
                receipt: ComputerArtifactReceiptV1 {
                    artifact_id: "00000000-0000-4000-8000-000000000006".into(),
                    display_name: "editor".into(),
                    size_bytes: 12,
                    sha256: format!("sha256:{}", "d".repeat(64)),
                    media_type: "application/octet-stream".into(),
                    expires_at_unix_ms: 50_000,
                },
            }],
        };
        let mut action = ComputerActionV1 {
            schema_version: 1,
            session_id: config.session_id.clone(),
            action_sequence: 1,
            idempotency_key: "00000000-0000-4000-8000-000000000007".into(),
            actor: ComputerActionActorV1::Human,
            lease_generation: config.lease_generation,
            approval_generation: config.approval_generation,
            cancellation_generation: config.cancellation_generation,
            operation: ComputerOperationV1::LaunchApprovedStagedTaskApp,
            payload: ComputerActionPayloadV1::LaunchApprovedStagedTaskApp {
                application: Box::new(application),
            },
            helper_authorization: None,
        };

        // A compromised guest agent can set Human and every public fence, but
        // cannot sign with the controller key provisioned through fw_cfg.
        action
            .sign_for_guest_helper(
                key_id,
                crate::computer_use::ComputerActionHelperPhaseV1::Stage,
                &agent,
            )
            .unwrap();
        assert!(!config.verifies_action(
            &action,
            crate::computer_use::ComputerActionHelperPhaseV1::Stage
        ));
        action
            .sign_for_guest_helper(
                key_id,
                crate::computer_use::ComputerActionHelperPhaseV1::Stage,
                &controller,
            )
            .unwrap();
        assert!(config.verifies_action(
            &action,
            crate::computer_use::ComputerActionHelperPhaseV1::Stage
        ));
        assert!(!config.verifies_action(
            &action,
            crate::computer_use::ComputerActionHelperPhaseV1::Launch
        ));

        let mut forged = action.clone();
        forged.action_sequence += 1;
        assert!(!config.verifies_action(
            &forged,
            crate::computer_use::ComputerActionHelperPhaseV1::Stage
        ));
        if let ComputerActionPayloadV1::LaunchApprovedStagedTaskApp { application } =
            &mut forged.payload
        {
            application.artifacts[0].receipt.sha256 = format!("sha256:{}", "e".repeat(64));
        }
        assert!(!config.verifies_action(
            &forged,
            crate::computer_use::ComputerActionHelperPhaseV1::Stage
        ));

        config.controller_verifying_key_hex = "00".repeat(32);
        assert!(!config.verifies_action(
            &action,
            crate::computer_use::ComputerActionHelperPhaseV1::Stage
        ));
    }
}
