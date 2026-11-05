use super::*;
use std::{
    net::{TcpListener, TcpStream},
    process::{Child, Command, Stdio},
    sync::{Arc, atomic::AtomicBool, mpsc},
    time::{Duration, Instant},
};

struct GitDaemon(Child);

impl Drop for GitDaemon {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn approved_intent(
    repository_id: &str,
    branches: &crate::core::workflow::BranchTargets,
    source_sha: &str,
) -> crate::core::git_intent::GitIntentV1 {
    crate::core::git_intent::GitIntentV1::approve(
        repository_id.into(),
        branches.source.clone(),
        source_sha.into(),
        branches.destination.clone(),
        crate::core::git_intent::PublicationModeV1::HoldForReview,
        None,
    )
    .unwrap()
}

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
    runner.git(&repo, &["branch", "feature"]).unwrap();
    let intent = approved_intent("root", &branches, &approved_sha);
    let metadata = crate::artifacts::task_docs::TaskMetadata::new(&identity, "root", vec![])
        .unwrap()
        .with_branch_targets(Some(&branches))
        .unwrap()
        .with_source_sha(Some(&approved_sha))
        .unwrap()
        .with_git_intent(Some(&intent))
        .unwrap();
    let dir = temp.path().join("task-state");
    std::fs::create_dir_all(&dir).unwrap();
    let source = resolve(Request {
        planning_store: &planning,
        code_root: &repo,
        dir: &dir,
        ticket: "planning/tasks/feature/001-pinned.md",
        metadata: Some(&metadata),
        runner: &runner,
    })
    .unwrap();

    std::fs::write(repo.join("README.md"), "advanced branch\n").unwrap();
    runner.git(&repo, &["add", "README.md"]).unwrap();
    runner
        .git(&repo, &["commit", "--quiet", "-m", "advance source branch"])
        .unwrap();
    let branch_head = runner.git(&repo, &["rev-parse", "HEAD"]).unwrap();

    assert_eq!(source.source_commit, approved_sha);
    assert_eq!(source.base_commit, source.source_commit);
    assert_eq!(
        source.cache.resolve_branch("main", &runner).unwrap(),
        approved_sha
    );
    assert_ne!(branch_head, source.source_commit);
    let after_move = resolve(Request {
        planning_store: &planning,
        code_root: &repo,
        dir: &dir,
        ticket: "planning/tasks/feature/001-pinned.md",
        metadata: Some(&metadata),
        runner: &runner,
    })
    .unwrap();
    assert_eq!(after_move.source_commit, approved_sha);
    assert_eq!(
        after_move.cache.resolve_branch("main", &runner).unwrap(),
        approved_sha
    );
}

// Production boundary: first task allocation must verify the destination
// still exists on the configured remote after planning. A validator unit test
// cannot prove this live Git advertisement is consulted. Uses only a local
// bare Git remote; setup cost is one commit and push.
#[test]
fn first_start_rejects_a_destination_deleted_after_approval() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("checkout");
    let remote = temp.path().join("remote.git");
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
    runner
        .git(
            temp.path(),
            &["init", "--bare", "-q", remote.to_str().unwrap()],
        )
        .unwrap();
    runner.git(&repo, &["init", "-q", "-b", "main"]).unwrap();
    runner
        .git(&repo, &["config", "user.name", "Koolade fixture"])
        .unwrap();
    runner
        .git(&repo, &["config", "user.email", "fixture@koolade.invalid"])
        .unwrap();
    std::fs::write(repo.join("README.md"), "base\n").unwrap();
    runner.git(&repo, &["add", "README.md"]).unwrap();
    runner.git(&repo, &["commit", "-q", "-m", "base"]).unwrap();
    runner
        .git(
            &repo,
            &["remote", "add", "origin", remote.to_str().unwrap()],
        )
        .unwrap();
    runner.git(&repo, &["push", "origin", "main"]).unwrap();
    let cache = RepositoryCache {
        identity: "synthetic-test-remote".into(),
        path: repo.clone(),
        origin_url: Some(remote.to_string_lossy().into_owned()),
        push_url: None,
        push_identity_url: None,
        fetch_url: Some(remote.to_string_lossy().into_owned()),
    };

    validate_destination(&cache, &repo, "synthetic-test-remote", "main", &runner).unwrap();
    runner.git(&repo, &["push", "origin", ":main"]).unwrap();
    assert!(validate_destination(&cache, &repo, "synthetic-test-remote", "main", &runner).is_err());
}

#[test]
// Production boundary: TaskMetadata's repository and branch intent must reach
// RepositoryCache from the private checkout map. A resolver unit test cannot
// prove the task source/cache use the selected checkout. Uses local Git and a
// loopback git daemon; setup cost is two repositories and one short-lived process.
fn secondary_task_source_uses_its_explicit_checkout_and_branch() {
    let temp = tempfile::tempdir().unwrap();
    let mut environment = crate::test_support::ProcessEnvironmentGuard::new();
    environment.set("KOOLADE_HOME", temp.path().join("operator-home"));
    let primary = temp.path().join("primary");
    let secondary = temp.path().join("api");
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    let daemon = Command::new("git")
        .args([
            "daemon",
            "--reuseaddr",
            &format!("--base-path={}", temp.path().display()),
            "--export-all",
            "--enable=receive-pack",
            "--listen=127.0.0.1",
            &format!("--port={port}"),
            temp.path().to_str().unwrap(),
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let _daemon = GitDaemon(daemon);
    let endpoint = ("127.0.0.1", port);
    for _ in 0..100 {
        if TcpStream::connect(endpoint).is_ok() {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let product_url = format!("git://127.0.0.1:{port}/product.git");
    let api_url = format!("git://127.0.0.1:{port}/api.git");
    let (progress, _updates) = mpsc::channel();
    let runner = Runner {
        gh: "gh".into(),
        runtime_config_source: None,
        deadline: Instant::now() + Duration::from_secs(30),
        cancel: Arc::new(AtomicBool::new(false)),
        progress,
        authorization_verifier: None,
    };
    for (path, name, remote_name, remote_url, content) in [
        (
            &primary,
            "primary",
            "product.git",
            product_url.as_str(),
            "primary source",
        ),
        (
            &secondary,
            "api",
            "api.git",
            api_url.as_str(),
            "secondary source",
        ),
    ] {
        let remote = temp.path().join(remote_name);
        runner
            .git(
                temp.path(),
                &["init", "--bare", "-q", remote.to_str().unwrap()],
            )
            .unwrap();
        std::fs::create_dir_all(path).unwrap();
        runner.git(path, &["init", "-q", "-b", "main"]).unwrap();
        runner
            .git(path, &["config", "user.name", "Koolade fixture"])
            .unwrap();
        runner
            .git(path, &["config", "user.email", "fixture@koolade.invalid"])
            .unwrap();
        runner
            .git(path, &["remote", "add", "origin", remote_url])
            .unwrap();
        std::fs::write(path.join("README.md"), format!("{content}\n")).unwrap();
        runner.git(path, &["add", "README.md"]).unwrap();
        runner
            .git(path, &["commit", "--quiet", "-m", name])
            .unwrap();
        runner.git(path, &["push", "-q", "origin", "main"]).unwrap();
        if name == "api" {
            runner
                .git(path, &["push", "-q", "origin", "main:api-task"])
                .unwrap();
        }
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
                remote: product_url.clone(),
                display_name: None,
            },
            crate::core::project_repos::Repository {
                id: "api".into(),
                role: "Backend API".into(),
                remote: api_url.clone(),
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
    let intent = approved_intent("api", &branches, &secondary_sha);
    let metadata = crate::artifacts::task_docs::TaskMetadata::new(&identity, "api", vec![])
        .unwrap()
        .with_branch_targets(Some(&branches))
        .unwrap()
        .with_source_sha(Some(&secondary_sha))
        .unwrap()
        .with_git_intent(Some(&intent))
        .unwrap();
    let dir = temp.path().join("task-state");
    std::fs::create_dir_all(&dir).unwrap();
    let source = resolve(Request {
        planning_store: &planning,
        code_root: &primary,
        dir: &dir,
        ticket: "planning/tasks/feature/001-api.md",
        metadata: Some(&metadata),
        runner: &runner,
    })
    .unwrap();

    assert_eq!(source.repository_id, "api");
    assert_eq!(source.source_commit, secondary_sha);
    assert_eq!(source.destination_branch, "api-task");
    assert_eq!(source.cache.origin_url.as_deref(), Some(api_url.as_str()));
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
    let source_sha = "a".repeat(40);
    let intent = approved_intent("api", &branches, &source_sha);
    let metadata = crate::artifacts::task_docs::TaskMetadata::new(&identity, "api", vec![])
        .unwrap()
        .with_branch_targets(Some(&branches))
        .unwrap()
        .with_source_sha(Some(&source_sha))
        .unwrap()
        .with_git_intent(Some(&intent))
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
