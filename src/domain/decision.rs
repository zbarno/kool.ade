//! Evidence-aware choices attached to an unresolved planning item.
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConfidenceLevel {
    Low,
    Medium,
    High,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DecisionConfidence {
    pub level: ConfidenceLevel,
    pub explanation: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DecisionRecommendation {
    #[serde(alias = "option_id")]
    pub option_id: String,
    pub rationale: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DecisionOption {
    pub id: String,
    pub label: String,
    pub summary: String,
    #[serde(default)]
    pub benefits: Vec<String>,
    #[serde(default)]
    pub costs: Vec<String>,
    #[serde(default)]
    pub risks: Vec<String>,
    #[serde(default)]
    pub consequences: Vec<String>,
    #[serde(default)]
    pub reversibility: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DecisionBrief {
    /// Packet binds this to the containing open-item ID after validation.
    #[serde(default)]
    pub id: String,
    pub question: String,
    #[serde(alias = "why_now")]
    pub why_now: String,
    #[serde(default)]
    pub recommendation: Option<DecisionRecommendation>,
    #[serde(default)]
    pub confidence: Option<DecisionConfidence>,
    #[serde(default)]
    pub options: Vec<DecisionOption>,
    #[serde(default)]
    pub benefits: Vec<String>,
    #[serde(default)]
    pub costs: Vec<String>,
    #[serde(default)]
    pub risks: Vec<String>,
    #[serde(default)]
    pub ramifications: Vec<String>,
    #[serde(default)]
    pub reversibility: String,
    #[serde(default)]
    #[serde(alias = "defer_consequence")]
    pub defer_consequence: String,
    #[serde(default)]
    pub evidence: Vec<String>,
    /// Model-generated assessment used only when a Review decision is approved.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub adr_assessment: Option<AdrAssessment>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AdrAssessment {
    /// Whether the approved choice is durable and material enough for an ADR.
    pub create: bool,
    /// Concise ADR title; required only when `create` is true.
    #[serde(default)]
    pub title: String,
    /// Issue-specific reason for creating or omitting a durable record.
    pub rationale: String,
    /// Concrete signals that should make this decision worth revisiting.
    #[serde(default)]
    pub revisit_when: Vec<String>,
}

impl DecisionBrief {
    pub fn bind_to_item(&mut self, item_id: &str) -> Result<(), String> {
        if !self.id.is_empty() && self.id != item_id {
            return Err("decision brief ID must match its containing open item".into());
        }
        self.id = item_id.to_owned();
        self.validate()
    }

    pub fn validate(&self) -> Result<(), String> {
        if !self.id.is_empty() {
            check_text("decision ID", &self.id, 80)?;
        }
        check_text("decision question", &self.question, 2000)?;
        check_text("decision timing", &self.why_now, 1200)?;
        if self.options.len() == 1 {
            return Err("a decision brief must have zero or multiple real options".into());
        }
        let mut ids = BTreeSet::new();
        for option in &self.options {
            check_text("option ID", &option.id, 48)?;
            if !option
                .id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
                || !ids.insert(option.id.to_ascii_lowercase())
            {
                return Err("decision options need unique short IDs".into());
            }
            check_text("option label", &option.label, 160)?;
            check_text("option summary", &option.summary, 900)?;
            check_list("option benefits", &option.benefits)?;
            check_list("option costs", &option.costs)?;
            check_list("option risks", &option.risks)?;
            check_list("option consequences", &option.consequences)?;
            check_text("option reversibility", &option.reversibility, 900)?;
            if option.costs.is_empty() && option.risks.is_empty() && option.consequences.is_empty()
            {
                return Err(format!("option {} lacks a consequence or risk", option.id));
            }
        }
        if let Some(recommendation) = &self.recommendation {
            if !ids.contains(&recommendation.option_id.to_ascii_lowercase()) {
                return Err("recommendation must refer to a listed option".into());
            }
            check_text("recommendation rationale", &recommendation.rationale, 1200)?;
        }
        if let Some(confidence) = &self.confidence {
            check_text("confidence explanation", &confidence.explanation, 900)?;
        }
        check_list("decision benefits", &self.benefits)?;
        check_list("decision costs", &self.costs)?;
        check_list("decision risks", &self.risks)?;
        check_list("decision ramifications", &self.ramifications)?;
        check_list("decision evidence", &self.evidence)?;
        check_text("reversibility", &self.reversibility, 1200)?;
        check_text("deferring this decision", &self.defer_consequence, 1200)?;
        if let Some(assessment) = &self.adr_assessment {
            check_text("ADR assessment rationale", &assessment.rationale, 1200)?;
            check_list("ADR revisit conditions", &assessment.revisit_when)?;
            if assessment.create {
                check_text("ADR title", &assessment.title, 160)?;
                if self.options.len() < 2 || self.recommendation.is_none() {
                    return Err(
                        "a material ADR needs at least two alternatives and an approved recommendation".into(),
                    );
                }
                if assessment.revisit_when.is_empty() {
                    return Err("a material ADR needs a concrete revisit condition".into());
                }
            }
        }
        Ok(())
    }
}

fn check_list(name: &str, values: &[String]) -> Result<(), String> {
    if values.len() > 24 {
        return Err(format!("{name} contains too many entries"));
    }
    for value in values {
        check_text(name, value, 900)?;
    }
    Ok(())
}

fn check_text(name: &str, value: &str, max: usize) -> Result<(), String> {
    if value.trim().is_empty() || value.chars().count() > max {
        return Err(format!(
            "{name} must be present and no longer than {max} characters"
        ));
    }
    Ok(())
}

#[cfg(test)]
#[path = "decision_tests.rs"]
mod tests;
