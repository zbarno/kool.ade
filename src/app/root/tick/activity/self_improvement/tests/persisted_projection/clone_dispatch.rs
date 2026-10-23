use super::*;

/// Production boundary: the app synchronizer may enter Implementing only after
/// the normal task implementation reports a ready isolated clone matching the
/// approved task, repository, source SHA, branch pair, and current canonical
/// product remote. A core state-machine test cannot prove the app projection
/// checks the persisted manifest and implementation record together. Uses the
/// headless app fixture and synthetic implementation records; no Docker or
/// network. Setup cost is low: one persisted feature task and repeated syncs.
pub(super) fn verify_clone_dispatch(
    app: &mut crate::app::root::KooladeApp,
    lifecycle: &crate::core::lifecycle::LifecycleStore<'_>,
    runs: &crate::core::self_improvement::SelfImprovementStore<'_>,
    run_uid: &str,
    task_uid: &str,
) {
    use crate::core::self_improvement::SelfImprovementPhaseV1 as Phase;

    let run = runs.load(run_uid).unwrap();
    assert_eq!(run.phase, Phase::Ready);
    let clone_path = std::env::temp_dir().join(format!("task-clone-{task_uid}"));
    for mismatch in [
        CloneMismatch::Task,
        CloneMismatch::Repository,
        CloneMismatch::Kind,
        CloneMismatch::NotReady,
        CloneMismatch::PathNotRegistered,
        CloneMismatch::SourceSha,
        CloneMismatch::SourceBranch,
        CloneMismatch::DestinationBranch,
        CloneMismatch::RepositoryIdentity,
    ] {
        let mut target =
            implementation_record(&run, task_uid, &clone_path, &run.target_repository_uid);
        mismatch.apply(&mut target);
        insert_implementation(app, target);
        synchronize_app_run(app, lifecycle, runs, run_uid).unwrap();
        let still_ready = runs.load(run_uid).unwrap();
        assert_eq!(still_ready.phase, Phase::Ready, "accepted {mismatch:?}");
        assert!(
            still_ready.clone_uid.is_none(),
            "allocated clone for {mismatch:?}"
        );
        assert_app_idle(app);
    }

    let valid_target =
        implementation_record(&run, task_uid, &clone_path, &run.target_repository_uid);
    insert_implementation(app, valid_target);

    let planning = match &app.screen {
        crate::app::root::Screen::Connected(project) => project.state.planning_store.clone(),
        crate::app::root::Screen::Welcome => panic!("fixture should remain connected"),
    };
    let original_manifest = crate::core::project_repos::ProjectManifest::load(&planning).unwrap();
    let mut changed_manifest = original_manifest.clone();
    changed_manifest
        .repositories
        .iter_mut()
        .find(|repository| repository.id == "root")
        .unwrap()
        .remote = "https://github.com/attacker/kool.ade".into();
    save_manifest(&planning, &changed_manifest);
    let error = synchronize_app_run(app, lifecycle, runs, run_uid).unwrap_err();
    assert!(error.contains("canonical product remote"), "{error}");
    let still_ready = runs.load(run_uid).unwrap();
    assert_eq!(still_ready.phase, Phase::Ready);
    assert!(still_ready.clone_uid.is_none());
    assert_app_idle(app);

    save_manifest(&planning, &original_manifest);
    synchronize_app_run(app, lifecycle, runs, run_uid).unwrap();
    let implementing = runs.load(run_uid).unwrap();
    assert_eq!(implementing.phase, Phase::Implementing);
    assert!(implementing.clone_uid.is_some());
    assert_app_idle(app);
}

fn save_manifest(
    planning: &crate::artifacts::planning_store::PlanningStore,
    manifest: &crate::core::project_repos::ProjectManifest,
) {
    planning
        .transaction(
            &[(
                crate::artifacts::planning_store::paths::PROJECT_MANIFEST.to_owned(),
                serde_json::to_vec(manifest).unwrap(),
            )],
            None,
        )
        .unwrap();
}

#[derive(Debug, Clone, Copy)]
enum CloneMismatch {
    Task,
    Repository,
    Kind,
    NotReady,
    PathNotRegistered,
    SourceSha,
    SourceBranch,
    DestinationBranch,
    RepositoryIdentity,
}

impl CloneMismatch {
    fn apply(self, target: &mut crate::core::implementation::Implementation) {
        use crate::core::implementation::TaskRepositoryKind;

        match self {
            Self::Task => target.task_uid = Some(uuid::Uuid::new_v4().to_string()),
            Self::Repository => target.repository_id = Some("wrong-repository".into()),
            Self::Kind => target.task_repository_kind = TaskRepositoryKind::LegacyWorktree,
            Self::NotReady => target.task_repository_ready = false,
            Self::PathNotRegistered => target.task_repositories.clear(),
            Self::SourceSha => target.source_commit = Some("wrong-source-sha".into()),
            Self::SourceBranch => target.source_branch = Some("feat/wrong-source".into()),
            Self::DestinationBranch => target.destination_branch = Some("release".into()),
            Self::RepositoryIdentity => target.repository_identity = None,
        }
    }
}

fn implementation_record(
    run: &crate::core::self_improvement::SelfImprovementRunV1,
    task_uid: &str,
    clone_path: &std::path::Path,
    repository_id: &str,
) -> crate::core::implementation::Implementation {
    use crate::core::implementation::{Implementation, ImplementationStatus, TaskRepositoryKind};

    Implementation {
        ticket: format!("planning/tasks/{task_uid}.md"),
        task_uid: Some(task_uid.into()),
        workspace_uid: Some(run.target_repository_uid.clone()),
        ticket_text: "Implement the approved self-improvement task".into(),
        approved_specification: None,
        approved_product_context: None,
        completed_dependency_context: None,
        manager_repair_context: None,
        branch: run.source_ref.clone().unwrap(),
        source_branch: run.source_ref.clone(),
        destination_branch: run.destination_ref.clone(),
        source_ref: run.source_ref.clone(),
        source_commit: Some(run.source_sha.clone()),
        repository_id: Some(repository_id.into()),
        project_id: Some(run.target_repository_uid.clone()),
        repository_identity: Some("fixture-repository-identity".into()),
        push_repository: None,
        repository_cache: None,
        task_repository_allocation_key: Some(task_uid.into()),
        base: run.destination_ref.clone().unwrap(),
        base_commit: run.source_sha.clone(),
        task_repository: clone_path.to_path_buf(),
        task_repository_kind: TaskRepositoryKind::Clone,
        task_repository_ready: true,
        task_repositories: vec![clone_path.to_path_buf()],
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

fn insert_implementation(
    app: &mut crate::app::root::KooladeApp,
    implementation: crate::core::implementation::Implementation,
) {
    let crate::app::root::Screen::Connected(project) = &mut app.screen else {
        panic!("fixture should remain connected")
    };
    project
        .implementation_states
        .insert(implementation.ticket.clone(), implementation);
}
