use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::{RuntimeQaStore, create_or_match_json, read_json, safe_uuid};
use crate::{
    core::lifecycle::{GateActor, GateDecision, GateReceipt},
    runtime_qa::{QaRunRequestV1, QaScenarioV1, validate_scenario_set},
};

/// Durable operator approval for the exact scenario set of one immutable QA
/// request. Scenarios are supplied by the UI/operator and are never inferred
/// from model output or synthesized by the runtime dispatcher.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ApprovedQaScenarioSetV1 {
    pub schema_version: u16,
    pub qa_run_id: String,
    pub request_sha256: String,
    pub scenario_sha256: String,
    pub scenarios: Vec<QaScenarioV1>,
    pub approval: GateReceipt,
}

impl RuntimeQaStore {
    /// Persist scenarios only with an explicit user approval bound to the
    /// request and canonical scenario-set hash.
    pub fn approve_scenarios(
        &self,
        request: &QaRunRequestV1,
        scenarios: &[QaScenarioV1],
        approval: &GateReceipt,
    ) -> Result<ApprovedQaScenarioSetV1, String> {
        request.validate()?;
        validate_scenario_set(request, scenarios)?;
        let request_sha256 = hash(request)?;
        let scenario_sha256 = crate::runtime_qa::scenario_set_sha256(scenarios)?;
        validate_approval(request, &scenario_sha256, approval)?;
        let record = ApprovedQaScenarioSetV1 {
            schema_version: 1,
            qa_run_id: request.qa_run_id.clone(),
            request_sha256,
            scenario_sha256,
            scenarios: scenarios.to_vec(),
            approval: approval.clone(),
        };
        let run = safe_uuid(&request.qa_run_id)?;
        let path = self.root.join(run).join("approved-scenarios-v2.json");
        create_or_match_json(&self.root, &path, &record)?;
        Ok(record)
    }

    /// Load only an exact, still-valid approved scenario set for this request.
    pub fn load_approved_scenarios(
        &self,
        request: &QaRunRequestV1,
    ) -> Result<ApprovedQaScenarioSetV1, String> {
        request.validate()?;
        let run = safe_uuid(&request.qa_run_id)?;
        let record: ApprovedQaScenarioSetV1 = read_json(
            &self.root,
            &self.root.join(run).join("approved-scenarios-v2.json"),
        )?;
        let request_sha256 = hash(request)?;
        let scenario_sha256 = crate::runtime_qa::scenario_set_sha256(&record.scenarios)?;
        if record.schema_version != 1
            || record.qa_run_id != request.qa_run_id
            || record.request_sha256 != request_sha256
            || record.scenario_sha256 != scenario_sha256
        {
            return Err("approved runtime QA scenarios are stale for this request".into());
        }
        validate_scenario_set(request, &record.scenarios)?;
        validate_approval(request, &scenario_sha256, &record.approval)?;
        Ok(record)
    }
}

fn validate_approval(
    request: &QaRunRequestV1,
    scenario_sha256: &str,
    approval: &GateReceipt,
) -> Result<(), String> {
    approval.validate()?;
    if approval.subject_uid != request.parent_task_uid
        || approval.subject_sha256 != scenario_sha256
        || approval.actor != GateActor::User
        || approval.decision != GateDecision::Approved
        || !approval.evidence_refs.iter().any(|reference| {
            reference == &format!("runtime-qa:{}/approved-scenarios", request.qa_run_id)
        })
        || approval.reviewer_run_id.is_some()
        || approval.reviewer_session_id.is_some()
        || approval.author_run_id.is_some()
        || approval.author_session_id.is_some()
    {
        return Err(
            "runtime QA scenarios require an explicit user approval bound to this exact set".into(),
        );
    }
    Ok(())
}

fn hash<T: Serialize + ?Sized>(value: &T) -> Result<String, String> {
    let bytes = serde_json::to_vec(value)
        .map_err(|error| format!("runtime QA approval cannot be encoded: {error}"))?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}
