use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FeaturePhase {
    Exploring,
    AwaitingAnswers,
    AwaitingPlanConfirmation,
    PlanReviewPending,
    PlanRework,
    ReadyToDecompose,
    Decomposing,
    Executing,
    Complete,
    Cancelled,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskPhase {
    Ready,
    Blocked,
    Running,
    AwaitingReview,
    Repairing,
    ReReview,
    AwaitingRuntimeQa,
    AwaitingSpecReconciliation,
    Done,
    PrPending,
    PrOpened,
    PrMerged,
    Failed,
    Cancelled,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GateActor {
    User,
    IndependentReviewer,
    ApplicationPolicy,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GateDecision {
    Approved,
    Rejected,
    ChangesRequested,
    NotApplicable,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GateReceipt {
    pub receipt_uid: String,
    pub subject_uid: String,
    pub subject_sha256: String,
    pub actor: GateActor,
    pub decision: GateDecision,
    pub recorded_at_unix_ms: u64,
    pub evidence_refs: Vec<String>,
    /// Hash of the exact versioned prompt manifest used by independent review.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt_sha256: Option<String>,
    /// Template and schema identity from which `prompt_sha256` is derived.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt_manifest: Option<crate::harness::prompt_catalog::PromptManifest>,
    pub reviewer_run_id: Option<String>,
    pub author_run_id: Option<String>,
    pub reviewer_session_id: Option<String>,
    pub author_session_id: Option<String>,
}

impl GateReceipt {
    pub fn prompt_role(&self) -> Option<crate::harness::prompt_catalog::PromptRole> {
        self.prompt_manifest.as_ref().and_then(|manifest| {
            crate::harness::prompt_catalog::validate_prompt_manifest(manifest).ok()
        })
    }

    pub fn validate(&self) -> Result<(), String> {
        if !is_uuid(&self.receipt_uid)
            || !is_uuid(&self.subject_uid)
            || !is_sha256(&self.subject_sha256)
            || self.recorded_at_unix_ms == 0
            || self.evidence_refs.len() > 128
            || self
                .evidence_refs
                .iter()
                .any(|item| item.trim().is_empty() || item.len() > 2048 || item.contains('\0'))
        {
            return Err("gate receipt is invalid or exceeds limits".into());
        }
        match self.actor {
            GateActor::User
                if self.reviewer_run_id.is_some()
                    || self.author_run_id.is_some()
                    || self.reviewer_session_id.is_some()
                    || self.author_session_id.is_some()
                    || self.decision == GateDecision::NotApplicable =>
            {
                Err("user approval cannot be attributed to an agent run".into())
            }
            GateActor::IndependentReviewer => {
                let distinct_runs =
                    distinct_ids(self.reviewer_run_id.as_ref(), self.author_run_id.as_ref());
                let distinct_sessions = distinct_ids(
                    self.reviewer_session_id.as_ref(),
                    self.author_session_id.as_ref(),
                );
                if !distinct_runs
                    || !distinct_sessions
                    || !self.prompt_sha256.as_deref().is_some_and(is_sha256)
                    || !self.prompt_manifest.as_ref().is_some_and(|manifest| {
                        crate::harness::prompt_catalog::validate_prompt_manifest(manifest)
                            .is_ok_and(|role| {
                                matches!(
                                    role,
                                    crate::harness::prompt_catalog::PromptRole::PlanReviewer
                                        | crate::harness::prompt_catalog::PromptRole::CodeReviewer
                                        | crate::harness::prompt_catalog::PromptRole::RuntimeQaReviewer
                                ) && self.prompt_sha256.as_deref()
                                    == Some(manifest.identity_sha256().as_str())
                            })
                    })
                    || self.evidence_refs.is_empty()
                    || self.decision == GateDecision::NotApplicable
                {
                    Err("independent review requires evidence, distinct run/session identities, and a current prompt hash".into())
                } else {
                    Ok(())
                }
            }
            GateActor::ApplicationPolicy => {
                if self.evidence_refs.is_empty() {
                    Err("application gate requires evidence references".into())
                } else {
                    Ok(())
                }
            }
            _ => Ok(()),
        }
    }
}

pub(super) fn is_uuid(value: &str) -> bool {
    uuid::Uuid::parse_str(value).is_ok_and(|id| id.hyphenated().to_string() == value)
}

pub(super) fn is_sha256(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

pub(super) fn is_git_oid(value: &str) -> bool {
    matches!(value.len(), 40 | 64) && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn distinct_ids(left: Option<&String>, right: Option<&String>) -> bool {
    left.zip(right)
        .is_some_and(|(left, right)| is_uuid(left) && is_uuid(right) && left != right)
}
