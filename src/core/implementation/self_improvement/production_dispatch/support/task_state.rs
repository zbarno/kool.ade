use crate::{
    core::implementation::{Implementation, ImplementationStatus, TaskRepositoryKind},
    execution::session::AuthorizationGenerationVerifier,
};
use std::path::Path;

pub(crate) fn implementation_state(
    ticket: &str,
    planning_item_uid: &str,
    repository_uid: &str,
    project_slug: &str,
    source_sha: &str,
    task_repository: &std::path::Path,
) -> crate::core::implementation::Implementation {
    Implementation {
        ticket: ticket.into(),
        task_uid: Some(planning_item_uid.into()),
        ticket_text: "Approved implementation task".into(),
        approved_specification: None,
        approved_product_context: None,
        completed_dependency_context: None,
        manager_repair_context: None,
        branch: "koolade/self-improvement-fixture".into(),
        source_branch: Some("koolade/self-improvement-fixture".into()),
        destination_branch: Some("main".into()),
        source_ref: Some("koolade/self-improvement-fixture".into()),
        source_commit: Some(source_sha.into()),
        repository_id: Some(repository_uid.into()),
        project_id: Some(project_slug.into()),
        workspace_uid: None,
        repository_identity: None,
        push_repository: None,
        repository_cache: None,
        task_repository_allocation_key: Some(crate::core::implementation::key(ticket)),
        base: "main".into(),
        base_commit: source_sha.into(),
        task_repository: task_repository.into(),
        task_repository_kind: TaskRepositoryKind::Clone,
        task_repository_ready: true,
        task_repositories: vec![task_repository.into()],
        task_repository_commits: Default::default(),
        status: ImplementationStatus::Preparing,
        detail: String::new(),
        pr_url: None,
        verified_head: None,
        auto_merge: false,
        merged_commit: None,
        pr_state: None,
        pr_details: None,
        pr_checked_at: None,
        pr_check_attempted_at: None,
        pr_check_error: None,
        independent_check: None,
        cleanup: Default::default(),
    }
}

pub(crate) fn assert_changed_persisted_source_is_rejected(
    state_path: &Path,
    implementation: &Implementation,
    verifier: &dyn AuthorizationGenerationVerifier,
    generation: &str,
) {
    let mut changed = implementation.clone();
    changed.source_commit = Some("f".repeat(40));
    std::fs::write(
        state_path,
        crate::core::implementation::serialize_state(&changed).unwrap(),
    )
    .unwrap();
    assert!(verifier.verify(generation).is_err());
    std::fs::write(
        state_path,
        crate::core::implementation::serialize_state(implementation).unwrap(),
    )
    .unwrap();
    verifier.verify(generation).unwrap();
}
