use serde::{Deserialize, Serialize};

use super::types::{FeaturePhase, GateActor, GateDecision, GateReceipt, is_sha256, is_uuid};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FeatureGoal {
    pub schema_version: u16,
    pub feature_uid: String,
    pub original_intent: String,
    pub draft: String,
    pub scope: Vec<String>,
    pub non_goals: Vec<String>,
    pub revision: u64,
    pub phase: FeaturePhase,
    pub approved_plan_sha256: Option<String>,
    /// Exact immutable approval snapshot, when bridged from the current plan workflow.
    #[serde(default)]
    pub plan_snapshot_uid: Option<String>,
    #[serde(default)]
    pub plan_snapshot_sha256: Option<String>,
    pub unresolved_questions: Vec<String>,
    pub decision_refs: Vec<String>,
    pub review_refs: Vec<String>,
    /// Durable evidence that required questions and acceptance criteria have
    /// been checked before this exact draft is offered for approval.
    #[serde(default)]
    pub readiness_evidence_refs: Vec<String>,
    /// Task identities atomically added to the approved plan's graph.
    #[serde(default)]
    pub task_uids: Vec<String>,
}

impl FeatureGoal {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema_version != 1
            || !is_uuid(&self.feature_uid)
            || self.original_intent.trim().is_empty()
            || self.original_intent.len() > 32 * 1024
            || self.original_intent.contains('\0')
            || self.draft.len() > 128 * 1024
            || self.draft.contains('\0')
            || self.scope.len() > 256
            || self.non_goals.len() > 256
            || self.unresolved_questions.len() > 256
            || self.decision_refs.len() > 256
            || self.review_refs.len() > 256
            || self.readiness_evidence_refs.len() > 256
            || self.task_uids.len() > 256
            || self
                .scope
                .iter()
                .chain(self.non_goals.iter())
                .chain(self.unresolved_questions.iter())
                .chain(self.decision_refs.iter())
                .chain(self.review_refs.iter())
                .chain(self.readiness_evidence_refs.iter())
                .any(|item| !valid_evidence_ref(item))
            || self.task_uids.iter().any(|uid| !is_uuid(uid))
            || self
                .task_uids
                .iter()
                .enumerate()
                .any(|(index, uid)| self.task_uids[..index].iter().any(|prior| prior == uid))
            || self
                .approved_plan_sha256
                .as_deref()
                .is_some_and(|sha| !is_sha256(sha))
            || self
                .plan_snapshot_uid
                .as_deref()
                .is_some_and(|uid| !is_uuid(uid))
            || self
                .plan_snapshot_sha256
                .as_deref()
                .is_some_and(|sha| !is_sha256(sha))
            || self.plan_snapshot_uid.is_some() != self.plan_snapshot_sha256.is_some()
            || self.plan_snapshot_sha256.as_deref().is_some_and(|hash| {
                self.approved_plan_sha256
                    .as_deref()
                    .is_some_and(|approved| approved != hash)
            })
        {
            return Err("feature goal is invalid or exceeds limits".into());
        }
        let needs_plan = matches!(
            self.phase,
            FeaturePhase::AwaitingPlanConfirmation
                | FeaturePhase::PlanReviewPending
                | FeaturePhase::ReadyToDecompose
                | FeaturePhase::Decomposing
                | FeaturePhase::Executing
                | FeaturePhase::Complete
        );
        if (needs_plan && self.approved_plan_sha256.is_none())
            || (matches!(
                self.phase,
                FeaturePhase::Exploring | FeaturePhase::AwaitingAnswers | FeaturePhase::PlanRework
            ) && self.approved_plan_sha256.is_some())
            || (self.phase == FeaturePhase::AwaitingAnswers && self.unresolved_questions.is_empty())
        {
            return Err("feature phase does not match its plan and question state".into());
        }
        Ok(())
    }

    /// Applies one proposal only when it is legal for the current phase and revision.
    pub fn apply(&mut self, expected_revision: u64, event: FeatureEvent) -> Result<(), String> {
        self.validate()?;
        if self.revision != expected_revision {
            return Err("feature revision is stale".into());
        }
        if self.revision == u64::MAX {
            return Err("feature revision is exhausted".into());
        }
        match event {
            FeatureEvent::QuestionsRaised { questions }
                if self.phase == FeaturePhase::Exploring
                    && !questions.is_empty()
                    && questions.len() <= 256
                    && questions.iter().all(|item| {
                        !item.trim().is_empty() && item.len() <= 2048 && !item.contains('\0')
                    }) =>
            {
                self.unresolved_questions = questions;
                self.phase = FeaturePhase::AwaitingAnswers;
            }
            FeatureEvent::AnswersResolved if self.phase == FeaturePhase::AwaitingAnswers => {
                self.unresolved_questions.clear();
                self.phase = FeaturePhase::Exploring;
            }
            FeatureEvent::DraftUpdated {
                draft,
                scope,
                non_goals,
            } if matches!(
                self.phase,
                FeaturePhase::Exploring | FeaturePhase::PlanRework
            ) && draft.len() <= 128 * 1024
                && scope.len() <= 256
                && non_goals.len() <= 256
                && !draft.trim().is_empty()
                && scope
                    .iter()
                    .all(|item| !item.trim().is_empty() && item.len() <= 2048)
                && non_goals
                    .iter()
                    .all(|item| !item.trim().is_empty() && item.len() <= 2048) =>
            {
                self.draft = draft;
                self.scope = scope;
                self.non_goals = non_goals;
                self.approved_plan_sha256 = None;
                self.plan_snapshot_uid = None;
                self.plan_snapshot_sha256 = None;
                self.readiness_evidence_refs.clear();
                self.phase = FeaturePhase::Exploring;
            }
            FeatureEvent::ReadinessRecorded { evidence_refs }
                if matches!(
                    self.phase,
                    FeaturePhase::Exploring | FeaturePhase::PlanRework
                ) && self.unresolved_questions.is_empty()
                    && !evidence_refs.is_empty()
                    && evidence_refs.len() <= 256
                    && evidence_refs.iter().all(|item| valid_evidence_ref(item)) =>
            {
                self.readiness_evidence_refs = evidence_refs;
                self.phase = FeaturePhase::Exploring;
            }
            FeatureEvent::PlanReady { plan_sha256 }
                if matches!(
                    self.phase,
                    FeaturePhase::Exploring | FeaturePhase::PlanRework
                ) && self.unresolved_questions.is_empty()
                    && is_sha256(&plan_sha256)
                    && !self.readiness_evidence_refs.is_empty()
                    && self.plan_sha256() == plan_sha256 =>
            {
                self.approved_plan_sha256 = Some(plan_sha256);
                self.phase = FeaturePhase::AwaitingPlanConfirmation;
            }
            FeatureEvent::UserPlanDecision(receipt)
                if self.phase == FeaturePhase::AwaitingPlanConfirmation
                    && receipt.actor == GateActor::User
                    && receipt.subject_uid
                        == self
                            .plan_snapshot_uid
                            .as_deref()
                            .unwrap_or(&self.feature_uid)
                    && self.approved_plan_sha256.as_deref()
                        == Some(receipt.subject_sha256.as_str())
                    && receipt.validate().is_ok() =>
            {
                if self.decision_refs.len() == 256 {
                    return Err("feature decision history exceeds its limit".into());
                }
                self.decision_refs.push(receipt.receipt_uid.clone());
                if receipt.decision == GateDecision::Approved {
                    self.phase = FeaturePhase::PlanReviewPending;
                } else if matches!(
                    receipt.decision,
                    GateDecision::Rejected | GateDecision::ChangesRequested
                ) {
                    self.approved_plan_sha256 = None;
                    self.phase = FeaturePhase::Exploring;
                } else {
                    return Err("user cannot mark plan approval as not applicable".into());
                }
            }
            FeatureEvent::PlanReview(receipt)
                if self.phase == FeaturePhase::PlanReviewPending
                    && receipt.actor == GateActor::IndependentReviewer
                    && receipt.prompt_role()
                        == Some(crate::harness::prompt_catalog::PromptRole::PlanReviewer)
                    && receipt.subject_uid
                        == self
                            .plan_snapshot_uid
                            .as_deref()
                            .unwrap_or(&self.feature_uid)
                    && self.approved_plan_sha256.as_deref()
                        == Some(receipt.subject_sha256.as_str())
                    && receipt.validate().is_ok() =>
            {
                if receipt.decision == GateDecision::Approved {
                    if self.review_refs.len() == 256 {
                        return Err("feature review history exceeds its limit".into());
                    }
                    self.phase = FeaturePhase::ReadyToDecompose;
                    self.review_refs.push(receipt.receipt_uid);
                } else {
                    self.phase = FeaturePhase::PlanRework;
                    self.approved_plan_sha256 = None;
                }
            }
            FeatureEvent::TaskGraphCreated { plan_sha256 }
                if self.phase == FeaturePhase::ReadyToDecompose
                    && self.approved_plan_sha256.as_deref() == Some(plan_sha256.as_str()) =>
            {
                self.phase = FeaturePhase::Executing;
            }
            FeatureEvent::TasksCompleted if self.phase == FeaturePhase::Executing => {
                self.phase = FeaturePhase::Complete;
            }
            FeatureEvent::Cancel
                if !matches!(self.phase, FeaturePhase::Complete | FeaturePhase::Cancelled) =>
            {
                self.phase = FeaturePhase::Cancelled;
            }
            _ => return Err("feature lifecycle transition is not allowed".into()),
        }
        self.revision += 1;
        self.validate()
    }

    /// Hashes the canonical plan content that user and reviewer receipts bind.
    pub fn plan_sha256(&self) -> String {
        if let Some(hash) = &self.plan_snapshot_sha256 {
            return hash.clone();
        }
        use sha2::Digest as _;
        let bytes = serde_json::to_vec(&(1_u16, &self.draft, &self.scope, &self.non_goals))
            .expect("serializing bounded plan fields cannot fail");
        format!("{:x}", sha2::Sha256::digest(bytes))
    }
}

fn valid_evidence_ref(value: &str) -> bool {
    !value.trim().is_empty() && value.len() <= 2048 && !value.contains('\0')
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FeatureEvent {
    QuestionsRaised {
        questions: Vec<String>,
    },
    AnswersResolved,
    DraftUpdated {
        draft: String,
        scope: Vec<String>,
        non_goals: Vec<String>,
    },
    ReadinessRecorded {
        evidence_refs: Vec<String>,
    },
    PlanReady {
        plan_sha256: String,
    },
    UserPlanDecision(GateReceipt),
    PlanReview(GateReceipt),
    TaskGraphCreated {
        plan_sha256: String,
    },
    TasksCompleted,
    Cancel,
}
