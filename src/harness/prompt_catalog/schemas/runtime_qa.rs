use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QaScenarioDraftV1 {
    pub scenarios: Vec<crate::runtime_qa::QaScenarioV1>,
}
