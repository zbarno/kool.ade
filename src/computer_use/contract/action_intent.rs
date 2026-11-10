use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Risk labels describe the human-reviewed operation, not a guess from pixels.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ComputerActionRiskV1 {
    Observation,
    LocalReversible,
    ExternalMessage,
    PublicPost,
    AccountChange,
    PrivateDataExport,
    DestructiveOperation,
    PaidTransaction,
    Unclassified,
}

/// Private, exact description of one proposed external side effect.
///
/// This record is local controller data. Do not place it in repository files,
/// provider prompts, public artifacts, or telemetry. `action_sha256` binds the
/// final typed GUI action and `frame_sha256` binds the screen reviewed by the
/// operator. The controller still rechecks both immediately before dispatch.
#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ComputerActionIntentV1 {
    pub schema_version: u16,
    pub intent_id: String,
    pub operator_id: String,
    pub task_id: String,
    pub session_id: String,
    pub lease_generation: u64,
    pub approval_generation: u64,
    pub cancellation_generation: u64,
    pub risk: ComputerActionRiskV1,
    pub destination: String,
    pub affected_item: String,
    pub content_preview: Option<String>,
    pub amount_minor_units: Option<u64>,
    pub currency: Option<String>,
    pub frame_revision: u64,
    pub frame_sha256: String,
    pub action_sha256: String,
    pub expires_at_unix_ms: u64,
    pub one_time_nonce: String,
}

impl ComputerActionIntentV1 {
    pub fn validate(&self, now_unix_ms: u64) -> Result<(), &'static str> {
        if self.schema_version != 1
            || !canonical_uuid(&self.intent_id)
            || !canonical_uuid(&self.operator_id)
            || !canonical_uuid(&self.task_id)
            || !canonical_uuid(&self.session_id)
            || self.lease_generation == 0
            || self.approval_generation == 0
            || self.cancellation_generation == 0
            || self.frame_revision == 0
            || !valid_sha256(&self.frame_sha256)
            || !valid_sha256(&self.action_sha256)
            || !canonical_uuid(&self.one_time_nonce)
            || self.expires_at_unix_ms <= now_unix_ms
            || self.expires_at_unix_ms.saturating_sub(now_unix_ms) > 300_000
            || !bounded_text(&self.destination, 512)
            || !bounded_text(&self.affected_item, 512)
            || self.content_preview.as_ref().is_some_and(|text| {
                text.len() > 2_048 || text.contains('\0') || text.trim().is_empty()
            })
        {
            return Err("action intent is malformed or expired");
        }

        let has_content = self.content_preview.is_some();
        match self.risk {
            ComputerActionRiskV1::ExternalMessage | ComputerActionRiskV1::PublicPost
                if !has_content || self.destination.trim().is_empty() =>
            {
                Err("message intent requires its destination and exact content")
            }
            ComputerActionRiskV1::PaidTransaction
                if self.amount_minor_units.is_none_or(|amount| amount == 0)
                    || !self.currency.as_deref().is_some_and(valid_currency)
                    || self.destination.trim().is_empty()
                    || self.affected_item.trim().is_empty() =>
            {
                Err("transaction intent requires item, destination, amount, and currency")
            }
            ComputerActionRiskV1::PaidTransaction
                if has_content || self.amount_minor_units.is_none() || self.currency.is_none() =>
            {
                Err("transaction scope contains unsupported content fields")
            }
            ComputerActionRiskV1::PaidTransaction => Ok(()),
            _ if self.amount_minor_units.is_some() || self.currency.is_some() => {
                Err("only a paid transaction may include amount or currency")
            }
            _ => Ok(()),
        }
    }

    /// Stable digest of every field shown for approval, including the one-use nonce.
    pub fn digest(&self) -> Result<String, &'static str> {
        let bytes = serde_json::to_vec(self).map_err(|_| "action intent could not be encoded")?;
        Ok(format!("sha256:{:x}", Sha256::digest(bytes)))
    }
}

/// Controller-owned one-time authority. It is never accepted from an agent or guest.
#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ComputerActionApprovalV1 {
    pub schema_version: u16,
    pub intent_id: String,
    pub intent_sha256: String,
    pub operator_id: String,
    pub task_id: String,
    pub session_id: String,
    pub lease_generation: u64,
    pub approval_generation: u64,
    pub cancellation_generation: u64,
    pub expires_at_unix_ms: u64,
    pub one_time_nonce: String,
}

impl ComputerActionApprovalV1 {
    pub fn validate_for(
        &self,
        intent: &ComputerActionIntentV1,
        now_unix_ms: u64,
    ) -> Result<(), &'static str> {
        intent.validate(now_unix_ms)?;
        if self.schema_version != 1
            || self.intent_id != intent.intent_id
            || self.intent_sha256 != intent.digest()?
            || self.operator_id != intent.operator_id
            || self.task_id != intent.task_id
            || self.session_id != intent.session_id
            || self.lease_generation != intent.lease_generation
            || self.approval_generation != intent.approval_generation
            || self.cancellation_generation != intent.cancellation_generation
            || self.one_time_nonce != intent.one_time_nonce
            || self.expires_at_unix_ms != intent.expires_at_unix_ms
            || self.expires_at_unix_ms <= now_unix_ms
        {
            return Err("approval does not match the current action intent");
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ComputerActionIntentStateV1 {
    Prepared,
    AwaitingApproval,
    ApprovedOnce,
    Executing,
    VerifiedCompleted,
    OutcomeUnknown,
    Cancelled,
    Denied,
    Expired,
}

impl ComputerActionIntentStateV1 {
    pub fn can_transition_to(self, next: Self) -> bool {
        use ComputerActionIntentStateV1 as State;
        matches!(
            (self, next),
            (
                State::Prepared,
                State::AwaitingApproval | State::Cancelled | State::Expired
            ) | (
                State::AwaitingApproval,
                State::ApprovedOnce | State::Denied | State::Cancelled | State::Expired
            ) | (
                State::ApprovedOnce,
                State::Executing | State::Cancelled | State::Expired
            ) | (
                State::Executing,
                State::VerifiedCompleted | State::OutcomeUnknown
            )
        )
    }
}

fn canonical_uuid(value: &str) -> bool {
    uuid::Uuid::parse_str(value).is_ok_and(|id| id.to_string() == value)
}

fn valid_sha256(value: &str) -> bool {
    value.len() == 71
        && value.starts_with("sha256:")
        && value[7..]
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn valid_currency(value: &str) -> bool {
    value.len() == 3 && value.bytes().all(|byte| byte.is_ascii_uppercase())
}

fn bounded_text(value: &str, maximum: usize) -> bool {
    value.len() <= maximum
        && !value.contains('\0')
        && value.chars().all(|character| !character.is_control())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn intent(risk: ComputerActionRiskV1, now: u64) -> ComputerActionIntentV1 {
        ComputerActionIntentV1 {
            schema_version: 1,
            intent_id: "00000000-0000-4000-8000-000000000001".into(),
            operator_id: "00000000-0000-4000-8000-000000000002".into(),
            task_id: "00000000-0000-4000-8000-000000000003".into(),
            session_id: "00000000-0000-4000-8000-000000000004".into(),
            lease_generation: 4,
            approval_generation: 5,
            cancellation_generation: 6,
            risk,
            destination: "merchant.test/order/42".into(),
            affected_item: "fixture item".into(),
            content_preview: matches!(
                risk,
                ComputerActionRiskV1::ExternalMessage | ComputerActionRiskV1::PublicPost
            )
            .then(|| "Synthetic message".into()),
            amount_minor_units: (risk == ComputerActionRiskV1::PaidTransaction).then_some(1299),
            currency: (risk == ComputerActionRiskV1::PaidTransaction).then(|| "USD".into()),
            frame_revision: 7,
            frame_sha256: format!("sha256:{}", "a".repeat(64)),
            action_sha256: format!("sha256:{}", "b".repeat(64)),
            expires_at_unix_ms: now + 60_000,
            one_time_nonce: "00000000-0000-4000-8000-000000000005".into(),
        }
    }

    #[test]
    fn intent_requires_exact_fields_for_synthetic_sensitive_categories() {
        for risk in [
            ComputerActionRiskV1::ExternalMessage,
            ComputerActionRiskV1::PublicPost,
            ComputerActionRiskV1::AccountChange,
            ComputerActionRiskV1::PrivateDataExport,
            ComputerActionRiskV1::DestructiveOperation,
            ComputerActionRiskV1::PaidTransaction,
        ] {
            let candidate = intent(risk, 100);
            assert!(candidate.validate(100).is_ok(), "{risk:?}");
        }
    }

    #[test]
    fn approval_binds_every_generation_scope_and_exact_action() {
        let intent = intent(ComputerActionRiskV1::PaidTransaction, 100);
        let approval = ComputerActionApprovalV1 {
            schema_version: 1,
            intent_id: intent.intent_id.clone(),
            intent_sha256: intent.digest().unwrap(),
            operator_id: intent.operator_id.clone(),
            task_id: intent.task_id.clone(),
            session_id: intent.session_id.clone(),
            lease_generation: intent.lease_generation,
            approval_generation: intent.approval_generation,
            cancellation_generation: intent.cancellation_generation,
            expires_at_unix_ms: intent.expires_at_unix_ms,
            one_time_nonce: intent.one_time_nonce.clone(),
        };
        assert!(approval.validate_for(&intent, 101).is_ok());

        let mut changed = intent.clone();
        changed.amount_minor_units = Some(1300);
        assert!(approval.validate_for(&changed, 101).is_err());
        changed = intent.clone();
        changed.action_sha256 = format!("sha256:{}", "c".repeat(64));
        assert!(approval.validate_for(&changed, 101).is_err());
        changed = intent.clone();
        changed.lease_generation += 1;
        assert!(approval.validate_for(&changed, 101).is_err());
    }

    #[test]
    fn expired_unclassified_and_incomplete_intents_fail_closed() {
        let candidate = intent(ComputerActionRiskV1::Unclassified, 100);
        assert!(candidate.validate(candidate.expires_at_unix_ms).is_err());
        let mut candidate = intent(ComputerActionRiskV1::PaidTransaction, 100);
        candidate.amount_minor_units = None;
        assert!(candidate.validate(100).is_err());
        let mut candidate = intent(ComputerActionRiskV1::ExternalMessage, 100);
        candidate.content_preview = None;
        assert!(candidate.validate(100).is_err());
    }

    #[test]
    fn intent_state_machine_consumes_approval_before_execution() {
        use ComputerActionIntentStateV1 as State;
        assert!(State::Prepared.can_transition_to(State::AwaitingApproval));
        assert!(State::AwaitingApproval.can_transition_to(State::ApprovedOnce));
        assert!(State::ApprovedOnce.can_transition_to(State::Executing));
        assert!(State::Executing.can_transition_to(State::OutcomeUnknown));
        assert!(!State::Executing.can_transition_to(State::ApprovedOnce));
        assert!(!State::OutcomeUnknown.can_transition_to(State::Executing));
        assert!(!State::VerifiedCompleted.can_transition_to(State::Executing));
        assert!(!State::Denied.can_transition_to(State::ApprovedOnce));
    }
}
