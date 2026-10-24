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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_evidence_sha256: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) source_evidence:
        Option<crate::runtime_qa::scenario_synthesis::QaScenarioSourceEvidenceV1>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) prompt_manifest: Option<crate::harness::prompt_catalog::PromptManifest>,
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
            scenario_sha256: super::scenario_set_sha256(&scenarios)?,
            source_evidence_sha256: None,
            source_evidence: None,
            prompt_manifest: None,
            scenarios,
        })
    }

    pub(crate) fn new_with_source_evidence(
        request: &QaRunRequestV1,
        task_revision: u64,
        scenarios: Vec<QaScenarioV1>,
        source_evidence: &crate::runtime_qa::scenario_synthesis::QaScenarioSourceEvidenceV1,
        prompt_manifest: &crate::harness::prompt_catalog::PromptManifest,
    ) -> Result<Self, String> {
        let mut proposal = Self::new(request, task_revision, scenarios)?;
        source_evidence.validate_for(request)?;
        proposal.source_evidence_sha256 = Some(source_evidence.sha256.clone());
        proposal.source_evidence = Some(source_evidence.clone());
        proposal.prompt_manifest = Some(prompt_manifest.clone());
        Ok(proposal)
    }

    pub(crate) fn validate_source_evidence(
        &self,
        evidence: &crate::runtime_qa::scenario_synthesis::QaScenarioSourceEvidenceV1,
    ) -> Result<(), String> {
        if self.source_evidence_sha256.as_deref() == Some(evidence.sha256.as_str())
            && self.source_evidence.as_ref() == Some(evidence)
        {
            Ok(())
        } else {
            Err("runtime QA scenario proposal is not bound to the current source evidence".into())
        }
    }

    pub fn proposal_sha256(&self) -> Result<String, String> {
        canonical_hash(self)
    }

    pub fn validate_for(&self, request: &QaRunRequestV1) -> Result<(), String> {
        request.validate()?;
        if self.schema_version != 1
            || self.qa_run_id != request.qa_run_id
            || self.request_sha256 != canonical_hash(request)?
            || self.scenario_sha256 != super::scenario_set_sha256(&self.scenarios)?
        {
            return Err("runtime QA scenario proposal is stale or has been modified".into());
        }
        match (
            &self.source_evidence,
            &self.source_evidence_sha256,
            &self.prompt_manifest,
        ) {
            (Some(evidence), Some(hash), Some(manifest)) => {
                evidence.validate_for(request)?;
                if evidence.sha256 != *hash
                    || crate::harness::prompt_catalog::validate_prompt_manifest(manifest)?
                        != crate::harness::prompt_catalog::PromptRole::RuntimeQaScenarioSynthesizer
                {
                    return Err("runtime QA scenario proposal source provenance is invalid".into());
                }
            }
            (None, None, None) => {}
            _ => {
                return Err("runtime QA scenario proposal has incomplete source provenance".into());
            }
        }
        validate_scenario_set(request, &self.scenarios)
    }
}

fn canonical_hash<T: Serialize + ?Sized>(value: &T) -> Result<String, String> {
    let bytes = serde_json::to_vec(value)
        .map_err(|error| format!("runtime QA proposal cannot be encoded: {error}"))?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}
