//! Versioned task directions captured only by the explicit decision action.
use serde::{Deserialize, Serialize};
use sha2::Digest as _;

pub const MAX_TASK_DECISION_BYTES: usize = 8_000;

/// Atomically append a decision and supersede only its previous answer to the same blocker.
pub fn save_task_decision(
    store: &crate::artifacts::planning_store::PlanningStore,
    intent_uid: &str,
    mut decision: TaskDecisionRecordV1,
) -> anyhow::Result<()> {
    for _ in 0..3 {
        let revision = store.revision()?;
        let (mut work, _) = super::load_expected(store, &revision)?;
        let root = work
            .iter_mut()
            .find(|item| item.uid == intent_uid)
            .ok_or_else(|| anyhow::anyhow!("TaskIntent decision owner is missing"))?;
        anyhow::ensure!(
            root.task_intent_root_uid() == Some(intent_uid),
            "Task decision owner is not a TaskIntent root"
        );
        decision.created_revision = root.record_revision.saturating_add(1).max(1);
        for prior in &mut root.task_decisions {
            if prior.task_uid == decision.task_uid
                && prior.source_context_sha256 == decision.source_context_sha256
                && prior.status == TaskDecisionStatusV1::Active
            {
                prior.status = TaskDecisionStatusV1::Superseded;
            }
        }
        root.task_decisions.push(decision.clone());
        match super::save_expected(store, &work, &revision) {
            Ok(_) => return Ok(()),
            Err(error)
                if error.to_string().contains("changed (expected")
                    || error.to_string().contains("StaleRecordRevision") =>
            {
                continue;
            }
            Err(error) => return Err(error),
        }
    }
    anyhow::bail!("Task decisions changed concurrently; retry the implementation decision")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskDecisionStatusV1 {
    Active,
    Superseded,
    Revoked,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskDecisionSourceV1 {
    ExplicitImplementationDirection,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TaskDecisionAuthorityReceiptV1 {
    pub schema_version: u16,
    pub receipt_uid: String,
    pub task_uid: String,
    pub task_intent_uid: String,
    pub feature_uid: String,
    pub task_spec_sha256: String,
    pub approved_plan_sha256: String,
    pub source_context_sha256: String,
    pub source_message_id: String,
    pub subject_sha256: String,
    pub recorded_at_unix_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TaskDecisionRecordV1 {
    pub schema_version: u16,
    pub decision_uid: String,
    pub task_uid: String,
    pub task_intent_uid: String,
    pub task_intent_cycle_uid: Option<String>,
    pub feature_uid: String,
    pub task_spec_sha256: String,
    pub approved_plan_sha256: String,
    pub source_message_id: String,
    pub source_context_sha256: String,
    pub source: TaskDecisionSourceV1,
    pub authority: crate::core::lifecycle::GateActor,
    pub authorization_receipt: TaskDecisionAuthorityReceiptV1,
    pub affected_requirement_ids: Vec<String>,
    pub created_at_unix_ms: u64,
    pub created_revision: u64,
    pub status: TaskDecisionStatusV1,
    pub text: String,
}

impl TaskDecisionRecordV1 {
    pub fn validate(&self) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.schema_version == 1,
            "Unsupported task decision version"
        );
        anyhow::ensure!(
            self.authority == crate::core::lifecycle::GateActor::User,
            "A task direction must be explicitly authored by a user"
        );
        let receipt = &self.authorization_receipt;
        anyhow::ensure!(
            receipt.schema_version == 1
                && uuid::Uuid::parse_str(&receipt.receipt_uid).is_ok()
                && receipt.task_uid == self.task_uid
                && receipt.task_intent_uid == self.task_intent_uid
                && receipt.feature_uid == self.feature_uid
                && receipt.task_spec_sha256 == self.task_spec_sha256
                && receipt.approved_plan_sha256 == self.approved_plan_sha256
                && receipt.source_context_sha256 == self.source_context_sha256
                && receipt.source_message_id == self.source_message_id
                && receipt.subject_sha256
                    == format!("{:x}", sha2::Sha256::digest(self.text.as_bytes()))
                && receipt.recorded_at_unix_ms == self.created_at_unix_ms,
            "Task decision authority receipt does not match its source direction"
        );
        for uid in [
            &self.decision_uid,
            &self.task_uid,
            &self.task_intent_uid,
            &self.feature_uid,
        ] {
            anyhow::ensure!(
                uuid::Uuid::parse_str(uid).is_ok(),
                "Task decision has an invalid identity"
            );
        }
        if let Some(uid) = &self.task_intent_cycle_uid {
            anyhow::ensure!(
                uuid::Uuid::parse_str(uid).is_ok(),
                "Task decision has an invalid cycle identity"
            );
        }
        for hash in [
            &self.task_spec_sha256,
            &self.approved_plan_sha256,
            &self.source_context_sha256,
        ] {
            anyhow::ensure!(
                hash.len() == 64 && hash.bytes().all(|byte| byte.is_ascii_hexdigit()),
                "Task decision has an invalid binding hash"
            );
        }
        anyhow::ensure!(
            !self.source_message_id.trim().is_empty() && self.source_message_id.len() <= 128,
            "Task decision has an invalid source message"
        );
        anyhow::ensure!(
            self.created_at_unix_ms > 0 && self.created_revision > 0,
            "Task decision is missing its creation revision"
        );
        anyhow::ensure!(
            !self.text.trim().is_empty() && self.text.len() <= MAX_TASK_DECISION_BYTES,
            "Task decision is empty or too large"
        );
        anyhow::ensure!(
            self.affected_requirement_ids.len() <= 128,
            "Task decision has too many requirement references"
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests;

/// Produce a stable worker context from every active record bound to this exact task contract.
pub fn task_decision_context(
    records: &[TaskDecisionRecordV1],
    task_uid: &str,
    task_intent_uid: &str,
    task_intent_cycle_uid: Option<&str>,
    feature_uid: &str,
    task_spec_sha256: &str,
    approved_plan_sha256: &str,
) -> anyhow::Result<Option<String>> {
    let mut applicable = records
        .iter()
        .filter(|record| {
            record.task_uid == task_uid && record.status == TaskDecisionStatusV1::Active
        })
        .collect::<Vec<_>>();
    if applicable.is_empty() {
        return Ok(None);
    }
    for record in &applicable {
        record.validate()?;
        anyhow::ensure!(
            record.task_intent_uid == task_intent_uid
                && record.task_intent_cycle_uid.as_deref() == task_intent_cycle_uid
                && record.feature_uid == feature_uid,
            "Task decision scope does not match the current task identity"
        );
        anyhow::ensure!(
            record.task_spec_sha256 == task_spec_sha256
                && record.approved_plan_sha256 == approved_plan_sha256,
            "Task decision is stale for the current task or approved plan; resolve it before implementation"
        );
    }
    applicable.sort_by(|a, b| {
        a.created_revision
            .cmp(&b.created_revision)
            .then_with(|| a.decision_uid.cmp(&b.decision_uid))
    });
    let mut context = format!(
        "Validated task directions (user-authored; not scope approvals):\ntask_uid={task_uid}\ntask_intent_uid={task_intent_uid}\ncycle_uid={}\nfeature_uid={feature_uid}\ntask_spec_sha256={task_spec_sha256}\napproved_plan_sha256={approved_plan_sha256}\n",
        task_intent_cycle_uid.unwrap_or("none")
    );
    for record in applicable {
        use std::fmt::Write as _;
        let text_json = serde_json::to_string(&record.text)?;
        writeln!(
            context,
            "- decision={} receipt={} source={} source_context_sha256={} requirement_ids={} text_json={}",
            record.decision_uid,
            record.authorization_receipt.receipt_uid,
            record.source_message_id,
            record.source_context_sha256,
            if record.affected_requirement_ids.is_empty() {
                "none".into()
            } else {
                record.affected_requirement_ids.join(",")
            },
            text_json
        )
        .expect("writing to string cannot fail");
        anyhow::ensure!(
            context.len() <= MAX_TASK_DECISION_BYTES * 2,
            "Applicable task decisions exceed the worker context limit"
        );
    }
    Ok(Some(context))
}
