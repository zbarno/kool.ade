use crate::{
    artifacts::planning_store::PlanningStore,
    core::{
        opportunity::{OpportunityStore, OpportunityTargetKindV1, OpportunityV1},
        self_improvement::{SelfImprovementPhaseV1, SelfImprovementRunV1, SelfImprovementStore},
    },
};

#[cfg(test)]
#[path = "self_improvement/tests.rs"]
mod tests;

pub(super) fn create_self_improvement_run(
    planning: &PlanningStore,
    opportunities: &OpportunityStore<'_>,
    proposal: &OpportunityV1,
    feature_uid: &str,
) -> anyhow::Result<Option<SelfImprovementRunV1>> {
    if proposal.target.kind != OpportunityTargetKindV1::KooladeProduct {
        return Ok(None);
    }
    let decision = opportunities
        .decision_for_opportunity(&proposal.opportunity_uid)?
        .ok_or_else(|| anyhow::anyhow!("persisted opportunity decision is missing"))?;
    let run = SelfImprovementRunV1 {
        schema_version: 1,
        run_uid: SelfImprovementStore::run_uid_for_decision(&decision.uid)?,
        opportunity_uid: proposal.opportunity_uid.clone(),
        approved_decision_uid: decision.uid.clone(),
        target_repository_uid: proposal.target.repository_uid.clone(),
        canonical_remote: proposal.target.canonical_remote.clone(),
        source_ref: None,
        source_sha: proposal.target.source_sha.clone(),
        destination_ref: None,
        feature_uid: feature_uid.to_owned(),
        task_uids: Vec::new(),
        owner_task_uid: None,
        parent_task_progress: Vec::new(),
        clone_uid: None,
        plan_approval_hash: None,
        independent_plan_review_uid: None,
        implementation_head_sha: None,
        implementation_subject_sha256: None,
        code_review_uids: Vec::new(),
        runtime_qa_uids: Vec::new(),
        spec_receipt_uid: None,
        pr_work_item_uid: None,
        pr_url: None,
        merged_sha: None,
        activated_version: None,
        phase: SelfImprovementPhaseV1::ApprovedToPlan,
        revision: 0,
    };
    let created = SelfImprovementStore::new(planning).create_from_pursued(
        opportunities,
        &decision.uid,
        run,
    )?;
    Ok(Some(created))
}
