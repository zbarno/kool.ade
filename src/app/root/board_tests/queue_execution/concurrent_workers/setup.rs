use super::super::super::*;
use super::fake_pi;
use std::{
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
};

pub(super) struct Prepared {
    pub(super) root: PathBuf,
    pub(super) repo_root: PathBuf,
    pub(super) local_state_root: PathBuf,
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
    let local_state_root = root.join("private-state");
    let remote = root.join("remote.git");
    std::fs::write(root.join("capacity-wait-barrier-enabled"), "").unwrap();
    std::fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-q", "-b", "main"]);
    git(&repo, &["config", "user.name", "Fixture"]);
    git(&repo, &["config", "user.email", "fixture@example.test"]);
    let code_remote_url = "ssh://git@github.com/koolade-fixture/fixture.git";
    std::fs::create_dir_all(&local_state_root).unwrap();
    let workspace_id = uuid::Uuid::new_v4();
    std::fs::create_dir_all(local_state_root.join("config")).unwrap();
    let repositories = crate::core::project_repos::ProjectManifest {
        repositories: vec![crate::core::project_repos::Repository {
            id: "root".into(),
            role: "Primary code".into(),
            remote: code_remote_url.into(),
            display_name: None,
        }],
    };
    std::fs::write(
        local_state_root.join("config/repositories.json"),
        serde_json::to_vec_pretty(&repositories).unwrap(),
    )
    .unwrap();
    std::fs::create_dir_all(local_state_root.join("planning/tasks/fixture")).unwrap();
    std::fs::create_dir_all(local_state_root.join("state")).unwrap();
    let batch_uid = uuid::Uuid::new_v4().hyphenated().to_string();
    let task_uids = (1..=4)
        .map(|_| uuid::Uuid::new_v4().hyphenated().to_string())
        .collect::<Vec<_>>();
    let mut docs = (1..=4)
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
        let path = local_state_root.join(&doc.path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let embedded =
            crate::artifacts::task_docs::embed(&doc.text, doc.metadata.as_ref().unwrap()).unwrap();
        let identity = serde_json::to_string(doc.identity.as_ref().unwrap()).unwrap();
        std::fs::write(
            path,
            format!("{embedded}\n<!-- koolade-artifact-id:v1 {identity} -->\n"),
        )
        .unwrap();
    }
    let feature_heading = "# CHG-001: Fixture\n\n**Status:** Ready\n\n## Intent\nFixture.\n\n## Current Behavior\nFixture.\n\n## Desired Behavior\nFixture.\n\n## Scope\nFixture.\n\n## Affected Product Areas\nFixture.\n\n## Requirements\nFixture.\n\n## Decisions and Assumptions\nFixture.\n\n## Acceptance Criteria\nFixture.\n";
    let identified = crate::domain::ArtifactIdentity::preserve_markdown(
        feature_heading,
        None,
        "CHG-001",
        "Fixture",
    )
    .unwrap();
    let feature_identity = crate::domain::ArtifactIdentity::from_markdown(&identified)
        .unwrap()
        .unwrap();
    let feature = crate::domain::ChangeMetadata::write_markdown(
        &identified,
        &feature_identity,
        crate::domain::ChangeStatus::Ready,
    )
    .unwrap();
    let feature_path = repo.join(format!("docs/koolade/features/{}.md", feature_identity.uid));
    std::fs::create_dir_all(feature_path.parent().unwrap()).unwrap();
    std::fs::write(&feature_path, &feature).unwrap();
    std::fs::create_dir_all(repo.join("docs/koolade/features")).unwrap();
    std::fs::write(
        repo.join("docs/koolade/features/AGENTS.md"),
        format!(
            "# Navigation\n\n- [Fixture feature]({}.md)\n",
            feature_identity.uid
        ),
    )
    .unwrap();
    std::fs::create_dir_all(repo.join("docs/koolade/product")).unwrap();
    std::fs::write(
        repo.join("docs/koolade/product/overview.md"),
        "# Fixture product\n\nSynthetic product context.\n",
    )
    .unwrap();
    std::fs::write(repo.join("README.md"), "code checkout fixture\n").unwrap();
    git(&repo, &["add", "README.md", "docs/koolade"]);
    git(
        &repo,
        &["commit", "-qm", "code and published specification baseline"],
    );
    let source_sha = git(&repo, &["rev-parse", "refs/heads/main"]);
    let task_git_intent = crate::core::git_intent::GitIntentV1::approve(
        "root".into(),
        "main".into(),
        source_sha,
        "main".into(),
        crate::core::git_intent::PublicationModeV1::AutoPublish,
        None,
    )
    .unwrap();
    for doc in &mut docs {
        let metadata = doc
            .metadata
            .as_ref()
            .unwrap()
            .clone()
            .with_git_intent(Some(&task_git_intent))
            .unwrap();
        doc.metadata = Some(metadata.clone());
        let path = local_state_root.join(&doc.path);
        let embedded = crate::artifacts::task_docs::embed(&doc.text, &metadata).unwrap();
        let identity = serde_json::to_string(doc.identity.as_ref().unwrap()).unwrap();
        std::fs::write(
            path,
            format!("{embedded}\n<!-- koolade-artifact-id:v1 {identity} -->\n"),
        )
        .unwrap();
    }
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
    crate::test_support::set_test_origin_default(&repo);
    std::fs::write(
        local_state_root.join("planning/local-approval.md"),
        "approved locally\n",
    )
    .unwrap();
    let planning_store = crate::artifacts::planning_store::PlanningStore::for_code_repository(
        workspace_id,
        &local_state_root,
        &repo,
    );
    let workflow = crate::core::workflow::Workflow {
        task_batches: vec![crate::core::workflow::TaskBatchRef {
            identity: Some(crate::domain::ArtifactIdentity {
                uid: batch_uid,
                display_id: "BATCH-001".into(),
                title: "Fixture tasks".into(),
                parent_uid: None,
            }),
            feature: "fixture".into(),
            directory: "planning/tasks/fixture".into(),
            count: docs.len(),
            created_at_ms: 1,
        }],
        feature_git_intents: std::collections::BTreeMap::from([(
            "CHG-001".into(),
            std::collections::BTreeMap::from([("root".into(), task_git_intent)]),
        )]),
        ..Default::default()
    };
    crate::artifacts::task_docs::save_workflow(&planning_store, &workflow).unwrap();
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
    // The private local state marker must stay out of code publication.
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
        project.queue.require_independent_checks = true;
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
        project.state.active_feature = Some(("CHG-001".into(), feature));
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
        repo_root: repo,
        local_state_root,
        remote,
        docs,
        app,
    }
}
