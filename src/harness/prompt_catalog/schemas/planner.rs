use serde::{Deserialize, Serialize};
use std::collections::HashSet;

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PlannerOutputV1 {
    pub proposed_questions: Vec<PlannerQuestionV1>,
    pub draft_changes: Vec<String>,
    pub readiness: ReadinessV1,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PlannerQuestionV1 {
    pub question: String,
    pub why_it_matters: String,
    pub options: Vec<String>,
    pub recommendation: String,
    pub impact_of_no_answer: String,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ReadinessV1 {
    pub ready: bool,
    pub blockers: Vec<String>,
    pub evidence: Vec<String>,
}

/// Planner v3 proposal shape. Feature identity and plan revision are supplied
/// by the application when it creates persisted clarification cards.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PlannerOutputV2 {
    pub proposed_questions: Vec<ClarificationProposalV1>,
    pub draft_changes: Vec<String>,
    pub readiness: ReadinessV2,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "PascalCase")]
pub enum ClarificationCategory {
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

#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "PascalCase")]
pub enum ClarificationPriority {
    Blocking,
    Important,
    Optional,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ClarificationOptionV1 {
    pub id: String,
    pub label: String,
    pub tradeoffs: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ClarificationProposalV1 {
    pub decision_subject: String,
    pub decision_domain: String,
    pub origin_requirement_ids: Vec<String>,
    pub category: ClarificationCategory,
    pub priority: ClarificationPriority,
    pub question: String,
    pub reason: String,
    pub options: Vec<ClarificationOptionV1>,
    pub recommended_option_id: Option<String>,
    pub no_answer_impact: String,
    pub evidence_refs: Vec<String>,
    pub assumption_if_deferred: Option<String>,
    pub depends_on_question_ids: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReadinessV2 {
    pub ready: bool,
    pub blockers: Vec<String>,
    pub evidence: Vec<String>,
    pub assessed_axes: Vec<String>,
    pub acceptance_tests: Vec<AcceptanceTestRefV1>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AcceptanceTestRefV1 {
    pub requirement_id: String,
    pub test_ref: String,
}

impl PlannerOutputV2 {
    pub fn validate(&self) -> Result<(), String> {
        let bounded = |text: &str, limit: usize| !text.trim().is_empty() && text.len() <= limit;
        if self.proposed_questions.len() > 64
            || self.draft_changes.len() > 128
            || self.readiness.blockers.len() > 128
            || self.readiness.evidence.len() > 128
            || self.readiness.assessed_axes.len() > 32
            || self.readiness.acceptance_tests.len() > 256
        {
            return Err("planner output exceeds collection limits".into());
        }
        for question in &self.proposed_questions {
            if !bounded(&question.decision_subject, 2048)
                || !bounded(&question.decision_domain, 256)
                || !bounded(&question.question, 4096)
                || !bounded(&question.reason, 8192)
                || !bounded(&question.no_answer_impact, 4096)
                || question.options.len() < 2
                || question.options.len() > 32
                || question.origin_requirement_ids.len() > 256
                || question.evidence_refs.is_empty()
                || question.evidence_refs.len() > 128
                || question.depends_on_question_ids.len() > 128
                || question
                    .origin_requirement_ids
                    .iter()
                    .any(|v| !bounded(v, 256))
                || question.evidence_refs.iter().any(|v| !bounded(v, 128))
                || question
                    .depends_on_question_ids
                    .iter()
                    .any(|v| !bounded(v, 128))
                || question
                    .assumption_if_deferred
                    .as_ref()
                    .is_some_and(|v| !bounded(v, 4096))
            {
                return Err("clarification proposal is incomplete or exceeds bounds".into());
            }
            let mut option_ids = HashSet::new();
            for option in &question.options {
                if !bounded(&option.id, 128)
                    || !bounded(&option.label, 1024)
                    || option.tradeoffs.len() > 4096
                    || !option_ids.insert(option.id.clone())
                {
                    return Err("clarification options must be unique and bounded".into());
                }
            }
            if question
                .recommended_option_id
                .as_ref()
                .is_some_and(|id| !option_ids.contains(id))
            {
                return Err("recommended clarification option is missing".into());
            }
        }
        if self.draft_changes.iter().any(|v| !bounded(v, 4096))
            || self.readiness.blockers.iter().any(|v| !bounded(v, 4096))
            || self
                .readiness
                .assessed_axes
                .iter()
                .any(|v| !bounded(v, 256))
            || self
                .readiness
                .acceptance_tests
                .iter()
                .any(|test| !bounded(&test.requirement_id, 256) || !bounded(&test.test_ref, 1024))
        {
            return Err("planner readiness fields are incomplete or exceed bounds".into());
        }
        Ok(())
    }
}
