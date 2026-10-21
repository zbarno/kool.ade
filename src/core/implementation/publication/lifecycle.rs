use super::super::{Implementation, PullRequestState};
use crate::{
    artifacts::planning_store::PlanningStore,
    core::lifecycle::{
        GateActor, GateDecision, GateReceipt, LifecycleStore, PullRequestPreparationStatus,
    },
};

mod recovery;
#[cfg(test)]
pub(in crate::core::implementation) mod tests;
pub(in crate::core::implementation) use recovery::preserve_done_after_failure;

#[derive(Clone)]
pub(in crate::core::implementation) struct LifecyclePublication {
    work_item_uid: String,
}

pub(in crate::core::implementation) enum LifecyclePublicationGate {
    Legacy,
    Blocked(String),
    Authorized(LifecyclePublication),
}

pub(in crate::core::implementation) fn implementation_is_done(
    store: &PlanningStore,
    implementation: &Implementation,
) -> anyhow::Result<bool> {
    let Some(uid) = implementation.task_uid.as_deref() else {
        return Ok(false);
    };
    Ok(LifecycleStore::new(store)
        .task_for_planning_item(uid)?
        .is_some_and(|task| task.phase == crate::core::lifecycle::TaskPhase::Done))
}

/// The explicit Create PR action is the user authorization boundary. Persist
/// it before any remote write; a restart can safely resume an ambiguous push.
pub(in crate::core::implementation) fn prepare_lifecycle_publication(
    store: &PlanningStore,
    implementation: &Implementation,
) -> anyhow::Result<LifecyclePublicationGate> {
    let Some(planning_item_uid) = implementation.task_uid.as_deref() else {
        return Ok(LifecyclePublicationGate::Legacy);
    };
    let lifecycle = LifecycleStore::new(store);
    let Some(task) = lifecycle.task_for_planning_item(planning_item_uid)? else {
        return Ok(LifecyclePublicationGate::Legacy);
    };
    if task.phase != crate::core::lifecycle::TaskPhase::Done {
        return Ok(LifecyclePublicationGate::Blocked(format!(
            "lifecycle task is {:?}; independent QA and specification reconciliation must complete before PR preparation",
            task.phase
        )));
    }
    // Integration can produce a new verified commit after the task's review,
    // QA, and specification receipts were frozen. Verification alone cannot
    // transfer those approvals to the integrated head, including on retries.
    if implementation
        .verified_head
        .as_deref()
        .is_none_or(|head| task.reviewed_code_head_sha.as_deref() != Some(head))
    {
        return Ok(LifecyclePublicationGate::Blocked(
            "publication head differs from the independently reviewed task head; refresh code review, runtime QA, and specification reconciliation for the integrated head before publication".into(),
        ));
    }
    let Some(work_item_uid) = task.pr_work_item_uid.clone() else {
        anyhow::bail!("completed lifecycle task has no PR preparation item");
    };
    let work = lifecycle.load_pr_preparation(&work_item_uid)?;
    lifecycle.ensure_product_specification_current(&task.feature_uid)?;
    match work.status {
        PullRequestPreparationStatus::AwaitingAuthorization
        | PullRequestPreparationStatus::NeedsAttention
            if work.publication_authorization.is_none() =>
        {
            let subject_sha256 = task
                .implementation_sha256
                .as_deref()
                .ok_or_else(|| anyhow::anyhow!("lifecycle task has no reviewed code hash"))?;
            let receipt = GateReceipt {
                receipt_uid: uuid::Uuid::new_v4().hyphenated().to_string(),
                subject_uid: work_item_uid.clone(),
                subject_sha256: subject_sha256.to_owned(),
                actor: GateActor::User,
                decision: GateDecision::Approved,
                recorded_at_unix_ms: now_ms(),
                evidence_refs: vec![format!("user-action:create-pr:{work_item_uid}")],
                prompt_sha256: None,
                prompt_manifest: None,
                reviewer_run_id: None,
                author_run_id: None,
                reviewer_session_id: None,
                author_session_id: None,
            };
            let event_uid = event_uid(&work_item_uid, &receipt.receipt_uid);
            lifecycle.authorize_pr_publication(
                &work_item_uid,
                work.revision,
                &event_uid,
                &receipt,
            )?;
        }
        PullRequestPreparationStatus::Publishing | PullRequestPreparationStatus::NeedsAttention => {
            anyhow::ensure!(
                work.publication_authorization
                    .as_ref()
                    .is_some_and(|receipt| {
                        receipt.subject_sha256
                            == task.implementation_sha256.as_deref().unwrap_or_default()
                    }),
                "lifecycle publication authorization is stale for the reviewed task"
            );
            lifecycle.authorize_pr_publication(
                &work_item_uid,
                work.revision,
                &uuid::Uuid::new_v4().to_string(),
                work.publication_authorization
                    .as_ref()
                    .expect("checked authorization"),
            )?;
        }
        PullRequestPreparationStatus::Opened => {
            anyhow::ensure!(
                work.provider_pr_url.is_some(),
                "opened lifecycle PR has no provider URL"
            );
        }
        status => anyhow::bail!("lifecycle PR work item is not publishable in {status:?}"),
    }
    Ok(LifecyclePublicationGate::Authorized(LifecyclePublication {
        work_item_uid,
    }))
}

pub(in crate::core::implementation) fn finish_lifecycle_publication(
    store: &PlanningStore,
    implementation: &Implementation,
    binding: Option<LifecyclePublication>,
) -> anyhow::Result<()> {
    let Some(binding) = binding else {
        return Ok(());
    };
    let lifecycle = LifecycleStore::new(store);
    let work = lifecycle.load_pr_preparation(&binding.work_item_uid)?;
    let provider_pr_url = implementation
        .pr_url
        .clone()
        .ok_or_else(|| anyhow::anyhow!("PR publisher returned without a provider URL"))?;
    let details = implementation
        .pr_details
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("PR publisher returned without reviewed head details"))?;
    let status = match implementation.pr_state {
        Some(PullRequestState::Open) => PullRequestPreparationStatus::Opened,
        Some(PullRequestState::Closed) => PullRequestPreparationStatus::ClosedUnmerged,
        Some(PullRequestState::Merged) => {
            anyhow::bail!("PR publisher reported merged without an authoritative merge commit")
        }
        None => anyhow::bail!("PR publisher did not report a provider pull request state"),
    };
    let event_uid = event_uid(
        &binding.work_item_uid,
        &format!("{status:?}:{}:{}", details.number, details.head_sha),
    );
    lifecycle.record_pr_publication_state(
        &binding.work_item_uid,
        work.revision,
        &event_uid,
        crate::core::lifecycle::PullRequestPublicationState {
            status,
            provider_pr_id: Some(details.number),
            provider_pr_url: Some(provider_pr_url),
            head_sha: Some(details.head_sha.clone()),
            base_sha: details
                .base_sha
                .clone()
                .or_else(|| Some(implementation.base_commit.clone())),
            merged_commit_sha: None,
        },
    )?;
    Ok(())
}

pub(in crate::core::implementation) fn reconcile_lifecycle_pr_state(
    store: &PlanningStore,
    implementation: &Implementation,
    target_repository: &std::path::Path,
) -> anyhow::Result<()> {
    let (Some(planning_item_uid), Some(provider_pr_url), Some(pr_state)) = (
        implementation.task_uid.as_deref(),
        implementation.pr_url.as_deref(),
        implementation.pr_state,
    ) else {
        return Ok(());
    };
    let lifecycle = LifecycleStore::new(store);
    let Some(task) = lifecycle.task_for_planning_item(planning_item_uid)? else {
        return Ok(());
    };
    let Some(work_item_uid) = task.pr_work_item_uid.as_deref() else {
        return Ok(());
    };
    let work = lifecycle.load_pr_preparation(work_item_uid)?;
    let Some(details) = implementation.pr_details.as_ref() else {
        anyhow::bail!("provider PR state has no validated head identity");
    };
    let status = match pr_state {
        PullRequestState::Open => PullRequestPreparationStatus::Opened,
        PullRequestState::Closed => PullRequestPreparationStatus::ClosedUnmerged,
        PullRequestState::Merged if implementation.merged_commit.is_some() => {
            PullRequestPreparationStatus::Merged
        }
        PullRequestState::Merged => {
            anyhow::bail!("provider reports merged PR without an authoritative merge commit")
        }
    };
    let event_uid = event_uid(
        work_item_uid,
        &format!(
            "{:?}:{}:{}:{}:{}",
            status,
            details.number,
            details.head_sha,
            work.revision,
            implementation
                .merged_commit
                .as_deref()
                .unwrap_or("unmerged")
        ),
    );
    lifecycle.record_pr_publication_state(
        work_item_uid,
        work.revision,
        &event_uid,
        crate::core::lifecycle::PullRequestPublicationState {
            status,
            provider_pr_id: Some(details.number),
            provider_pr_url: Some(provider_pr_url.to_owned()),
            head_sha: Some(details.head_sha.clone()),
            base_sha: work.base_sha.clone().or_else(|| {
                details
                    .base_sha
                    .clone()
                    .or_else(|| Some(implementation.base_commit.clone()))
            }),
            merged_commit_sha: implementation.merged_commit.clone(),
        },
    )?;
    if status == PullRequestPreparationStatus::Merged {
        let repository_id = implementation.repository_id.as_deref().unwrap_or("root");
        lifecycle
            .record_verified_current_implementation(
                &task.task_uid,
                repository_id,
                target_repository,
            )
            .map_err(|error| {
                anyhow::anyhow!("merged PR was not promoted to current implementation: {error}")
            })?;
    }
    Ok(())
}

fn event_uid(work_item_uid: &str, event: &str) -> String {
    let namespace = uuid::Uuid::parse_str(work_item_uid).unwrap_or(uuid::Uuid::NAMESPACE_OID);
    uuid::Uuid::new_v5(&namespace, format!("pr-lifecycle-v1:{event}").as_bytes())
        .hyphenated()
        .to_string()
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u64::MAX as u128) as u64
}
