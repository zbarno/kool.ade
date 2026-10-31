use super::super::super::*;
use super::fake_pi;
use std::{
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
};

pub(super) struct Prepared {
    pub(super) root: PathBuf,
    pub(super) repo: PathBuf,
    pub(super) planning_repo: PathBuf,
    pub(super) remote: PathBuf,
    pub(super) docs: Vec<crate::artifacts::task_docs::TaskDocument>,
    pub(super) app: KooladeApp,
}

pub(super) fn git(cwd: &Path, args: &[&str]) -> String {
    let output = std::process::Command::new("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().to_string()
}

pub(super) fn prepare(root: PathBuf) -> Prepared {
    let repo = root.join("repo");
    let planning_repo = root.join("planning-repo");
    let remote = root.join("remote.git");
    std::fs::write(
        root.join("planning-repository-path"),
        planning_repo.to_string_lossy().as_bytes(),
    )
    .unwrap();
    std::fs::write(root.join("capacity-wait-barrier-enabled"), "").unwrap();
    std::fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-q", "-b", "main"]);
    git(&repo, &["config", "user.name", "Fixture"]);
    git(&repo, &["config", "user.email", "fixture@example.test"]);
    std::fs::write(repo.join("README.md"), "code checkout fixture\n").unwrap();
    git(&repo, &["add", "README.md"]);
    git(&repo, &["commit", "-qm", "code baseline"]);
    let code_remote_url = "ssh://git@github.com/koolade-fixture/fixture.git";
    std::fs::create_dir_all(&planning_repo).unwrap();
    git(&planning_repo, &["init", "-q", "-b", "main"]);
    git(&planning_repo, &["config", "user.name", "Fixture"]);
    git(
        &planning_repo,
        &["config", "user.email", "fixture@example.test"],
    );
    let workspace_id = uuid::Uuid::new_v4();
    std::fs::write(
        planning_repo.join("manifest.json"),
        serde_json::to_vec_pretty(&crate::core::managed_workspace::WorkspaceManifest::new(
            workspace_id,
        ))
        .unwrap(),
    )
    .unwrap();
    std::fs::create_dir_all(planning_repo.join("config")).unwrap();
    let repositories = crate::core::project_repos::ProjectManifest {
        repositories: vec![crate::core::project_repos::Repository {
            id: "root".into(),
            role: "Primary code".into(),
            remote: code_remote_url.into(),
            display_name: None,
        }],
    };
    std::fs::write(
        planning_repo.join("config/repositories.json"),
        serde_json::to_vec_pretty(&repositories).unwrap(),
    )
    .unwrap();
    std::fs::create_dir_all(planning_repo.join("planning/tasks/fixture")).unwrap();
    std::fs::create_dir_all(planning_repo.join("state")).unwrap();
    let batch_uid = uuid::Uuid::new_v4().hyphenated().to_string();
    let task_uids = (1..=4)
        .map(|_| uuid::Uuid::new_v4().hyphenated().to_string())
        .collect::<Vec<_>>();
    let docs = (1..=4)
        .map(|number| {
            let title = format!("Task {number}");
            let identity = crate::domain::ArtifactIdentity {
                uid: task_uids[number - 1].clone(),
                display_id: format!("TASK-{number:03}"),
                title: title.clone(),
                parent_uid: Some(batch_uid.clone()),
            };
            let dependency_uids = if number == 3 {
                task_uids[..2].to_vec()
            } else {
                Vec::new()
            };
            let metadata = crate::artifacts::task_docs::TaskMetadata::new(
                &identity,
                "root",
                dependency_uids,
            )
            .unwrap();
            let dependencies = if number == 3 {
                "- [Task 001](001-task.md) must be complete.\n- [Task 002](002-task.md) must be complete."
            } else {
                "None."
            };
            crate::artifacts::task_docs::TaskDocument {
                path: format!("planning/tasks/fixture/{number:03}-task.md"),
                title,
                text: format!(
                    "# Task {number}\n\nFeature: CHG-001\n\n## Dependencies\n{dependencies}\n\n## Affected files and components\n- src/task-{number}.rs\n\n## Acceptance criteria\n- File exists.\n"
                ),
                identity: Some(identity),
                metadata: Some(metadata),
                task_state: None,

                metadata_error: None,
            }
        })
        .collect::<Vec<_>>();
    for doc in &docs {
        std::fs::create_dir_all(planning_repo.join(&doc.path).parent().unwrap()).unwrap();
        let metadata = serde_json::to_string(doc.metadata.as_ref().unwrap()).unwrap();
        let identity = serde_json::to_string(doc.identity.as_ref().unwrap()).unwrap();
        let stored = format!(
            "---\nkoolade-task: {metadata}\n---\n\n{}\n<!-- koolade-artifact-id:v1 {identity} -->\n",
            doc.text
        );
        std::fs::write(planning_repo.join(&doc.path), stored).unwrap();
    }
    let feature = "# CHG-001: Fixture\n\n**Status:** Ready\n\n## Intent\nFixture.\n\n## Current Behavior\nFixture.\n\n## Desired Behavior\nFixture.\n\n## Scope\nFixture.\n\n## Affected Product Areas\nFixture.\n\n## Requirements\nFixture.\n\n## Decisions and Assumptions\nFixture.\n\n## Acceptance Criteria\nFixture.\n";
    std::fs::create_dir_all(planning_repo.join("planning/changes/CHG-001-fixture")).unwrap();
    std::fs::write(
        planning_repo.join("planning/changes/CHG-001-fixture/specification.md"),
        feature,
    )
    .unwrap();
    // Implementation loads the immutable approved specification beside the
    // generated task batch, as real task generation does.
    std::fs::write(
        planning_repo.join("planning/tasks/fixture/specification.md"),
        feature,
    )
    .unwrap();
    std::fs::write(planning_repo.join("state/workflow.json"), serde_json::json!({"brief":null,"reviewedSpecification":null,"taskBatches":[{"feature":"fixture","directory":"planning/tasks/fixture","count":4}]}).to_string()).unwrap();
    git(&planning_repo, &["add", "."]);
    git(&planning_repo, &["commit", "-qm", "planning baseline"]);
    git(&root, &["init", "--bare", "-q", remote.to_str().unwrap()]);
    git(&remote, &["config", "uploadpack.allowFilter", "true"]);
    let ssh = root.join("ssh-fixture");
    std::fs::write(
        &ssh,
        "#!/bin/sh\nroot=$(dirname \"$0\")\ncase \"$*\" in\n  *git-upload-pack*) exec git-upload-pack \"$root/remote.git\" ;;\n  *git-receive-pack*) exec git-receive-pack \"$root/remote.git\" ;;\n  *) echo 'unsupported fixture SSH command' >&2; exit 2 ;;\nesac\n",
    )
    .unwrap();
    std::fs::set_permissions(&ssh, std::fs::Permissions::from_mode(0o700)).unwrap();
    // SAFETY: the cross-process environment lock is held and the value is restored above.
    unsafe { std::env::set_var("GIT_SSH_COMMAND", &ssh) };
    git(&repo, &["remote", "add", "origin", code_remote_url]);
    git(&repo, &["push", "-q", "origin", "main"]);
    std::fs::write(
        planning_repo.join("planning/local-approval.md"),
        "approved locally\n",
    )
    .unwrap();
    git(&planning_repo, &["add", "planning/local-approval.md"]);
    git(&planning_repo, &["commit", "-qm", "approve local plan"]);
    let planning_store = crate::artifacts::planning_store::PlanningStore::new(
        workspace_id,
        &planning_repo,
        crate::artifacts::planning_store::StoreMode::ManagedLocal,
    );
    let fake_bin = root.join("fake-bin");
    std::fs::create_dir_all(&fake_bin).unwrap();
    let fake_gh = fake_bin.join("gh");
    std::fs::write(
            &fake_gh,
            "#!/bin/sh\ncommit=''\nwhile [ \"$#\" -gt 0 ]; do\n  if [ \"$1\" = --commit ]; then shift; commit=$1; fi\n  shift\ndone\nprintf '[{\"headSha\":\"%s\",\"status\":\"completed\",\"conclusion\":\"success\",\"workflowName\":\"fixture\",\"createdAt\":\"2099-01-01T00:00:00Z\",\"url\":\"https://github.com/koolade-fixture/fixture/actions/runs/1\"}]\\n' \"$commit\"\n",
        )
        .unwrap();
    std::fs::set_permissions(&fake_gh, std::fs::Permissions::from_mode(0o700)).unwrap();
    let test_path = std::env::join_paths(std::iter::once(fake_bin.clone()).chain(
        std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()),
    ))
    .unwrap();
    unsafe { std::env::set_var("PATH", test_path) };
    // Planning approval lives in its own repository and must stay out of code
    // publication.
    let pi = fake_pi::create(&root);
    unsafe { std::env::set_var("KOOLADE_PI_BIN", &pi) };
    let mut app = fixture();
    if let Screen::Connected(project) = &mut app.screen {
        project.state =
            crate::core::state::PlannerState::load_with_store(&repo, &planning_store).unwrap();
        // Plan confirmation binds to the current product specification, just
        // as a connected project does after its initial bootstrap.
        project.state.bootstrap_missing().unwrap();
        // Queue the capacity waiter only after the two intended workers are
        // active, so task 4 cannot race ahead while task 2 is retrying.
        project.task_documents = docs[..3].to_vec();
        project.queue.max_parallel = 2;
        project.queue.auto_publish = true;
        project.queue.blocked.insert(
                docs[1].path.clone(),
                crate::core::implementation::Failure::new(
                    crate::core::implementation::FailureKind::RemoteDiverged,
                    crate::core::implementation::RecoveryDisposition::AutomaticRetry,
                    "Local main and freshly fetched origin/main diverged before publication; verified work is preserved",
                ),
            );
        project.implementation_states.clear();
        project.chat_slug = format!("auto-e2e-{}", std::process::id());
    }
    // Exercise the board task action, including worker dispatch,
    // concurrent execution and integration into the remote.
    if let Screen::Connected(project) = &mut app.screen {
        project.state.active_feature = Some(("CHG-001".into(), feature.into()));
        assert!(!crate::core::workflow::feature_approved(
            &planning_store,
            &project.state.workflow,
            "CHG-001"
        ));
    }
    // Feature approval is a separate explicit action in Specifications;
    // implementation then starts from the selected task's board details.
    crate::app::root::feature_approval::tests::record_test_plan_author(&mut app, "CHG-001");
    app.approve_feature_only("CHG-001");
    crate::app::root::feature_approval::tests::assert_current_test_plan_confirmation(
        &mut app, "CHG-001",
    );
    crate::app::root::feature_approval::tests::record_test_plan_review(
        &mut app,
        "CHG-001",
        crate::core::lifecycle::GateDecision::Approved,
    );
    let ctx = egui::Context::default();
    frame(&mut app, &ctx, vec![]);
    click_text(&mut app, &ctx, "Task 1");
    click_text(&mut app, &ctx, "Implement & continue queue");
    if let Screen::Connected(project) = &app.screen {
        assert!(project.active_turn.is_none());
        assert!(project.active_implementations.contains_key(&docs[0].path));
        assert!(crate::core::workflow::feature_approved(
            &planning_store,
            &project.state.workflow,
            "CHG-001"
        ));
        let saved =
            crate::core::state::PlannerState::load_with_store(&repo, &planning_store).unwrap();
        assert!(crate::core::workflow::feature_approved(
            &planning_store,
            &saved.workflow,
            "CHG-001"
        ));
    }
    Prepared {
        root,
        repo,
        planning_repo,
        remote,
        docs,
        app,
    }
}
