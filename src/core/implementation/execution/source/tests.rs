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

    let planning = PlanningStore::legacy_embedded(uuid::Uuid::nil(), &repo);
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
        text: "Pinned source fixture",
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
