use super::super::*;
use crate::{
    artifacts::planning_store::PlanningStore,
    core::lifecycle::{LifecycleStore, PullRequestPreparationStatus, TaskPhase},
    harness::prompt_catalog::{PromptManifest, PromptRole},
};
use serde::{Deserialize, Serialize};

mod body;
mod privacy;
mod request;
mod validation;

const RECEIPT_FILE: &str = "pr-preparer-v1.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PreparedProposalV1 {
    schema_version: u16,
    scope_sha256: String,
    prompt: PromptManifest,
    proposal: crate::harness::prompt_catalog::PrProposalV1,
}

pub(in crate::core::implementation) fn prepare_lifecycle_pr_proposal(
    store: &PlanningStore,
    dir: &Path,
    state: &Implementation,
    harness: &dyn AiHarness,
    runner: &Runner,
) -> anyhow::Result<bool> {
    let Some(planning_item_uid) = state.task_uid.as_deref() else {
        return Ok(false);
    };
    let lifecycle = LifecycleStore::new(store);
    let Some(task) = lifecycle.task_for_planning_item(planning_item_uid)? else {
        return Ok(false);
    };
    if task.phase != TaskPhase::Done {
        return Ok(false);
    }
    let work_uid = task
        .pr_work_item_uid
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("completed lifecycle task has no PR preparation item"))?;
    let work = lifecycle.load_pr_preparation(work_uid)?;
    if !matches!(
        work.status,
        PullRequestPreparationStatus::AwaitingAuthorization
            | PullRequestPreparationStatus::Publishing
            | PullRequestPreparationStatus::NeedsAttention
    ) {
        return Ok(false);
    }
    anyhow::ensure!(
        work.task_uid == task.task_uid && work.feature_uid == task.feature_uid,
        "PR preparation item does not match its lifecycle task"
    );
    let feature = lifecycle.load_feature(&task.feature_uid)?;
    anyhow::ensure!(
        matches!(
            feature.phase,
            crate::core::lifecycle::FeaturePhase::Complete
                | crate::core::lifecycle::FeaturePhase::Executing
        ),
        "PR Preparer requires an approved lifecycle feature"
    );
    let included_tasks = work
        .included_task_uids
        .iter()
        .map(|uid| lifecycle.load_task(uid))
        .collect::<Result<Vec<_>, _>>()?;
    request::validate_included_tasks(&task, &included_tasks)?;
    super::super::integration::validate_subtasks_for_pr(
        dir,
        &task,
        &included_tasks,
        state,
        runner,
    )?;
    let scope = request::collect_scope(state, dir, runner, &task, &included_tasks, &feature)?;
    let request = request::build_request(state, &task, &included_tasks, &feature, &scope, runner)?;
    let receipt_path = dir.join(RECEIPT_FILE);
    let had_receipt = receipt_path.exists();
    if let Some(receipt) = validation::read_reusable_receipt(&receipt_path, &scope)? {
        if let Err(error) = body::write(dir, &receipt, &scope) {
            if privacy::is_sensitive_content_error(&error) {
                std::fs::remove_file(&receipt_path)?;
                body::discard(dir)?;
            }
            return Err(error);
        }
        runner.update(
            "Reusing the validated PR Preparer proposal for the unchanged integrated diff…",
        );
        return Ok(true);
    }
    if had_receipt {
        runner
            .update("The integrated scope or PR Preparer prompt changed; rebuilding its proposal…");
    }

    runner.remaining()?;
    runner.update(
        "Checking the exact lifecycle diff, branch refs, commits, and evidence with PR Preparer…",
    );
    let outcome = harness.execute(&request.request)?;
    runner.remaining()?;
    let context = crate::harness::prompt_catalog::OutputValidationContext {
        evidence_ids: scope.expected.evidence_ids.iter().cloned().collect(),
        ..Default::default()
    };
    let output = crate::harness::prompt_catalog::validate_output_with_manifest(
        &request.manifest,
        &outcome.final_text,
        &context,
    )
    .map_err(anyhow::Error::msg)?;
    let crate::harness::prompt_catalog::RoleOutputV1::PrProposal(proposal) = output else {
        anyhow::bail!("PR Preparer returned an unexpected output schema");
    };
    validation::validate_proposal(&proposal, &scope.expected)?;
    let receipt = PreparedProposalV1 {
        schema_version: 1,
        scope_sha256: scope.scope_sha256.clone(),
        prompt: request.manifest,
        proposal,
    };
    if let Err(error) = body::render(&receipt, &scope) {
        if privacy::is_sensitive_content_error(&error) {
            body::discard(dir)?;
        }
        return Err(error);
    }
    crate::artifacts::atomic_write_bytes(&receipt_path, &serde_json::to_vec_pretty(&receipt)?)?;
    body::write(dir, &receipt, &scope)?;
    runner.update("PR Preparer matched the application-validated publication scope.");
    Ok(true)
}

#[derive(Debug, Clone, Serialize)]
struct ExpectedProposal {
    task_uid: String,
    base_ref: String,
    head_ref: String,
    commits: Vec<String>,
    files: Vec<String>,
    evidence_ids: Vec<String>,
}

struct ProposalScope {
    expected: ExpectedProposal,
    scope_sha256: String,
    feature_plan: String,
    task_spec: String,
    gate_evidence: String,
    verified_report: String,
    diff: String,
    scope_manifest: String,
}

struct PromptRequest {
    request: PlanningRequest,
    manifest: PromptManifest,
}

#[derive(Debug, Clone, Serialize)]
struct TaskGateEvidence<'a> {
    task_uid: &'a str,
    revision: u64,
    phase: TaskPhase,
    approved_plan_sha256: &'a str,
    implementation_sha256: Option<&'a str>,
    reviewed_code_head_sha: Option<&'a str>,
    reviewed_code_subject_sha256: Option<&'a str>,
    code_review: &'a Option<crate::core::lifecycle::GateReceipt>,
    runtime_qa: &'a Option<crate::core::lifecycle::GateReceipt>,
    spec_reconciliation: &'a Option<crate::core::lifecycle::GateReceipt>,
}
