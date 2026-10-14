use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum QuestionCategory {
    Goal,
    Scope,
    Ux,
    Integration,
    Api,
    Data,
    Security,
    Privacy,
    Performance,
    Operations,
    Testing,
    FailureMode,
    Deployment,
}

impl QuestionCategory {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Goal => "Goal",
            Self::Scope => "Scope",
            Self::Ux => "UX",
            Self::Integration => "Integration",
            Self::Api => "API",
            Self::Data => "Data",
            Self::Security => "Security",
            Self::Privacy => "Privacy",
            Self::Performance => "Performance",
            Self::Operations => "Operations",
            Self::Testing => "Testing",
            Self::FailureMode => "Failure mode",
            Self::Deployment => "Deployment",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum QuestionPriority {
    Blocking,
    Important,
    Optional,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum QuestionStatus {
    Draft,
    Open,
    Answered,
    Declined,
    Superseded,
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(deny_unknown_fields)]
pub struct QuestionOption {
    pub id: String,
    pub label: String,
    pub tradeoffs: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(deny_unknown_fields)]
pub struct DeferredAssumption {
    pub assumption: String,
    pub owner: String,
    pub risk: String,
    pub verification_test: String,
    pub follow_up_task: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(deny_unknown_fields)]
pub struct QuestionV1 {
    pub uid: String,
    pub feature_uid: String,
    pub plan_revision: u64,
    pub decision_subject: String,
    pub decision_domain: String,
    pub origin_requirement_ids: Vec<String>,
    pub category: QuestionCategory,
    pub priority: QuestionPriority,
    pub question: String,
    pub reason: String,
    pub options: Vec<QuestionOption>,
    pub recommended_option_id: Option<String>,
    pub no_answer_impact: String,
    pub evidence_refs: Vec<String>,
    pub assumption_if_deferred: Option<String>,
    pub deferred: Option<DeferredAssumption>,
    pub depends_on_question_ids: Vec<String>,
    pub status: QuestionStatus,
    pub answer_text: Option<String>,
    pub selected_option: Option<String>,
    pub answered_by: Option<String>,
    pub answered_at_ms: Option<u64>,
    pub revision: u64,
}

impl QuestionV1 {
    pub fn validate(&self) -> Result<(), String> {
        if uuid::Uuid::parse_str(&self.uid).is_err()
            || uuid::Uuid::parse_str(&self.feature_uid).is_err()
            || self.uid
                != uuid::Uuid::parse_str(&self.uid)
                    .map(|id| id.hyphenated().to_string())
                    .unwrap_or_default()
            || self.feature_uid
                != uuid::Uuid::parse_str(&self.feature_uid)
                    .map(|id| id.hyphenated().to_string())
                    .unwrap_or_default()
        {
            return Err("question and feature IDs must be UUIDs".into());
        }
        if self.question.trim().is_empty()
            || self.decision_subject.trim().is_empty()
            || self.decision_domain.trim().is_empty()
            || self.question.len() > 4096
            || self.reason.len() > 8192
            || self.decision_subject.len() > 2048
            || self.decision_domain.len() > 256
            || self.no_answer_impact.len() > 4096
            || self.origin_requirement_ids.len() > 256
            || self.options.len() > 32
            || self.evidence_refs.len() > 128
            || self.depends_on_question_ids.len() > 128
            || self
                .answer_text
                .as_ref()
                .is_some_and(|value| value.trim().is_empty() || value.len() > 16_384)
            || self
                .answered_by
                .as_ref()
                .is_some_and(|value| value.trim().is_empty() || value.len() > 256)
        {
            return Err("question, subject, and domain are required".into());
        }
        if self.options.iter().any(|option| {
            option.id.trim().is_empty()
                || option.label.trim().is_empty()
                || option.id.len() > 128
                || option.label.len() > 1024
                || option.tradeoffs.len() > 4096
        }) {
            return Err("options require IDs and labels".into());
        }
        if self
            .recommended_option_id
            .as_ref()
            .is_some_and(|id| !self.options.iter().any(|option| &option.id == id))
        {
            return Err("recommended option is not present".into());
        }
        if self
            .selected_option
            .as_ref()
            .is_some_and(|id| !self.options.iter().any(|option| &option.id == id))
        {
            return Err("selected option is not present".into());
        }
        if self.deferred.is_some() && self.priority != QuestionPriority::Optional {
            return Err("only optional questions may be deferred".into());
        }
        let resolved = matches!(
            self.status,
            QuestionStatus::Answered | QuestionStatus::Declined
        );
        let declined = self.status == QuestionStatus::Declined;
        if resolved != self.answered_at_ms.is_some()
            || resolved != self.answered_by.is_some()
            || (!resolved && (self.answer_text.is_some() || self.selected_option.is_some()))
            || (self.status == QuestionStatus::Answered
                && self.answer_text.is_none()
                && self.selected_option.is_none())
            || (declined && (self.answer_text.is_some() || self.selected_option.is_some()))
        {
            return Err("question answer fields do not match its status".into());
        }
        if let Some(deferred) = &self.deferred
            && [
                deferred.assumption.as_str(),
                deferred.owner.as_str(),
                deferred.risk.as_str(),
                deferred.verification_test.as_str(),
                deferred.follow_up_task.as_str(),
            ]
            .iter()
            .any(|value| value.trim().is_empty() || value.len() > 2048)
        {
            return Err(
                "deferred assumptions require bounded owner, risk, test, and follow-up".into(),
            );
        }
        if self
            .origin_requirement_ids
            .iter()
            .any(|value| value.trim().is_empty() || value.len() > 256)
            || self
                .evidence_refs
                .iter()
                .any(|value| value.trim().is_empty() || value.len() > 2048)
            || self
                .depends_on_question_ids
                .iter()
                .any(|value| uuid::Uuid::parse_str(value).is_err())
        {
            return Err("question references are invalid or exceed limits".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(deny_unknown_fields)]
pub struct QuestionProposal {
    pub feature_uid: String,
    pub plan_revision: u64,
    pub decision_subject: String,
    pub decision_domain: String,
    pub origin_requirement_ids: Vec<String>,
    pub category: QuestionCategory,
    pub priority: QuestionPriority,
    pub question: String,
    pub reason: String,
    pub options: Vec<QuestionOption>,
    pub recommended_option_id: Option<String>,
    pub no_answer_impact: String,
    pub evidence_refs: Vec<String>,
    pub assumption_if_deferred: Option<String>,
    pub depends_on_question_ids: Vec<String>,
}
