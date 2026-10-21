use crate::core::lifecycle::{GateActor, GateDecision, GateReceipt};
use serde::{Deserialize, Serialize};

/// Immutable publication inputs captured when the user authorizes a PR.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PrPublicationScopeV1 {
    pub schema_version: u16,
    pub publication_generation: u64,
    pub source_repository_id: String,
    pub source_sha: String,
    pub implementation_branch: String,
    pub destination_branch: String,
    pub expected_base_sha: String,
    pub reviewed_diff_hash: String,
    pub included_task_uids: Vec<String>,
    pub accepted_spec_revision: u64,
    pub task_evidence: Vec<PrPublicationTaskEvidenceV1>,
}

/// Completion gates copied into the frozen publication scope for each parent
/// or child task included by the one PR.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PrPublicationTaskEvidenceV1 {
    pub task_uid: String,
    pub task_revision: u64,
    pub reviewed_head_sha: String,
    pub reviewed_diff_hash: String,
    pub code_review_receipt: GateReceipt,
    pub qa_receipt: GateReceipt,
    pub spec_sync_receipt: GateReceipt,
}

impl PrPublicationScopeV1 {
    pub(crate) fn validate(&self, work_item_tasks: &[String]) -> Result<(), String> {
        if self.schema_version != 1
            || self.publication_generation == 0
            || self.source_repository_id.trim().is_empty()
            || self.source_repository_id.len() > 128
            || self.source_repository_id.contains('\0')
            || !crate::core::lifecycle::types::is_git_oid(&self.source_sha)
            || !valid_branch(&self.implementation_branch)
            || !valid_branch(&self.destination_branch)
            || !crate::core::lifecycle::types::is_git_oid(&self.expected_base_sha)
            || !crate::core::lifecycle::types::is_sha256(&self.reviewed_diff_hash)
            || self.accepted_spec_revision == 0
            || self.included_task_uids != work_item_tasks
            || self.task_evidence.len() != work_item_tasks.len()
            || self
                .task_evidence
                .iter()
                .zip(work_item_tasks)
                .any(|(evidence, task_uid)| {
                    evidence.task_uid != *task_uid
                        || evidence.task_revision == 0
                        || !crate::core::lifecycle::types::is_git_oid(&evidence.reviewed_head_sha)
                        || !crate::core::lifecycle::types::is_sha256(&evidence.reviewed_diff_hash)
                        || !valid_receipt(
                            &evidence.code_review_receipt,
                            task_uid,
                            &evidence.reviewed_diff_hash,
                            GateActor::IndependentReviewer,
                            GateDecision::Approved,
                        )
                        || !matches!(
                            evidence.qa_receipt.actor,
                            GateActor::IndependentReviewer | GateActor::ApplicationPolicy
                        )
                        || (evidence.qa_receipt.decision == GateDecision::NotApplicable
                            && evidence.qa_receipt.actor != GateActor::ApplicationPolicy)
                        || !valid_receipt(
                            &evidence.qa_receipt,
                            task_uid,
                            &evidence.reviewed_diff_hash,
                            evidence.qa_receipt.actor,
                            evidence.qa_receipt.decision,
                        )
                        || !matches!(
                            evidence.qa_receipt.decision,
                            GateDecision::Approved | GateDecision::NotApplicable
                        )
                        || !valid_receipt(
                            &evidence.spec_sync_receipt,
                            task_uid,
                            &evidence.reviewed_diff_hash,
                            GateActor::ApplicationPolicy,
                            GateDecision::Approved,
                        )
                })
        {
            return Err("PR publication scope is invalid or differs from its task gates".into());
        }
        Ok(())
    }
}

fn valid_receipt(
    receipt: &GateReceipt,
    task_uid: &str,
    diff_hash: &str,
    actor: GateActor,
    decision: GateDecision,
) -> bool {
    receipt.validate().is_ok()
        && receipt.subject_uid == task_uid
        && receipt.subject_sha256 == diff_hash
        && receipt.actor == actor
        && receipt.decision == decision
}

fn valid_branch(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 255
        && !value.starts_with('-')
        && !value.starts_with('/')
        && !value.ends_with('/')
        && !value.ends_with('.')
        && !value.contains("..")
        && !value.contains("@{")
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"-._/@".contains(&byte))
}
