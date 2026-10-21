use serde::{Deserialize, Serialize};

use super::{
    QaCaseStatus, QaEvidenceManifestV1, QaFindingV1, QaOutcome, is_sha256, is_uuid, valid_text,
};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct QaCaseResultV1 {
    pub scenario_id: String,
    pub status: QaCaseStatus,
    pub evidence_refs: Vec<String>,
    pub observations: Vec<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum QaCoverageStatus {
    Observed,
    Untested,
    Blocked,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct QaRequirementCoverageV1 {
    pub requirement_id: String,
    pub status: QaCoverageStatus,
    pub evidence_refs: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct QaCoverageV1 {
    pub requirements: Vec<QaRequirementCoverageV1>,
    pub scenario_ids: Vec<String>,
    pub states: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct QaResultV1 {
    pub schema_version: u16,
    pub subject_sha: String,
    pub run_id: String,
    pub reviewer_execution_id: String,
    pub reviewer_session_id: String,
    pub outcome: QaOutcome,
    pub evidence_manifest_sha256: String,
    pub cases: Vec<QaCaseResultV1>,
    pub coverage: QaCoverageV1,
    pub findings: Vec<QaFindingV1>,
    pub tool_versions: std::collections::BTreeMap<String, String>,
    pub started_at_unix_ms: u64,
    pub completed_at_unix_ms: u64,
}

impl QaResultV1 {
    pub fn validate(
        &self,
        request: &super::QaRunRequestV1,
        scenarios: &[super::QaScenarioV1],
        manifest: &QaEvidenceManifestV1,
    ) -> Result<(), String> {
        manifest.validate(request, scenarios)?;
        let evidence_hash = manifest.sha256()?;
        if self.schema_version != 1
            || self.subject_sha != request.code_head_sha
            || self.run_id != request.qa_run_id
            || !is_uuid(&self.reviewer_execution_id)
            || !is_uuid(&self.reviewer_session_id)
            || self.reviewer_execution_id == request.author_run_id
            || self.reviewer_session_id == request.author_session_id
            || self.evidence_manifest_sha256 != evidence_hash
            || self.cases.len() != scenarios.len()
            || self.coverage.requirements.len() != request.requirement_ids.len()
            || self.coverage.scenario_ids.len() != scenarios.len()
            || self.coverage.states.len() > 512
            || self
                .coverage
                .states
                .iter()
                .any(|state| !valid_text(state, 256))
            || self.started_at_unix_ms == 0
            || self.completed_at_unix_ms < self.started_at_unix_ms
            || self.tool_versions.is_empty()
            || self.tool_versions.len() > 128
            || self
                .tool_versions
                .iter()
                .any(|(key, value)| !valid_text(key, 128) || !valid_text(value, 128))
            || self.tool_versions.get("driver") != Some(&manifest.driver_version)
        {
            return Err("runtime QA result does not match its immutable request/evidence".into());
        }
        let mut known_evidence = manifest
            .screenshots
            .iter()
            .map(|shot| shot.id.as_str())
            .collect::<std::collections::BTreeSet<_>>();
        known_evidence.extend(
            manifest
                .actions
                .iter()
                .flat_map(|action| action.evidence_ids.iter().map(String::as_str)),
        );
        let case_ids = self
            .cases
            .iter()
            .map(|case| case.scenario_id.as_str())
            .collect::<std::collections::BTreeSet<_>>();
        let covered_scenario_ids = self
            .coverage
            .scenario_ids
            .iter()
            .collect::<std::collections::BTreeSet<_>>();
        if case_ids.len() != self.cases.len()
            || scenarios
                .iter()
                .any(|scenario| !case_ids.contains(scenario.id.as_str()))
            || covered_scenario_ids.len() != scenarios.len()
            || scenarios
                .iter()
                .any(|scenario| !covered_scenario_ids.contains(&&scenario.id))
            || self.cases.iter().any(|case| {
                case.evidence_refs
                    .iter()
                    .any(|id| !known_evidence.contains(id.as_str()))
                    || (matches!(case.status, QaCaseStatus::Passed | QaCaseStatus::Failed)
                        && case.evidence_refs.is_empty())
                    || case.observations.len() > 128
                    || case.observations.iter().any(|item| !valid_text(item, 4096))
            })
        {
            return Err("runtime QA case coverage or evidence references are invalid".into());
        }
        let requirement_ids = self
            .coverage
            .requirements
            .iter()
            .map(|item| item.requirement_id.as_str())
            .collect::<std::collections::BTreeSet<_>>();
        if requirement_ids.len() != request.requirement_ids.len()
            || request
                .requirement_ids
                .iter()
                .any(|id| !requirement_ids.contains(id.as_str()))
            || self.coverage.requirements.iter().any(|item| {
                item.status == super::QaCoverageStatus::Observed
                    && (item.evidence_refs.is_empty()
                        || item
                            .evidence_refs
                            .iter()
                            .any(|id| !known_evidence.contains(id.as_str())))
            })
            || self
                .findings
                .iter()
                .any(|finding| finding.validate(manifest).is_err())
        {
            return Err("runtime QA requirement coverage or finding evidence is invalid".into());
        }
        if self.outcome == QaOutcome::Pass
            && (self
                .cases
                .iter()
                .any(|case| case.status != QaCaseStatus::Passed)
                || self
                    .coverage
                    .requirements
                    .iter()
                    .any(|item| item.status != super::QaCoverageStatus::Observed)
                || manifest.screenshots.is_empty()
                || manifest
                    .assertions
                    .iter()
                    .any(|item| item.status != super::QaAssertionStatus::Passed)
                || self.coverage.states.is_empty()
                || request.requirement_ids.iter().any(|requirement| {
                    !manifest.assertions.iter().any(|assertion| {
                        assertion.status == super::QaAssertionStatus::Passed
                            && assertion.requirement_ids.iter().any(|id| id == requirement)
                    })
                })
                || self.findings.iter().any(|finding| finding.blocking))
        {
            return Err(
                "runtime QA cannot pass with failed, untested, blocked, or blocking evidence"
                    .into(),
            );
        }
        if self.outcome == QaOutcome::Fail
            && !self
                .cases
                .iter()
                .any(|case| case.status == QaCaseStatus::Failed)
            && !manifest
                .assertions
                .iter()
                .any(|assertion| assertion.status == super::QaAssertionStatus::Failed)
            && self.findings.iter().all(|finding| !finding.blocking)
            && manifest.crashes.is_empty()
        {
            return Err("runtime QA cannot report failure without an observed failing assertion, case, finding, or crash".into());
        }
        if self.outcome == QaOutcome::InfrastructureBlocked
            && !self
                .cases
                .iter()
                .any(|case| case.status == QaCaseStatus::Blocked)
            && !self
                .coverage
                .requirements
                .iter()
                .any(|coverage| coverage.status == super::QaCoverageStatus::Blocked)
            && !manifest
                .assertions
                .iter()
                .any(|assertion| assertion.status == super::QaAssertionStatus::Blocked)
        {
            return Err(
                "runtime QA infrastructure block requires explicit blocked evidence".into(),
            );
        }
        if self.findings.len() > 512 {
            return Err("runtime QA result exceeds report limits".into());
        }
        Ok(())
    }
}

pub fn validate_report(
    request: &super::QaRunRequestV1,
    scenarios: &[super::QaScenarioV1],
    manifest: &QaEvidenceManifestV1,
    result: &QaResultV1,
) -> Result<(), String> {
    super::scenario::validate_scenario_set(request, scenarios)?;
    if result.evidence_manifest_sha256 != manifest.sha256()? || !is_sha256(&result.subject_sha) {
        return Err("runtime QA report evidence hash or subject is invalid".into());
    }
    result.validate(request, scenarios, manifest)
}
