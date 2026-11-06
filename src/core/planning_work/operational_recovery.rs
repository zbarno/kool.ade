//! Durable Manager ownership of a failed saved answer. Private application state.
use super::{Work, WorkKind, WorkStatus};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FailureKind {
    FeatureIdentity,
    ResponseFormat,
    Execution,
    PlanningConflict,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RecoveryV1 {
    #[serde(default)]
    pub autonomous_investigation: bool,
    #[serde(default)]
    pub investigation_claims: u8,
    pub source_key: String,
    pub source_uid: String,
    pub reply_id: String,
    pub expected_feature_uid: Option<String>,
    pub failure: FailureKind,
    pub evidence: String,
    pub repair_attempts: u8,
    pub repair_error: Option<String>,
    pub attempts: u8,
    pub next_attempt_ms: i64,
}

/// One durable task per immutable source and submitted answer, across restarts.
pub(crate) fn task(source: &Work, reply_id: &str, failure: FailureKind, evidence: String) -> Work {
    let mut task = Work::new(
        String::new(),
        format!("Manager triage: {}", source.title),
        "Investigate the saved answer failure, repair supported operational failures, and verify that the original answer was applied. Preserve its scope and immutable feature identity.".into(),
        "Manager owns recovery. The original answer remains saved; no repeated user answer is required.".into(),
    );
    use sha2::Digest;
    let digest = sha2::Sha256::digest(format!("operational-triage:{}:{reply_id}", source.uid));
    let mut bytes = [0; 16];
    bytes.copy_from_slice(&digest[..16]);
    bytes[6] = (bytes[6] & 0x0f) | 0x50;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    task.uid = uuid::Uuid::from_bytes(bytes).to_string();
    task.key = format!("task:manager-triage:{}", task.uid);
    task.kind = WorkKind::Question;
    task.status = WorkStatus::Todo;
    task.parent_uid = Some(source.uid.clone());
    task.task_intent_uid = source.task_intent_root_uid().map(str::to_owned);
    task.task_intent_cycle_uid = source.task_intent_cycle_uid.clone();
    task.operational_recovery = Some(RecoveryV1 {
        autonomous_investigation: false,
        investigation_claims: 0,
        source_key: source.key.clone(),
        source_uid: source.uid.clone(),
        reply_id: reply_id.to_owned(),
        expected_feature_uid: source.feature_uid.clone(),
        failure,
        evidence: crate::core::context_build::clip_bytes(&evidence, 16 * 1024),
        repair_attempts: 0,
        repair_error: None,
        attempts: 0,
        next_attempt_ms: 0,
    });
    task
}

pub(super) fn validate(work: &Work, recovery: &RecoveryV1) -> anyhow::Result<()> {
    anyhow::ensure!(
        work.kind == WorkKind::Question
            && work.key.starts_with("task:manager-triage:")
            && work.parent_uid.as_deref() == Some(recovery.source_uid.as_str())
            && work.feature_uid.is_none()
            && work.feature_id.is_none()
            && uuid::Uuid::parse_str(&recovery.source_uid).is_ok()
            && recovery
                .expected_feature_uid
                .as_ref()
                .is_none_or(|uid| uuid::Uuid::parse_str(uid).is_ok())
            && !recovery.source_key.is_empty()
            && recovery.source_key.len() <= 512
            && !recovery.reply_id.is_empty()
            && recovery.reply_id.len() <= 512
            && recovery.evidence.len() <= 16 * 1024
            && recovery.investigation_claims <= 2
            && recovery.attempts <= 3
            && recovery.repair_attempts <= 2,
        "Operational recovery metadata is invalid or does not name its original task"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn recovery_identity_is_stable_and_metadata_cannot_retarget_parent() {
        let source = Work::new(
            "task:original".into(),
            "Original goal".into(),
            "Preserve intent".into(),
            String::new(),
        );
        let first = task(
            &source,
            "reply-one",
            FailureKind::FeatureIdentity,
            "Exact diagnostic".into(),
        );
        let second = task(
            &source,
            "reply-one",
            FailureKind::Execution,
            "Updated diagnostic".into(),
        );
        assert_eq!(first.uid, second.uid);
        assert_ne!(
            first.uid,
            task(&source, "reply-two", FailureKind::Execution, String::new()).uid
        );
        validate(&first, first.operational_recovery.as_ref().unwrap()).unwrap();
        let mut tampered = first.clone();
        tampered.parent_uid = Some(uuid::Uuid::new_v4().to_string());
        assert!(validate(&tampered, tampered.operational_recovery.as_ref().unwrap()).is_err());
    }
}
