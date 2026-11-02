use serde::{Deserialize, Serialize};

use super::HelperOutcome;

pub(crate) const MAX_HANDOFF_BYTES: usize = 4096;
pub(crate) const WSL_FEATURE_TASK: &str = "setup:windows:wsl-features";
pub(crate) const WSL_FEATURE_OPERATION: &str = "enable_windows_wsl_features";
const HANDOFF_TTL_MS: u64 = 10 * 60 * 1000;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct HandoffV1 {
    pub schema_version: u8,
    pub ticket_id: String,
    pub operation: String,
    pub task_id: String,
    pub operation_id: String,
    pub owner_instance_id: String,
    pub receipt_id: String,
    pub scope_digest: String,
    pub plan_digest: String,
    pub invocation_digest: String,
    pub expires_at_unix_ms: u64,
}

impl HandoffV1 {
    pub(crate) fn validate(&self, now_unix_ms: u64) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.schema_version == 1,
            "unsupported Windows setup handoff"
        );
        anyhow::ensure!(
            self.operation == WSL_FEATURE_OPERATION,
            "unsupported Windows setup operation"
        );
        anyhow::ensure!(
            self.task_id == WSL_FEATURE_TASK,
            "Windows setup approval targets another task"
        );
        for id in [
            &self.ticket_id,
            &self.operation_id,
            &self.owner_instance_id,
            &self.receipt_id,
        ] {
            let parsed = uuid::Uuid::parse_str(id)
                .map_err(|_| anyhow::anyhow!("Windows setup handoff identity is invalid"))?;
            anyhow::ensure!(
                parsed.get_version() == Some(uuid::Version::Random)
                    && parsed.get_variant() == uuid::Variant::RFC4122,
                "Windows setup handoff identity is invalid"
            );
        }
        for digest in [
            &self.scope_digest,
            &self.plan_digest,
            &self.invocation_digest,
        ] {
            anyhow::ensure!(
                digest.len() == 64 && digest.bytes().all(|byte| byte.is_ascii_hexdigit()),
                "Windows setup handoff binding is invalid"
            );
        }
        anyhow::ensure!(
            self.expires_at_unix_ms > now_unix_ms
                && self.expires_at_unix_ms <= now_unix_ms.saturating_add(HANDOFF_TTL_MS),
            "Windows setup authorization expired"
        );
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct HandoffResultV1 {
    pub schema_version: u8,
    pub ticket_id: String,
    pub outcome: HelperOutcome,
    pub exit_code: i32,
}

impl HandoffResultV1 {
    pub(crate) fn validate(&self, ticket_id: &str) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.schema_version == 1 && self.ticket_id == ticket_id,
            "Windows setup result does not match its one-use handoff"
        );
        let expected = match self.outcome {
            HelperOutcome::Completed => 0,
            HelperOutcome::RebootRequired => 3010,
            HelperOutcome::Failed => 1,
        };
        anyhow::ensure!(
            self.exit_code == expected,
            "Windows setup result code does not match its typed outcome"
        );
        Ok(())
    }
}

pub(crate) fn encode<T: Serialize>(message: &T) -> anyhow::Result<Vec<u8>> {
    let bytes = serde_json::to_vec(message)?;
    anyhow::ensure!(
        !bytes.is_empty() && bytes.len() <= MAX_HANDOFF_BYTES,
        "Windows setup handoff exceeds its size limit"
    );
    Ok(bytes)
}

pub(crate) fn decode<T: for<'de> Deserialize<'de>>(bytes: &[u8]) -> anyhow::Result<T> {
    anyhow::ensure!(
        !bytes.is_empty() && bytes.len() <= MAX_HANDOFF_BYTES,
        "Windows setup handoff exceeds its size limit"
    );
    Ok(serde_json::from_slice(bytes)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn handoff() -> HandoffV1 {
        HandoffV1 {
            schema_version: 1,
            ticket_id: uuid::Uuid::new_v4().to_string(),
            operation: WSL_FEATURE_OPERATION.into(),
            task_id: WSL_FEATURE_TASK.into(),
            operation_id: uuid::Uuid::new_v4().to_string(),
            owner_instance_id: uuid::Uuid::new_v4().to_string(),
            receipt_id: uuid::Uuid::new_v4().to_string(),
            scope_digest: "a".repeat(64),
            plan_digest: "b".repeat(64),
            invocation_digest: "c".repeat(64),
            expires_at_unix_ms: 10_000,
        }
    }

    #[test]
    fn handoff_binds_one_supported_task_and_unexpired_digests() {
        let request = handoff();
        request.validate(9_999).unwrap();
        let bytes = encode(&request).unwrap();
        assert_eq!(decode::<HandoffV1>(&bytes).unwrap(), request);

        let mut expired = request.clone();
        expired.expires_at_unix_ms = 9_999;
        assert!(expired.validate(9_999).is_err());
        let mut wrong_task = request.clone();
        wrong_task.task_id = "setup:linux:rootless:packages".into();
        assert!(wrong_task.validate(9_999).is_err());
        let mut wrong_operation = request.clone();
        wrong_operation.operation = "run_host_command".into();
        assert!(wrong_operation.validate(9_999).is_err());
        let mut invalid_variant = request.clone();
        invalid_variant.ticket_id = "00000000-0000-4000-0000-000000000001".into();
        assert!(invalid_variant.validate(9_999).is_err());
        let mut invalid_digest = request;
        invalid_digest.scope_digest = "not-a-digest".into();
        assert!(invalid_digest.validate(9_999).is_err());
    }

    #[test]
    fn helper_result_is_bound_to_ticket_and_exit_code() {
        let result = HandoffResultV1 {
            schema_version: 1,
            ticket_id: uuid::Uuid::new_v4().to_string(),
            outcome: HelperOutcome::RebootRequired,
            exit_code: 3010,
        };
        result.validate(&result.ticket_id).unwrap();
        assert!(
            result
                .validate("00000000-0000-4000-8000-000000000000")
                .is_err()
        );
        let mut mismatch = result.clone();
        mismatch.exit_code = 0;
        assert!(mismatch.validate(&result.ticket_id).is_err());
        let failed = HandoffResultV1 {
            outcome: HelperOutcome::Failed,
            exit_code: 1,
            ..result
        };
        failed.validate(&failed.ticket_id).unwrap();
    }

    #[test]
    fn messages_reject_unknown_fields_and_oversized_payloads() {
        assert!(decode::<HandoffV1>(br#"{"schemaVersion":1,"extra":true}"#).is_err());
        assert!(decode::<HandoffV1>(&vec![b'x'; MAX_HANDOFF_BYTES + 1]).is_err());
    }
}
