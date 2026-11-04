use super::*;
use std::{
    sync::{Arc, atomic::AtomicBool, mpsc},
    time::{Duration, Instant},
};

#[test]
fn task_metadata_source_sha_keeps_the_task_on_its_recorded_commit() {
    let temp = tempfile::tempdir().unwrap();
    let mut environment = crate::test_support::ProcessEnvironmentGuard::new();
    environment.set("KOOLADE_HOME", temp.path().join("operator-home"));
    let repo = temp.path().join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    let (progress, _updates) = mpsc::channel();
    let runner = Runner {
        gh: "gh".into(),
        runtime_config_source: None,
        deadline: Instant::now() + Duration::from_secs(30),
        cancel: Arc::new(AtomicBool::new(false)),
        progress,
        authorization_verifier: None,
    };
    runner.git(&repo, &["init", "-q", "-b", "main"]).unwrap();
    runner
        .git(&repo, &["config", "user.name", "Koolade fixture"])
        .unwrap();
    runner
        .git(&repo, &["config", "user.email", "fixture@koolade.invalid"])
        .unwrap();
    std::fs::write(repo.join("README.md"), "approved source\n").unwrap();
    runner.git(&repo, &["add", "README.md"]).unwrap();
    runner
        .git(&repo, &["commit", "--quiet", "-m", "approved source"])
        .unwrap();
    let approved_sha = runner.git(&repo, &["rev-parse", "HEAD"]).unwrap();
    std::fs::write(repo.join("README.md"), "advanced branch\n").unwrap();
    runner.git(&repo, &["add", "README.md"]).unwrap();
    runner
        .git(&repo, &["commit", "--quiet", "-m", "advance source branch"])
        .unwrap();
    let branch_head = runner.git(&repo, &["rev-parse", "HEAD"]).unwrap();

    let planning = PlanningStore::new(
        uuid::Uuid::nil(),
        temp.path().join("private-state"),
        crate::artifacts::planning_store::StoreMode::ManagedLocal,
    );
    let repository_manifest = crate::core::project_repos::ProjectManifest {
        repositories: vec![crate::core::project_repos::Repository {
            id: "root".into(),
            role: "Primary code".into(),
            remote: String::new(),
            display_name: None,
        }],
    };
    planning
        .atomic_write(
            crate::artifacts::layout::store::PROJECT_MANIFEST,
            &serde_json::to_vec(&repository_manifest).unwrap(),
        )
        .unwrap();
    let identity = crate::domain::ArtifactIdentity {
        uid: uuid::Uuid::new_v4().hyphenated().to_string(),
        display_id: "TASK-1".into(),
        title: "Pinned source fixture".into(),
        parent_uid: Some(uuid::Uuid::new_v4().hyphenated().to_string()),
    };
    let branches = crate::core::workflow::BranchTargets {
        source: "main".into(),
        destination: "feature".into(),
    };
    let metadata = crate::artifacts::task_docs::TaskMetadata::new(&identity, "root", vec![])
        .unwrap()
        .with_branch_targets(Some(&branches))
        .unwrap()
        .with_source_sha(Some(&approved_sha))
        .unwrap();
    let dir = temp.path().join("task-state");
    std::fs::create_dir_all(&dir).unwrap();
    let source = resolve(Request {
        planning_store: &planning,
        repo: &repo,
        dir: &dir,
        ticket: "task.md",
        metadata: Some(&metadata),
        publication_mode: PublicationMode::HoldForReview,
        runner: &runner,
    })
    .unwrap();

    assert_eq!(source.source_commit, approved_sha);
    assert_eq!(source.base_commit, source.source_commit);
    assert_eq!(
        source.cache.resolve_branch("main", &runner).unwrap(),
        branch_head
    );
}

#[test]
// Production boundary: TaskMetadata's repository and branch intent must reach
// RepositoryCache from the private checkout map. A resolver unit test cannot
// prove the task source/cache use the selected checkout. Uses local Git only,
// creates two temporary repositories, and has low setup cost.
fn secondary_task_source_uses_its_explicit_checkout_and_branch() {
    let temp = tempfile::tempdir().unwrap();
    let mut environment = crate::test_support::ProcessEnvironmentGuard::new();
    environment.set("KOOLADE_HOME", temp.path().join("operator-home"));
    let primary = temp.path().join("primary");
    let secondary = temp.path().join("api");
    let (progress, _updates) = mpsc::channel();
    let runner = Runner {
        gh: "gh".into(),
        runtime_config_source: None,
        deadline: Instant::now() + Duration::from_secs(30),
        cancel: Arc::new(AtomicBool::new(false)),
        progress,
        authorization_verifier: None,
    };
    for (path, name, remote, content) in [
        (
            &primary,
            "primary",
            "https://example.test/team/product.git",
            "primary source",
        ),
        (
            &secondary,
            "api",
            "https://example.test/team/api.git",
            "secondary source",
        ),
    ] {
        std::fs::create_dir_all(path).unwrap();
        runner.git(path, &["init", "-q", "-b", "main"]).unwrap();
        runner
            .git(path, &["config", "user.name", "Koolade fixture"])
            .unwrap();
        runner
            .git(path, &["config", "user.email", "fixture@koolade.invalid"])
            .unwrap();
        runner
            .git(path, &["remote", "add", "origin", remote])
            .unwrap();
        std::fs::write(path.join("README.md"), format!("{content}\n")).unwrap();
        runner.git(path, &["add", "README.md"]).unwrap();
        runner
            .git(path, &["commit", "--quiet", "-m", name])
            .unwrap();
    }
    let secondary_sha = runner.git(&secondary, &["rev-parse", "HEAD"]).unwrap();
    let planning = PlanningStore::for_code_repository(
        uuid::Uuid::new_v4(),
        temp.path().join("private-state"),
        &primary,
    );
    let manifest = crate::core::project_repos::ProjectManifest {
        repositories: vec![
            crate::core::project_repos::Repository {
                id: "root".into(),
                role: "Primary code".into(),
                remote: "https://example.test/team/product.git".into(),
                display_name: None,
            },
            crate::core::project_repos::Repository {
                id: "api".into(),
                role: "Backend API".into(),
                remote: "https://example.test/team/api.git".into(),
                display_name: None,
            },
        ],
    };
    planning
        .atomic_write(
            crate::artifacts::layout::store::PROJECT_MANIFEST,
            &serde_json::to_vec(&manifest).unwrap(),
        )
        .unwrap();
    let revision = planning.revision().unwrap();
    crate::core::project_repos::connect_checkout(
        &planning, &manifest, &primary, "api", &secondary, &revision,
    )
    .unwrap();
    let identity = crate::domain::ArtifactIdentity {
        uid: uuid::Uuid::new_v4().hyphenated().to_string(),
        display_id: "TASK-API".into(),
        title: "Secondary repository task".into(),
        parent_uid: Some(uuid::Uuid::new_v4().hyphenated().to_string()),
    };
    let branches = crate::core::workflow::BranchTargets {
        source: "main".into(),
        destination: "api-task".into(),
    };
    let metadata = crate::artifacts::task_docs::TaskMetadata::new(&identity, "api", vec![])
        .unwrap()
        .with_branch_targets(Some(&branches))
        .unwrap()
        .with_source_sha(Some(&secondary_sha))
        .unwrap();
    let dir = temp.path().join("task-state");
    std::fs::create_dir_all(&dir).unwrap();
    let source = resolve(Request {
        planning_store: &planning,
        repo: &primary,
        dir: &dir,
        ticket: "task.md",
        metadata: Some(&metadata),
        publication_mode: PublicationMode::HoldForReview,
        runner: &runner,
    })
    .unwrap();

    assert_eq!(source.repository_id, "api");
    assert_eq!(source.source_commit, secondary_sha);
    assert_eq!(source.destination_branch, "api-task");
    assert_eq!(
        source.cache.origin_url.as_deref(),
        Some("https://example.test/team/api.git")
    );
    assert_eq!(
        planning.published_repository_root.as_deref(),
        Some(primary.as_path())
    );
}

#[test]
// Production boundary: a resumed reconciliation may reuse a clone only when
// the TaskIntent still names that repository. A unit test of metadata parsing
// cannot prove the saved-plan resume fence; Git is not invoked because the
// mismatch must be rejected before cache resolution.
fn resume_rejects_a_saved_clone_for_a_different_task_repository() {
    let temp = tempfile::tempdir().unwrap();
    let (progress, _updates) = mpsc::channel();
    let runner = Runner {
        gh: "gh".into(),
        runtime_config_source: None,
        deadline: Instant::now() + Duration::from_secs(30),
        cancel: Arc::new(AtomicBool::new(false)),
        progress,
        authorization_verifier: None,
    };
    let planning = PlanningStore::new(
        uuid::Uuid::new_v4(),
        temp.path().join("private-state"),
        crate::artifacts::planning_store::StoreMode::ManagedLocal,
    );
    let identity = crate::domain::ArtifactIdentity {
        uid: uuid::Uuid::new_v4().hyphenated().to_string(),
        display_id: "TASK-RESUME".into(),
        title: "Changed target".into(),
        parent_uid: Some(uuid::Uuid::new_v4().hyphenated().to_string()),
    };
    let branches = crate::core::workflow::BranchTargets {
        source: "main".into(),
        destination: "feature".into(),
    };
    let metadata = crate::artifacts::task_docs::TaskMetadata::new(&identity, "api", vec![])
        .unwrap()
        .with_branch_targets(Some(&branches))
        .unwrap();
    let plan = crate::core::implementation::initial_reconciliation::Plan {
        schema_version: 1,
        base: "main".into(),
        local_commit: "local".into(),
        remote_commit: "remote".into(),
        common_base: "base".into(),
        required_verification: Vec::new(),
        verified_commit: None,
        verification: Vec::new(),
        clone_repository: Some(
            crate::core::implementation::initial_reconciliation::CloneRepositoryPlan {
                repository_id: "root".into(),
                repository_identity: "saved-identity".into(),
                source_ref: "main".into(),
                destination_branch: "feature".into(),
            },
        ),
    };
    let error = resume_from_plan(
        &planning,
        temp.path(),
        "task.md",
        Some(&metadata),
        &plan,
        &runner,
    )
    .err()
    .unwrap();
    assert!(error.to_string().contains("Task repository intent changed"));
}
