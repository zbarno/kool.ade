use super::super::Implementation;
use crate::artifacts::planning_store::PlanningStore;
use crate::core::lifecycle::{
    LifecycleStore, PrPublicationScopeV1, PrPublicationTaskEvidenceV1, PullRequestPreparation,
    TaskPhase,
};

pub(in crate::core::implementation) fn build_publication_scope(
    store: &PlanningStore,
    implementation: &Implementation,
    task_uid: &str,
    work: &PullRequestPreparation,
) -> anyhow::Result<PrPublicationScopeV1> {
    let lifecycle = LifecycleStore::new(store);
    let task = lifecycle.load_task(task_uid)?;
    anyhow::ensure!(
        task.phase == TaskPhase::Done && task.task_uid == work.task_uid,
        "publication scope requires the current completed parent task"
    );
    let source_repository_id = implementation
        .repository_id
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| anyhow::anyhow!("PR source repository identity is unavailable"))?;
    let source_sha = implementation
        .verified_head
        .as_deref()
        .filter(|head| task.reviewed_code_head_sha.as_deref() == Some(*head))
        .ok_or_else(|| anyhow::anyhow!("publication head is not the reviewed task head"))?;
    // Task setup stores the resolved destination branch in `base`; the
    // optional field only retains an explicitly configured destination.
    let destination_branch = implementation
        .destination_branch
        .as_deref()
        .or_else(|| {
            (!implementation.base.trim().is_empty()).then_some(implementation.base.as_str())
        })
        .ok_or_else(|| anyhow::anyhow!("PR destination branch is unavailable"))?;
    let spec_revision = lifecycle
        .load_spec_revision(&task.feature_uid)?
        .ok_or_else(|| anyhow::anyhow!("accepted specification revision is unavailable"))?;

    let mut task_evidence = Vec::with_capacity(work.included_task_uids.len());
    for included_uid in &work.included_task_uids {
        let included = lifecycle.load_task(included_uid)?;
        included.validate().map_err(anyhow::Error::msg)?;
        anyhow::ensure!(
            included.phase == TaskPhase::Done && included.feature_uid == task.feature_uid,
            "publication scope contains a task without current completion gates"
        );
        let reviewed_head_sha = included
            .reviewed_code_head_sha
            .clone()
            .ok_or_else(|| anyhow::anyhow!("included task has no reviewed head"))?;
        let reviewed_diff_hash = included
            .implementation_sha256
            .clone()
            .ok_or_else(|| anyhow::anyhow!("included task has no reviewed diff hash"))?;
        task_evidence.push(PrPublicationTaskEvidenceV1 {
            task_uid: included.task_uid,
            task_revision: included.revision,
            reviewed_head_sha,
            reviewed_diff_hash,
            code_review_receipt: included
                .code_review
                .ok_or_else(|| anyhow::anyhow!("included task has no code-review receipt"))?,
            qa_receipt: included
                .runtime_qa
                .ok_or_else(|| anyhow::anyhow!("included task has no QA receipt"))?,
            spec_sync_receipt: included.spec_reconciliation.ok_or_else(|| {
                anyhow::anyhow!("included task has no specification-sync receipt")
            })?,
        });
    }

    let scope = PrPublicationScopeV1 {
        schema_version: 1,
        publication_generation: 1,
        source_repository_id: source_repository_id.to_owned(),
        source_sha: source_sha.to_owned(),
        implementation_branch: implementation.branch.clone(),
        destination_branch: destination_branch.to_owned(),
        expected_base_sha: implementation.base_commit.clone(),
        reviewed_diff_hash: task
            .implementation_sha256
            .clone()
            .ok_or_else(|| anyhow::anyhow!("parent task has no reviewed diff hash"))?,
        included_task_uids: work.included_task_uids.clone(),
        accepted_spec_revision: spec_revision.seq,
        task_evidence,
    };
    scope
        .validate(&work.included_task_uids)
        .map_err(anyhow::Error::msg)?;
    anyhow::ensure!(
        scope.reviewed_diff_hash == task.implementation_sha256.unwrap_or_default(),
        "frozen PR scope differs from the completed parent diff"
    );
    Ok(scope)
}
