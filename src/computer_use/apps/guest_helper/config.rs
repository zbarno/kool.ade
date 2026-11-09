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
}

impl GuestAppHelperConfigV1 {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema_version != 1
            || !canonical_uuid(&self.session_id)
            || !canonical_uuid(&self.worker_id)
            || !canonical_uuid(&self.boot_id)
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
}
