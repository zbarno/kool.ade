use sha2::Digest;

use super::super::ManagerOutput;
mod evidence;
pub(super) use evidence::{koolade_product_contract_evidence, trusted_opportunity_evidence};

pub(in crate::app::manager) fn persist_opportunities(
    project: &super::super::super::session::Project,
    output: &ManagerOutput,
) -> Vec<String> {
    output
        .opportunities
        .iter()
        .map(|prompt| persist_opportunity(project, prompt, &output.evidence_hashes))
        .collect()
}

fn persist_opportunity(
    project: &super::super::super::session::Project,
    prompt: &crate::harness::prompt_catalog::OpportunityProposalV1,
    evidence_hashes: &std::collections::HashMap<String, String>,
) -> String {
    use crate::{
        core::opportunity::{
            EvidenceConfidentialityV1, OpportunityEvidenceRefV1, OpportunityKindV1,
            OpportunityOriginV1, OpportunityScopeV1, OpportunityStatusV1, OpportunityTargetKindV1,
            OpportunityTargetV1, OpportunityV1,
        },
        harness::prompt_catalog::{
            OpportunityKind, OpportunityOrigin, OpportunityScope, OpportunityTarget,
        },
    };
    let Some(repository) = project
        .state
        .repositories
        .repositories
        .iter()
        .find(|repository| repository.id == prompt.target_repository_ref)
    else {
        return format!(
            "Skipped opportunity with unknown repository reference {}",
            prompt.target_repository_ref
        );
    };
    let Some(repo_path) = project
        .state
        .repositories
        .target_if_available(&project.state.repo_root, &repository.id)
        .ok()
        .flatten()
    else {
        return format!(
            "Skipped opportunity because repository {} is unavailable",
            repository.id
        );
    };
    let Ok(output) = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(repo_path)
        .output()
    else {
        return format!(
            "Skipped opportunity because repository {} has no readable Git revision",
            repository.id
        );
    };
    if !output.status.success() {
        return format!(
            "Skipped opportunity because repository {} has no readable Git revision",
            repository.id
        );
    }
    let source_sha = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    let canonical_remote = if repository.remote.trim().is_empty() {
        prompt.target_repository.clone()
    } else {
        repository.remote.clone()
    };
    let workspace_uid = project
        .state
        .planning_store
        .project_id
        .hyphenated()
        .to_string();
    let repository_uid = stable_uid(&workspace_uid, "repository", &repository.id);
    let evidence_refs = match prompt
        .evidence_refs
        .iter()
        .map(|evidence_id| {
            let digest = evidence_hashes
                .get(evidence_id)
                .ok_or_else(|| format!("missing evidence hash for {evidence_id}"))?;
            Ok(OpportunityEvidenceRefV1 {
                evidence_uid: stable_uid(&workspace_uid, "evidence", evidence_id),
                sha256: digest.clone(),
                confidentiality: EvidenceConfidentialityV1::UserLocal,
            })
        })
        .collect::<Result<Vec<_>, String>>()
    {
        Ok(refs) => refs,
        Err(error) => {
            return format!("Skipped opportunity because its evidence is unavailable: {error}");
        }
    };
    let observation_revision = match prompt.observation_revision.parse::<u64>() {
        Ok(revision) if revision > 0 => revision,
        _ => return "Skipped opportunity because its observation revision is invalid".into(),
    };
    let proposal = OpportunityV1 {
        schema_version: 1,
        opportunity_uid: uuid::Uuid::new_v4().hyphenated().to_string(),
        workspace_uid: workspace_uid.clone(),
        origin: match &prompt.origin {
            OpportunityOrigin::UserGoal => OpportunityOriginV1::UserGoal,
            OpportunityOrigin::RunFailure => OpportunityOriginV1::RunFailure,
            OpportunityOrigin::QaFinding => OpportunityOriginV1::QaFinding,
            OpportunityOrigin::ReviewFinding => OpportunityOriginV1::ReviewFinding,
            OpportunityOrigin::SpecDrift => OpportunityOriginV1::SpecDrift,
            OpportunityOrigin::CodeInspection => OpportunityOriginV1::CodeInspection,
            OpportunityOrigin::UserFeedback => OpportunityOriginV1::UserFeedback,
            OpportunityOrigin::Roadmap => OpportunityOriginV1::Roadmap,
            OpportunityOrigin::ScheduledExploration => OpportunityOriginV1::ScheduledExploration,
        },
        kind: match &prompt.kind {
            OpportunityKind::NewFeature => OpportunityKindV1::NewFeature,
            OpportunityKind::ExistingFeatureExtension => {
                OpportunityKindV1::ExistingFeatureExtension
            }
            OpportunityKind::BugFix => OpportunityKindV1::BugFix,
            OpportunityKind::Integration => OpportunityKindV1::Integration,
            OpportunityKind::TechStackSupport => OpportunityKindV1::TechStackSupport,
            OpportunityKind::Quality => OpportunityKindV1::Quality,
            OpportunityKind::Security => OpportunityKindV1::Security,
            OpportunityKind::Performance => OpportunityKindV1::Performance,
            OpportunityKind::DeveloperExperience => OpportunityKindV1::DeveloperExperience,
            OpportunityKind::Documentation => OpportunityKindV1::Documentation,
            OpportunityKind::Infrastructure => OpportunityKindV1::Infrastructure,
        },
        target: OpportunityTargetV1 {
            kind: match &prompt.target {
                OpportunityTarget::ActiveUserProject => OpportunityTargetKindV1::ActiveUserProject,
                OpportunityTarget::KooladeProduct => OpportunityTargetKindV1::KooladeProduct,
                OpportunityTarget::OtherLinkedRepository => {
                    OpportunityTargetKindV1::OtherLinkedRepository
                }
            },
            repository_uid,
            canonical_remote,
            source_sha,
        },
        related_feature_uids: prompt.related_feature_uids.clone(),
        related_task_uids: prompt.related_task_uids.clone(),
        discovered_at_ms: chrono::Utc::now().timestamp_millis().max(1) as u64,
        observation_revision,
        evidence_refs,
        problem_or_opportunity: prompt.problem_or_opportunity.clone(),
        user_value: prompt.user_value.clone(),
        expected_outcome: prompt.expected_outcome.clone(),
        possible_approaches: prompt.possible_approaches.clone(),
        recommended_approach: prompt.recommended_approach.clone(),
        alternatives: prompt
            .alternatives
            .lines()
            .map(|line| line.trim().trim_start_matches("- ").to_owned())
            .filter(|line| !line.is_empty())
            .collect(),
        estimated_scope: match &prompt.estimated_scope {
            OpportunityScope::Small => OpportunityScopeV1::Small,
            OpportunityScope::Medium => OpportunityScopeV1::Medium,
            OpportunityScope::Large => OpportunityScopeV1::Large,
            OpportunityScope::Unknown => OpportunityScopeV1::Unknown,
        },
        impact: prompt.impact.clone(),
        cost_or_risk: prompt.cost_or_risk.clone(),
        affected_systems: prompt.affected_systems.clone(),
        current_capability_state: prompt.current_capability_state.clone(),
        generic_workaround: prompt.generic_workaround.clone(),
        proposed_approach: prompt.proposed_approach.clone(),
        dedup_fingerprint: "0".repeat(64),
        status: OpportunityStatusV1::AwaitingDecision,
        revision: 1,
    };
    match crate::core::opportunity::OpportunityStore::new(&project.state.planning_store)
        .save_prompt_advisory(proposal, prompt.clone())
    {
        Ok(crate::core::opportunity::OpportunitySaveResult::Created { proposal, .. }) => format!(
            "New improvement proposal: {} ({})",
            proposal.problem_or_opportunity, proposal.opportunity_uid
        ),
        Ok(crate::core::opportunity::OpportunitySaveResult::Duplicate { proposal, .. }) => format!(
            "Already tracked improvement proposal: {} ({})",
            proposal.problem_or_opportunity, proposal.opportunity_uid
        ),
        Err(error) => format!("Improvement proposal could not be persisted: {error}"),
    }
}

fn stable_uid(workspace_uid: &str, kind: &str, value: &str) -> String {
    let digest = sha2::Sha256::digest(format!("{workspace_uid}\0{kind}\0{value}").as_bytes());
    let mut bytes = [0u8; 16];
    bytes.copy_from_slice(&digest[..16]);
    bytes[6] = (bytes[6] & 0x0f) | 0x50;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    uuid::Uuid::from_bytes(bytes).hyphenated().to_string()
}
