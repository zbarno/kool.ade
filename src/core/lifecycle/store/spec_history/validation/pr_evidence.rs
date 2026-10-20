pub(super) fn provider_pr_state_evidence_ref(
    feature_uid: &str,
    task_uid: &str,
    preparation: &super::super::super::task_transition::PullRequestPreparation,
) -> Result<String, crate::artifacts::planning_store::StoreError> {
    if preparation.feature_uid != feature_uid
        || preparation.task_uid != task_uid
        || !preparation
            .included_task_uids
            .iter()
            .any(|included| included == task_uid)
    {
        return Err(
            crate::artifacts::planning_store::StoreError::MalformedState(
                "pull request evidence belongs to a different feature or task".into(),
            ),
        );
    }
    Ok(format!(
        "provider-pr-state:{}:{}",
        preparation.work_item_uid, preparation.revision
    ))
}

pub(super) fn persisted_provider_pr_state_evidence_ref(
    lifecycle: &super::super::super::LifecycleStore<'_>,
    feature_uid: &str,
    task_uid: &str,
    work_item_uid: &str,
) -> Result<String, crate::artifacts::planning_store::StoreError> {
    let preparation = lifecycle.load_pr_preparation(work_item_uid)?;
    provider_pr_state_evidence_ref(feature_uid, task_uid, &preparation)
}

#[cfg(test)]
#[test]
fn pr_state_evidence_reference_is_bound_to_exact_persisted_revision() {
    let uid = "00000000-0000-4000-8000-000000000123";
    let task = "00000000-0000-4000-8000-000000000456";
    let mut pr = crate::core::lifecycle::PullRequestPreparation {
        schema_version: 1,
        uid: uid.into(),
        work_item_uid: uid.into(),
        feature_uid: "00000000-0000-4000-8000-000000000789".into(),
        task_uid: task.into(),
        included_task_uids: vec![task.into()],
        revision: 7,
        created_at_ms: 1,
        updated_at_ms: 1,
        status: crate::core::lifecycle::PullRequestPreparationStatus::Merged,
        publication_authorization: None,
        recovery: None,
        provider_pr_id: Some(1),
        provider_pr_url: Some("https://github.com/example/repo/pull/1".into()),
        head_sha: Some("a".repeat(40)),
        base_sha: Some("b".repeat(40)),
        merged_commit_sha: Some("c".repeat(40)),
    };
    let feature = pr.feature_uid.clone();
    let reference = provider_pr_state_evidence_ref(&feature, task, &pr).unwrap();
    let known = std::collections::HashSet::from([reference]);
    assert!(known.contains(&format!("provider-pr-state:{uid}:7")));
    assert!(!known.contains(&format!("provider-pr-state:{uid}:8")));
    pr.task_uid = "00000000-0000-4000-8000-000000000999".into();
    assert!(provider_pr_state_evidence_ref(&feature, task, &pr).is_err());
}

#[cfg(test)]
#[path = "pr_evidence/tests.rs"]
mod tests;
