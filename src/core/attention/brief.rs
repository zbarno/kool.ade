use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct OptionBrief {
    pub id: String,
    pub label: String,
    pub meaning: String,
    pub consequence: String,
    #[serde(default)]
    pub source_evidence: Option<String>,
    /// Clarification requests never authorize or resolve a decision.
    #[serde(default)]
    pub clarification: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HumanStep {
    pub owner: String,
    pub action: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Recommendation {
    pub option_id: String,
    pub rationale: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Brief {
    pub problem: String,
    #[serde(default)]
    pub recommendation: Option<Recommendation>,
    #[serde(default)]
    pub options: Vec<OptionBrief>,
    #[serde(default)]
    pub steps: Vec<HumanStep>,
    pub after: String,
}

impl Brief {
    /// The action pane carries the question, options, and supporting context.
    /// Keep new conversation openings brief; historical messages stay intact.
    pub fn conversation_message(&self) -> String {
        if !self.options.is_empty() {
            "I need your choice before I continue. Ask me if anything is unclear.".into()
        } else if !self.steps.is_empty() {
            "I need your help before I continue. Ask me if anything is unclear.".into()
        } else {
            "You can ask me about this task.".into()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conversation_opening_is_concise_and_does_not_duplicate_answer_cards() {
        let brief = Brief {
            problem: "The provider quota stopped this task.".into(),
            recommendation: None,
            options: vec![OptionBrief {
                id: "wait".into(),
                label: "Wait for reset".into(),
                meaning: "Continue tomorrow.".into(),
                consequence: "The task remains paused until then.".into(),
                clarification: false,
                source_evidence: None,
            }],
            steps: vec![HumanStep {
                owner: "Account owner".into(),
                action: "Choose whether to wait or request more capacity.".into(),
            }],
            after: "Resume implementation when capacity is available.".into(),
        };
        let message = brief.conversation_message();
        assert!(message.contains("I need your choice"));
        assert!(message.chars().count() < 100);
        assert!(!message.contains(&brief.problem));
        assert!(!message.contains("Wait for reset"));
    }
}
