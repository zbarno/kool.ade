use super::support::*;
use crate::app::root::{KooladeApp, Screen};
use std::path::Path;

fn init_code_repository(root: &Path, remote: &str) {
    std::fs::create_dir_all(root).unwrap();
    git(
        None,
        &[
            "-c",
            "init.templateDir=",
            "init",
            "--quiet",
            "--initial-branch=main",
            "--",
            root.to_str().unwrap(),
        ],
    );
    git(Some(root), &["config", "user.name", "Koolade Test"]);
    git(
        Some(root),
        &["config", "user.email", "koolade@example.invalid"],
    );
    git(Some(root), &["remote", "add", "origin", remote]);
    std::fs::write(root.join("README.md"), "API checkout fixture\n").unwrap();
    git(Some(root), &["add", "README.md"]);
    git(Some(root), &["commit", "-qm", "API checkout fixture"]);
}

#[test]
fn remote_join_shows_code_repo_tasks_and_checkout_setup_without_cloning_code() {
    let _shield = crate::core::gitops::test_support::shield_with_synthetic_identity(
        "workspace-code-repo-join-ui",
    );
    let base = std::env::temp_dir().join(format!(
        "koolade_remote_code_repo_ui_{}_{}",
        std::process::id(),
        uuid::Uuid::new_v4()
    ));
    let home_a = base.join("operator-a");
    let home_b = base.join("operator-b");
    std::fs::create_dir_all(&home_a).unwrap();
    std::fs::create_dir_all(&home_b).unwrap();
    let mut state_home = StateHome::new();
    state_home.set(&home_a);

    let manager_a = crate::core::managed_workspace::WorkspaceManager::from_state_root();
    let workspace = manager_a.create("Shared API workspace").unwrap();
    let code_remote = "https://example.test/team/api-service.git";
    let code_checkout = base.join("api-service");
    init_code_repository(&code_checkout, code_remote);
    manager_a
        .register_code_repository(
            workspace.id,
            crate::core::managed_workspace::CodeRepositoryRegistration {
                id: "api-service".into(),
                role: "API service".into(),
                remote: Some(code_remote.into()),
                checkout: code_checkout,
            },
        )
        .unwrap();

    let brief = crate::core::workflow::InterviewBrief {
        feature_name: "Remote API task fixture".into(),
        problem: "A joined operator needs to inspect shared tasks.".into(),
        goal: "Show a code task and explain checkout setup.".into(),
        target_users: "Workspace collaborators".into(),
        intended_outcome: "A task stays visible without cloning its code repository.".into(),
        success_criteria: vec!["The task remains visible after remote join.".into()],
        in_scope: vec!["A task targets the registered API repository.".into()],
        out_of_scope: Vec::new(),
        constraints: Vec::new(),
        ready_for_tasks: true,
    };
    let batch = crate::core::workflow::TaskBatch {
        brief,
        specification: "# Remote API task fixture\n".into(),
        feature_id: None,
        contract: None,
        branch_targets: None,
        task_routing: Default::default(),
        stories: vec![crate::core::workflow::TaskStory {
            title: "Add API endpoint".into(),
            target_repository: "api-service".into(),
            intent: "Implement the requested API endpoint.".into(),
            goal: "Add the endpoint in the shared API repository.".into(),
            context: "The shared workspace stores task stories separately from code checkouts."
                .into(),
            user_story: "As a collaborator, I can see API work from the shared board.".into(),
            purpose: "Keep planning visible while each operator configures their own checkout."
                .into(),
            scope_items: vec![1],
            success_criteria: vec![1],
            affected_files: vec!["src/api.rs".into()],
            implementation_steps: vec!["Implement the endpoint in the API repository.".into()],
            acceptance_criteria: vec!["The endpoint behavior is covered by a test.".into()],
            test_plan: vec!["Run the API repository tests.".into()],
            verification_commands: vec!["cargo test".into()],
            definition_of_done: vec!["The endpoint and tests are committed.".into()],
            ..Default::default()
        }],
    };
    let mut workflow = crate::artifacts::task_docs::load_workflow(&workspace.store).unwrap();
    let task_paths =
        crate::artifacts::task_docs::write_batch(&workspace.store, &batch, &mut workflow).unwrap();
    let ticket = task_paths
        .iter()
        .find(|path| path.ends_with("001-add-api-endpoint.md"))
        .unwrap()
        .clone();
    let mut commit_paths = vec![crate::core::project_repos::PROJECT_FILE.into()];
    commit_paths.extend(task_paths);
    crate::core::gitops::commit(
        &workspace.planning_repo,
        "Kool.ad/e: publish API task fixture",
        &commit_paths,
    )
    .unwrap();

    let planning_remote = base.join("remotes/planning.git");
    clone_bare_remote(&workspace.planning_repo, &planning_remote);
    let workspace = manager_a
        .set_planning_remote(workspace.id, &planning_remote.to_string_lossy())
        .unwrap();

    state_home.set(&home_b);
    let mut app = KooladeApp::default();
    let ctx = egui::Context::default();
    let join_url = url::Url::from_file_path(&planning_remote)
        .unwrap()
        .to_string();
    type_into(
        &mut app,
        &ctx,
        "Planning Git remote URL or local checkout path",
        &join_url,
    );
    click(&mut app, &ctx, "Join workspace");

    let Screen::Connected(joined) = &app.screen else {
        panic!("the joiner should see the shared planning workspace")
    };
    assert_eq!(joined.state.planning_store.project_id, workspace.id);
    assert_eq!(
        joined.state.planning_store.project_id,
        workspace.manifest.workspace_id
    );
    assert!(
        joined
            .state
            .repositories
            .repositories
            .iter()
            .any(|repository| {
                repository.id == "api-service" && repository.remote == code_remote
            })
    );
    assert!(
        joined
            .task_documents
            .iter()
            .any(|document| document.path == ticket)
    );
    let owner_workflow = crate::artifacts::task_docs::load_workflow(&workspace.store).unwrap();
    let owner_documents =
        crate::artifacts::task_docs::load_board(&workspace.store, &owner_workflow);
    let owner_task_ids: Vec<_> = owner_documents
        .iter()
        .filter_map(|document| {
            document
                .identity
                .as_ref()
                .map(|identity| identity.uid.clone())
        })
        .collect();
    let joiner_task_ids: Vec<_> = joined
        .task_documents
        .iter()
        .filter_map(|document| {
            document
                .identity
                .as_ref()
                .map(|identity| identity.uid.clone())
        })
        .collect();
    assert_eq!(joiner_task_ids, owner_task_ids);
    let local = crate::core::managed_workspace::WorkspaceManager::from_state_root()
        .open(workspace.id)
        .unwrap();
    assert!(local.association.code_checkouts.is_empty());
    assert!(!home_b.join("api-service").exists());
    assert!(!local.planning_repo.join(".koolade-packet").exists());
    let board = frame(&mut app, &ctx, Vec::new());
    assert!(
        visible_text(&board)
            .iter()
            .any(|text| text.contains("Add API endpoint")),
        "the joiner should see the task without the code checkout"
    );

    click(&mut app, &ctx, "Add API endpoint");
    let task_details = frame(&mut app, &ctx, Vec::new());
    assert!(
        visible_text(&task_details)
            .iter()
            .any(|text| text == "Implement" || text == "Implement & continue queue"),
        "the shared task exposes its implementation control"
    );
    app.start_implementation_with_capabilities(
        ticket.clone(),
        true,
        crate::harness::runtime_capabilities::RuntimeCapabilities {
            planning_access:
                crate::harness::runtime_capabilities::PlanningAccess::SandboxedRepositoryReads,
            implementation: true,
        },
    );
    let Screen::Connected(joined) = &app.screen else {
        unreachable!()
    };
    assert!(joined.active_implementations.is_empty());
    let setup_message = &joined.queue.last_error;
    assert!(setup_message.contains("api-service"), "{setup_message}");
    assert!(
        setup_message.contains("Workspace settings > Code repositories"),
        "{setup_message}"
    );
    let blocked = frame(&mut app, &ctx, Vec::new());
    assert!(
        visible_text(&blocked)
            .iter()
            .any(|text| text.contains("map its checkout before implementation")),
        "task details should explain how to configure the missing checkout"
    );
    drop(app);
    let _ = std::fs::remove_dir_all(base);
}
