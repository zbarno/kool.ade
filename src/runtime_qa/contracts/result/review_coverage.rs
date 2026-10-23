use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum QaReviewCoverageStatusV1 {
    Pass,
    Fail,
    Untested,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct QaReviewRequirementCoverageV1 {
    pub requirement_id: String,
    pub status: QaReviewCoverageStatusV1,
    pub evidence_refs: Vec<String>,
}
