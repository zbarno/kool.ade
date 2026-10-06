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
    /// Renders the generated blocker explanation in the structure used by
    /// task conversations, preserving the actions and consequences in the brief.
    pub fn conversation_message(&self) -> String {
        let mut needed = self
            .steps
            .iter()
            .map(|step| format!("- {}: {}", step.owner, step.action))
            .collect::<Vec<_>>();
        needed.extend(self.options.iter().map(|option| {
            format!(
                "- {}: {} Consequence: {}",
                option.label, option.meaning, option.consequence
            )
        }));
        if needed.is_empty() {
            needed.push("Review the task details and take the action described above.".into());
        }
        if let Some(recommendation) = &self.recommendation {
            let label = self
                .options
                .iter()
                .find(|option| option.id == recommendation.option_id)
                .map(|option| option.label.as_str())
                .unwrap_or("the recommended option");
            needed.push(format!(
                "Kool.ad/e recommends {label}: {}",
                recommendation.rationale
            ));
        }
        format!(
            "## What Happened\n{}\n\n## WHAT IS NEEDED OF THE USER\n{}\n\n## NEXT STEPS\n{}",
            self.problem,
            needed.join("\n"),
            self.after
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conversation_message_uses_required_sections_and_specific_user_action() {
        let brief = Brief {
            problem: "The provider quota stopped this task.".into(),
            recommendation: None,
            options: vec![OptionBrief {
                id: "wait".into(),
                label: "Wait for reset".into(),
                meaning: "Continue tomorrow.".into(),
                consequence: "The task remains paused until then.".into(),
                source_evidence: None,
            }],
            steps: vec![HumanStep {
                owner: "Account owner".into(),
                action: "Choose whether to wait or request more capacity.".into(),
            }],
            after: "Resume implementation when capacity is available.".into(),
        };
        let message = brief.conversation_message();
        for expected in [
            "## What Happened",
            "## WHAT IS NEEDED OF THE USER",
            "## NEXT STEPS",
            "Account owner: Choose whether to wait or request more capacity.",
            "The task remains paused until then.",
        ] {
            assert!(message.contains(expected), "missing {expected}");
        }
    }
}
