use super::*;

use crate::core::self_improvement::{SelfImprovementEventV1, SelfImprovementPhaseV1};
use std::process::Command;

/// Production boundary: exercise the dispatch authorization hook using persisted
/// plan/reviewer receipts, lifecycle task metadata, and self-improvement state.
/// The opt-in CI branch passes that returned verifier into the app-owned Docker
/// session, proves an authorized command runs in the task clone, then proves run
/// cancellation blocks the next command. A unit test cannot prove this wiring
/// across runner dispatch, session creation, and Docker operations. External
/// dependency: Linux rootless Docker with the pinned General and Rust images.
/// Expected setup cost: one temporary Git repo, persisted planning/lifecycle
/// records, both image smoke receipts, one task clone, and two short commands.
#[test]
fn production_dispatch_verifier_binds_approved_plan_task_source_and_revocation() {
    let temp = tempfile::tempdir().unwrap();
    let mut environment = crate::test_support::ProcessEnvironmentGuard::new();
    environment.set("KOOLADE_HOME", temp.path().join("operator-home"));
    let repo = temp.path().join("product");
    std::fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "--quiet", "--initial-branch=main"]);
    git(
        &repo,
        &[
            "remote",
            "add",
            "origin",
            "https://github.com/zbarno/kool.ade",
        ],
    );
    std::fs::write(repo.join("README.md"), "fixture\n").unwrap();
    git(&repo, &["add", "README.md"]);
    git(
        &repo,
        &[
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@koolade.invalid",
            "commit",
            "--quiet",
            "-m",
            "fixture baseline",
        ],
    );
    let source_sha = git(&repo, &["rev-parse", "HEAD"]);

    let ReadyRunFixture {
        planning,
        workspace_uid,
        lifecycle_task_uid,
        planning_item_uid,
        ready,
    } = build_ready_run(temp.path(), &repo, &source_sha);
    let runs = crate::core::self_improvement::SelfImprovementStore::new(&planning);

    let state_root = temp.path().join("state-root");
    std::fs::create_dir_all(&state_root).unwrap();
    let ticket = format!("{planning_item_uid}.md");
    let allocation_key = super::super::super::super::key(&ticket);
    let project_slug = crate::persistence::project_slug(&state_root.canonicalize().unwrap());
    let repository_id = "root";
    let task_repository = super::super::super::super::task_repository::allocated_path(
        &project_slug,
        repository_id,
        &allocation_key,
    )
    .unwrap();
    std::fs::create_dir_all(task_repository.parent().unwrap()).unwrap();
    let clone_output = Command::new("git")
        .args(["clone", "--quiet", "--no-hardlinks", "--"])
        .arg(&repo)
        .arg(&task_repository)
        .output()
        .unwrap();
    assert!(
        clone_output.status.success(),
        "task clone failed: {}",
        String::from_utf8_lossy(&clone_output.stderr)
    );
    git(
        &task_repository,
        &[
            "remote",
            "set-url",
            "origin",
            "https://github.com/zbarno/kool.ade",
        ],
    );
    git(
        &task_repository,
        &[
            "checkout",
            "--quiet",
            "-b",
            "koolade/self-improvement-fixture",
            &source_sha,
        ],
    );
    let implementation = implementation_state(
        &ticket,
        &planning_item_uid,
        repository_id,
        &project_slug,
        &source_sha,
        &task_repository,
    );
    let task_state_dir = state_root.join("task-state");
    std::fs::create_dir_all(&task_state_dir).unwrap();
    let state_path = task_state_dir.join("state.json");
    std::fs::write(
        &state_path,
        super::super::super::super::serialize_state(&implementation).unwrap(),
    )
    .unwrap();
    let metadata = crate::artifacts::task_docs::TaskMetadata::new(
        &crate::domain::ArtifactIdentity {
            display_id: "TASK-001".into(),
            uid: planning_item_uid.clone(),
            parent_uid: Some(uuid::Uuid::new_v4().to_string()),
            title: "Production dispatch".into(),
        },
        "root",
        Vec::new(),
    )
    .unwrap();
    let verifier = super::authorize_resolved_task_source(
        &planning,
        &state_root,
        state_path.clone(),
        Some(&planning_item_uid),
        Some(&metadata),
        &implementation,
        None,
    )
    .unwrap()
    .expect("the production dispatch hook must install Evolve authorization");
    let mut mismatched_repository = implementation.clone();
    mismatched_repository.repository_id = Some("different-repository".into());
    assert!(
        super::authorize_resolved_task_source(
            &planning,
            &state_root,
            state_path.clone(),
            Some(&planning_item_uid),
            Some(&metadata),
            &mismatched_repository,
            None,
        )
        .is_err()
    );
    let generation = verifier.generation();
    verifier.verify(&generation).unwrap();
    assert_changed_persisted_source_is_rejected(
        &state_path,
        &implementation,
        verifier.as_ref(),
        &generation,
    );
    let source_status = git(&repo, &["status", "--porcelain"]);

    let workspace = crate::execution::contract::WorkspaceRef {
        project_id: workspace_uid.clone(),
        task_id: lifecycle_task_uid.clone(),
        source_sha: source_sha.clone(),
    };
    let clone_resolver =
        crate::execution::docker::backend::OwnedTaskWorkspaceResolver::for_current_user(
            workspace.clone(),
            &task_repository,
        )
        .expect("the fixture clone must be registered beneath current-user project storage");
    assert_eq!(
        crate::execution::docker::backend::TaskWorkspaceResolver::resolve_owned_clone(
            &clone_resolver,
            &workspace,
        )
        .unwrap(),
        task_repository.canonicalize().unwrap()
    );

    if exercise_authorized_docker_if_enabled(
        &task_repository,
        &workspace_uid,
        &lifecycle_task_uid,
        &source_sha,
        verifier.clone(),
        &runs,
        &ready,
    ) {
        assert_eq!(git(&repo, &["status", "--porcelain"]), source_status);
        return;
    }

    // The same verifier is checked for every Docker operation. Revoking the
    // persisted run after session creation must deny the next operation.
    let cancelled = runs
        .apply_event(
            &ready.run_uid,
            ready.revision,
            &uuid::Uuid::new_v4().to_string(),
            SelfImprovementEventV1::Cancel,
        )
        .unwrap();
    assert_eq!(cancelled.phase, SelfImprovementPhaseV1::Cancelled);
    assert!(verifier.verify(&generation).is_err());
}
