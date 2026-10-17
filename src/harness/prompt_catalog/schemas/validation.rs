use serde::Deserialize;
use std::collections::{HashMap, HashSet};

use super::*;
use crate::harness::prompt_catalog::PromptRole;

mod opportunity;
mod refs;
mod spec_delta;
mod task_graph;

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum RoleOutputV1 {
    Manager(ManagerOutputV2),
    ManagerLegacy(ManagerOutputV1),
    Planner(PlannerOutputV1),
    PlannerV3(PlannerOutputV2),
    Review(ReviewVerdictV1),
    TaskGraph(TaskGraphV1),
    WorkResult(WorkResultV1),
    SpecDelta(SpecDeltaV1),
    PrProposal(PrProposalV1),
    OpportunityDecision(OpportunityDecisionV1),
    PlanningTurn(Box<crate::harness::TurnEnvelope>),
    QaScenarioDraft(crate::harness::prompt_catalog::QaScenarioDraftV1),
}

#[derive(Debug, Clone, Default)]
pub struct OutputValidationContext<'a> {
    pub turn_purpose: crate::core::workflow::TurnPurpose,
    pub expected_feature_uid: Option<&'a str>,
    pub expected_approved_spec_ref: Option<&'a str>,
    pub expected_approved_plan_hash: Option<&'a str>,
    pub approved_decision_refs: Vec<String>,
    pub expected_subject_hash: Option<&'a str>,
    pub evidence_ids: HashSet<String>,
    pub required_requirements: Vec<String>,
    /// Repository ID to exact portable identity mapping.
    pub allowed_repositories: HashMap<String, String>,
    /// Exact permitted source/destination branches by repository ID.
    pub approved_branch_targets: HashMap<String, (String, String)>,
    /// Optional authoritative checkout SHA per repository ID. Model supplied
    /// source SHAs are accepted only when they match one of these entries.
    pub repository_source_shas: HashMap<String, String>,
    pub active_repository_ref: Option<String>,
    pub koolade_product_repository_ref: Option<String>,
    pub public_evidence_ids: HashSet<String>,
}

/// Strictly decode role output. If direct JSON parsing fails, make one
/// deterministic wrapper/fence extraction attempt; never repair semantics.
pub fn validate_output(
    role: PromptRole,
    text: &str,
    expected_subject_hash: Option<&str>,
) -> Result<RoleOutputV1, String> {
    validate_output_with_context(
        role,
        text,
        &OutputValidationContext {
            expected_subject_hash,
            ..Default::default()
        },
    )
}

pub fn validate_output_with_context(
    role: PromptRole,
    text: &str,
    context: &OutputValidationContext<'_>,
) -> Result<RoleOutputV1, String> {
    if text.len() > 1024 * 1024 {
        return Err("model response exceeds 1 MiB".into());
    }
    let direct = decode(role, text, context.turn_purpose);
    let decoded = direct.or_else(|_| {
        decode(
            role,
            &extract_json(text).ok_or_else(|| "no JSON object found".to_owned())?,
            context.turn_purpose,
        )
    })?;
    if let RoleOutputV1::Review(review) = &decoded {
        if let Some(expected) = context.expected_subject_hash
            && review.subject_hash != expected
        {
            return Err("review subject hash does not match the immutable subject".into());
        }
        if review.pass
            && review
                .findings
                .iter()
                .any(|finding| matches!(finding.severity, Severity::Blocker | Severity::High))
        {
            return Err("review cannot pass with blocking or high-severity findings".into());
        }
        if review.pass
            && (review
                .coverage
                .iter()
                .any(|coverage| !matches!(coverage.status, CoverageStatus::Pass))
                || !review.untested.is_empty())
        {
            return Err("review cannot pass with failed or untested requirements".into());
        }
        let actual: HashSet<_> = review
            .coverage
            .iter()
            .map(|coverage| coverage.requirement.as_str())
            .collect();
        if actual.len() != review.coverage.len()
            || context
                .required_requirements
                .iter()
                .any(|requirement| !actual.contains(requirement.as_str()))
        {
            return Err("review coverage is duplicate or omits a required acceptance check".into());
        }
        if !context.required_requirements.is_empty()
            && review.coverage.iter().any(|coverage| {
                !context
                    .required_requirements
                    .contains(&coverage.requirement)
            })
        {
            return Err(
                "review coverage includes a requirement outside the approved matrix".into(),
            );
        }
        for coverage in &review.coverage {
            if matches!(coverage.status, CoverageStatus::Fail)
                && !review
                    .findings
                    .iter()
                    .any(|finding| finding.requirement == coverage.requirement)
            {
                return Err(
                    "failed coverage requires a traceable finding; zero-findings review rejected"
                        .into(),
                );
            }
            validate_refs(&coverage.evidence, &context.evidence_ids)?;
        }
        for finding in &review.findings {
            validate_refs(&finding.evidence, &context.evidence_ids)?;
        }
    }
    if let RoleOutputV1::WorkResult(report) = &decoded {
        if report.schema_version != 2 {
            return Err(format!(
                "unsupported WorkResultV1 schema version {}",
                report.schema_version
            ));
        }
        match report.status {
            WorkStatus::Complete
                if !matches!(report.blocker_disposition, BlockerDisposition::None)
                    || !report.remaining.is_empty() =>
            {
                return Err("complete work result has a blocker or remaining work".into());
            }
            WorkStatus::Blocked
                if matches!(report.blocker_disposition, BlockerDisposition::None) =>
            {
                return Err("blocked work result must identify its blocker disposition".into());
            }
            _ => {}
        }
    }
    if let RoleOutputV1::PlannerV3(planner) = &decoded {
        planner.validate()?;
        if !context.required_requirements.is_empty() {
            for question in &planner.proposed_questions {
                if question
                    .origin_requirement_ids
                    .iter()
                    .any(|id| !context.required_requirements.contains(id))
                {
                    return Err(
                        "clarification proposal cites a requirement outside the active plan".into(),
                    );
                }
            }
            if planner
                .readiness
                .acceptance_tests
                .iter()
                .any(|test| !context.required_requirements.contains(&test.requirement_id))
            {
                return Err("acceptance test cites a requirement outside the active plan".into());
            }
        }
    }
    refs::validate_output_refs(&decoded, context)?;
    Ok(decoded)
}

/// Validate output only after confirming that the persisted manifest names a
/// current role version and exactly matching schema/template hashes.
pub fn validate_output_with_manifest(
    manifest: &super::super::PromptManifest,
    text: &str,
    context: &OutputValidationContext<'_>,
) -> Result<RoleOutputV1, String> {
    let role = super::super::validate_prompt_manifest(manifest)?;
    validate_output_with_context(role, text, context)
}

fn validate_refs(refs: &[String], allowed: &HashSet<String>) -> Result<(), String> {
    if refs.iter().any(|reference| !allowed.contains(reference)) {
        return Err("output cites evidence absent from the application evidence manifest".into());
    }
    Ok(())
}

fn decode(
    role: PromptRole,
    text: &str,
    turn_purpose: crate::core::workflow::TurnPurpose,
) -> Result<RoleOutputV1, String> {
    macro_rules! parse {
        ($ty:ty, $variant:ident) => {
            serde_json::from_str::<$ty>(text)
                .map(RoleOutputV1::$variant)
                .map_err(|error| error.to_string())
        };
    }
    match role {
        PromptRole::Manager | PromptRole::ManagerV3 => parse!(ManagerOutputV2, Manager),
        PromptRole::ManagerV2 => parse!(ManagerOutputV1, ManagerLegacy),
        PromptRole::Planner => parse!(PlannerOutputV2, PlannerV3),
        PromptRole::PlannerV2 => parse!(PlannerOutputV1, Planner),
        PromptRole::PlannerConversation => {
            crate::harness::responses::decode_turn(text, turn_purpose)
                .map(Box::new)
                .map(RoleOutputV1::PlanningTurn)
        }
        PromptRole::PlanReviewer
        | PromptRole::CodeReviewer
        | PromptRole::RuntimeQaReviewer
        | PromptRole::RuntimeQaReviewerV2 => parse!(ReviewVerdictV1, Review),
        PromptRole::RuntimeQaScenarioSynthesizer => {
            parse!(QaScenarioDraftV1, QaScenarioDraft)
        }
        PromptRole::Decomposer => parse!(TaskGraphV1, TaskGraph),
        PromptRole::Implementer | PromptRole::RepairWorker => parse!(WorkResultV1, WorkResult),
        PromptRole::SpecReconciler => parse!(SpecDeltaV1, SpecDelta),
        PromptRole::PrPreparer => parse!(PrProposalV1, PrProposal),
    }
}

fn extract_json(text: &str) -> Option<String> {
    let clean = text
        .trim()
        .strip_prefix("```json")
        .unwrap_or(text.trim())
        .strip_suffix("```")
        .unwrap_or(text.trim())
        .trim();
    let start = clean.find('{')?;
    let end = clean.rfind('}')?;
    (start <= end).then(|| clean[start..=end].to_owned())
}
