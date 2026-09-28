use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PlanComparisonRecord {
    pub schema_version: u32,
    pub feature_id: String,
    pub alternatives: crate::domain::PlanComparison,
    /// Prior proposals for this feature, retained when a comparison is retried.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub history: Vec<crate::domain::PlanComparison>,
    pub transcript: String,
    pub status: PlanComparisonStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selected_plan: Option<String>,
    /// Unix milliseconds, stable across serialization and restart.
    pub updated_at_ms: u128,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlanComparisonStatus {
    Proposed,
    Adopted,
    Discarded,
}

impl PlanComparisonRecord {
    pub fn validate(&self) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.schema_version == 1,
            "Unsupported comparison record version"
        );
        anyhow::ensure!(
            !self.feature_id.trim().is_empty(),
            "Comparison record has no feature ID"
        );
        let comparison = &self.alternatives;
        crate::core::validation::validate_persisted(comparison)?;
        for previous in &self.history {
            crate::core::validation::validate_persisted(previous)?;
        }
        anyhow::ensure!(
            comparison.alternatives.len() == 2
                && comparison.alternatives[0].id == "A"
                && comparison.alternatives[1].id == "B"
                && ["A", "B"].contains(&comparison.recommendation.plan_id.as_str())
                && comparison.selected_plan == self.selected_plan,
            "Malformed plan comparison record"
        );
        match self.status {
            PlanComparisonStatus::Proposed => anyhow::ensure!(
                self.selected_plan.is_none(),
                "Proposed comparison cannot have a selection"
            ),
            PlanComparisonStatus::Adopted => anyhow::ensure!(
                self.selected_plan.is_some(),
                "Adopted comparison requires a selection"
            ),
            PlanComparisonStatus::Discarded => {}
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn workflow_without_comparison_records_still_loads() {
        let old =
            r#"{"brief":null,"reviewedSpecification":null,"taskBatches":[],"approvedFeatures":{}}"#;
        let workflow: crate::core::workflow::Workflow = serde_json::from_str(old).unwrap();
        assert!(workflow.plan_comparisons.is_empty());
    }
}
