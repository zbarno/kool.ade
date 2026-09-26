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
