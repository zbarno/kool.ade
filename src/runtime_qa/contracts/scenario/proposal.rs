use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::super::{QaRunRequestV1, QaScenarioV1, validate_scenario_set};

/// Immutable synthesized scenario proposal. Persisting this record does not
/// authorize execution; the operator must separately approve its exact hash.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct QaScenarioProposalV1 {
    pub schema_version: u16,
    pub qa_run_id: String,
    pub task_revision: u64,
    pub request_sha256: String,
    pub scenario_sha256: String,
    pub scenarios: Vec<QaScenarioV1>,
}

impl QaScenarioProposalV1 {
    pub fn new(
        request: &QaRunRequestV1,
        task_revision: u64,
        scenarios: Vec<QaScenarioV1>,
    ) -> Result<Self, String> {
        request.validate()?;
        validate_scenario_set(request, &scenarios)?;
        Ok(Self {
            schema_version: 1,
            qa_run_id: request.qa_run_id.clone(),
            task_revision,
            request_sha256: canonical_hash(request)?,
            scenario_sha256: canonical_hash(&scenarios)?,
            scenarios,
        })
    }

    pub fn proposal_sha256(&self) -> Result<String, String> {
        canonical_hash(self)
    }

    pub fn validate_for(&self, request: &QaRunRequestV1) -> Result<(), String> {
        request.validate()?;
        if self.schema_version != 1
            || self.qa_run_id != request.qa_run_id
            || self.request_sha256 != canonical_hash(request)?
            || self.scenario_sha256 != canonical_hash(&self.scenarios)?
        {
            return Err("runtime QA scenario proposal is stale or has been modified".into());
        }
        validate_scenario_set(request, &self.scenarios)
    }
}

fn canonical_hash<T: Serialize + ?Sized>(value: &T) -> Result<String, String> {
    let bytes = serde_json::to_vec(value)
        .map_err(|error| format!("runtime QA proposal cannot be encoded: {error}"))?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}
