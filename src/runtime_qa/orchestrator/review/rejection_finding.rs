use crate::runtime_qa::{QaEvidenceManifestV1, QaRunRequestV1, QaScenarioV1};

pub(super) fn rejection_finding(
    request: &QaRunRequestV1,
    scenarios: &[QaScenarioV1],
    evidence: &QaEvidenceManifestV1,
    requirement: &str,
    actual: String,
) -> Result<super::super::super::QaFindingV1, String> {
    let scenario = scenarios
        .iter()
        .find(|scenario| {
            evidence.assertions.iter().any(|assertion| {
                assertion.scenario_ids.contains(&scenario.id)
                    && assertion.requirement_ids.iter().any(|id| id == requirement)
            })
        })
        .ok_or_else(|| "review rejection does not map to observed scenario evidence".to_owned())?;
    let screenshot_ids = evidence
        .screenshots
        .iter()
        .filter(|shot| shot.scenario_id == scenario.id)
        .map(|shot| shot.id.clone())
        .collect::<Vec<_>>();
    let trace_ids = evidence
        .actions
        .iter()
        .filter(|action| action.scenario_id == scenario.id)
        .map(|action| action.index.to_string())
        .collect::<Vec<_>>();
    if screenshot_ids.is_empty() && trace_ids.is_empty() {
        return Err("review rejection has no observed screenshot or action evidence".into());
    }
    Ok(super::super::super::QaFindingV1 {
        finding_uid: uuid::Uuid::new_v4().hyphenated().to_string(),
        code_sha: request.code_head_sha.clone(),
        scenario_uid: scenario.id.clone(),
        requirement_uids: vec![requirement.to_owned()],
        category: super::super::super::QaFindingCategory::Functional,
        severity: super::super::super::QaFindingSeverity::High,
        expected: "The approved acceptance criterion is satisfied.".into(),
        actual: actual.clone(),
        reproducible_steps: vec![actual.clone()],
        screenshot_ids,
        trace_ids,
        smallest_fix: actual,
        blocking: true,
    })
}
