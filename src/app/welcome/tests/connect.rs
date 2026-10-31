use super::super::*;
use crate::core::managed_workspace::{
    CodeRepositoryRegistration, NewProjectSpec, WorkspaceManager,
};
use crate::error::AppError;
use std::{path::Path, process::Command};

fn git(root: &Path, args: &[&str]) {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn register_primary_code(
    manager: &WorkspaceManager,
    workspace_id: uuid::Uuid,
    checkout: &Path,
    remote: &str,
) {
    git(checkout, &["remote", "add", "origin", remote]);
    manager
        .register_code_repository(
            workspace_id,
            CodeRepositoryRegistration {
                id: crate::core::managed_workspace::PAIR_CODE_REPO_ID.into(),
                role: "Primary code".into(),
                remote: Some(remote.into()),
                checkout: checkout.to_path_buf(),
            },
        )
        .unwrap();
}

#[test]
fn connecting_missing_path_errors_friendly() {
    match attempt_connect("") {
        Ok(_) => panic!("expected connection error"),
        Err(err) => assert!(matches!(err, AppError::InvalidRepo { .. })),
    }
}

#[test]
fn connecting_nonexistent_path_errors_friendly() {
    match attempt_connect("/no/such/dir-xyz-123") {
        Ok(_) => panic!("expected connection error"),
        Err(err) => assert!(matches!(err, AppError::InvalidRepo { .. })),
    }
}

#[test]
fn unregistered_code_checkout_is_not_attached_or_bootstrapped() {
    let _env = super::helpers::EnvSandbox::enter("unregistered-code-checkout", false);
    let code = super::helpers::local_source_repo("unregistered-code-checkout");

    let error = match attempt_connect(code.to_str().unwrap()) {
        Ok(_) => panic!("unregistered code checkout must not be attached"),
        Err(error) => error,
    };

    assert!(matches!(error, AppError::InvalidRepo { .. }));
    assert!(error.to_string().contains("no managed planning repository"));
    assert!(!code.join(".koolade-packet").exists());
    assert!(!code.join("planning").exists());
    let _ = std::fs::remove_dir_all(code);
}

#[test]
fn connecting_with_open_board_items_queues_an_initial_manager_review() {
    let _env = super::helpers::EnvSandbox::enter("initial-manager-review", false);
    let code = super::helpers::local_source_repo("initial-manager-review");
    let manager = WorkspaceManager::from_state_root();
    let workspace = manager.create("Initial review project").unwrap();
    register_primary_code(
        &manager,
        workspace.id,
        &code,
        "https://github.com/example/initial-review.git",
    );
    let item = crate::domain::OpenItem::new(
        "CLR-001".into(),
        crate::domain::Priority::Normal,
        crate::domain::ItemKind::Question,
        "Product".into(),
        None,
        "Choose a behavior".into(),
        "The implementation needs this decision".into(),
    );
    workspace
        .store
        .atomic_write(
            crate::artifacts::planning_store::paths::OPEN_ITEMS,
            crate::artifacts::items_io::serialize(&[item]).as_bytes(),
        )
        .unwrap();

    let project = attempt_connect(code.to_str().unwrap()).unwrap();

    assert_eq!(project.state.repo_root, code);
    assert_eq!(project.state.planning_store.root, workspace.planning_repo);
    assert!(
        project
            .activity
            .pending
            .iter()
            .any(|event| { event.contains("review existing board work") })
    );
    assert!(!project.state.repo_root.join("planning").exists());
    let _ = std::fs::remove_dir_all(project.state.repo_root);
}

#[test]
fn connecting_with_injected_store_keeps_planning_in_a_separate_root() {
    let _env = super::helpers::EnvSandbox::enter("injected-planning-store", false);
    let code = super::helpers::local_source_repo("injected-planning-store-code");
    let planning = std::env::temp_dir().join(format!(
        "koolade_injected_planning_store_{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&planning).unwrap();
    git(&planning, &["init", "-q", "-b", "main"]);
    git(&planning, &["config", "user.name", "Koolade Test"]);
    git(
        &planning,
        &["config", "user.email", "koolade@example.invalid"],
    );
    let store = crate::artifacts::planning_store::PlanningStore::new(
        uuid::Uuid::new_v4(),
        &planning,
        crate::artifacts::planning_store::StoreMode::ManagedLocal,
    );
    store
        .atomic_write(
            crate::artifacts::planning_store::paths::PROJECT_MANIFEST,
            br#"{"repositories":[]}"#,
        )
        .unwrap();
    git(&planning, &["add", "config/repositories.json"]);
    git(&planning, &["commit", "-qm", "fixture planning repository"]);

    let project = attempt_connect_with_store(code.to_str().unwrap(), store.clone()).unwrap();

    assert_eq!(project.state.repo_root, code);
    assert_eq!(project.state.planning_store, store);
    assert!(
        project
            .state
            .planning_store
            .read(crate::artifacts::planning_store::paths::PRODUCT_INDEX)
            .is_ok()
    );
    assert!(!project.state.repo_root.join("planning/product").exists());
    assert!(!project.state.repo_root.join(".koolade-packet").exists());

    let _ = std::fs::remove_dir_all(project.state.repo_root);
    let _ = std::fs::remove_dir_all(planning);
}

#[test]
fn new_project_uses_a_derived_planning_pair_and_reopens_by_workspace_id() {
    let _env = super::helpers::EnvSandbox::enter("managed-workspace-connect", false);
    let parent =
        std::env::temp_dir().join(format!("koolade-project-parent-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&parent).unwrap();
    let mut spec = NewProjectSpec::new("Managed planning project", "managed-planning-project");
    spec.parent_dir = parent.clone();

    let created = attempt_create_project(&spec).unwrap();
    let code = created.state.repo_root.clone();
    let planning = created.state.planning_store.root.clone();
    let workspace_id = created.state.planning_store.project_id;
    assert_eq!(
        planning,
        WorkspaceManager::from_state_root()
            .project_dir(workspace_id)
            .join("planning-repo")
    );
    assert_ne!(code, planning);
    assert!(!parent.join("managed-planning-project.packet").exists());
    assert_eq!(
        created.state.planning_store.mode,
        crate::artifacts::planning_store::StoreMode::ManagedLocal
    );
    assert!(!code.join(".koolade-packet").exists());
    assert!(!planning.join(".koolade-packet").exists());

    let by_path = attempt_connect(code.to_str().unwrap()).unwrap();
    let by_id = attempt_open_workspace(workspace_id).unwrap();
    for reopened in [&by_path, &by_id] {
        assert_eq!(reopened.state.planning_store.project_id, workspace_id);
        assert_eq!(reopened.state.repo_root, code);
        assert_eq!(reopened.state.planning_store.root, planning);
    }
    assert!(
        std::fs::read_to_string(planning.join("planning/product/index.md"))
            .unwrap()
            .contains("Managed planning project")
    );
    let _ = std::fs::remove_dir_all(parent);
}

#[test]
fn application_created_workspace_reopens_its_managed_code_checkout() {
    // Production boundary: application creation -> durable code-repository
    // registration -> workspace-ID reopen. This catches repository-ID
    // mismatches that a unit test of either registration or path lookup alone
    // would miss. Uses only temporary Git repositories and local state.
    let _env = super::helpers::EnvSandbox::enter("application-workspace-reopen", false);
    let parent = std::env::temp_dir().join(format!(
        "koolade-application-parent-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&parent).unwrap();
    let request = crate::core::managed_workspace::ApplicationCreation {
        transaction_id: uuid::Uuid::new_v4(),
        workspace_id: uuid::Uuid::new_v4(),
        title: "Created application".into(),
        intent: "A local fixture application".into(),
        code_directory: parent.join("application"),
    };
    let manager = WorkspaceManager::from_state_root();
    let created = manager.create_application(&request).unwrap();
    assert!(created.repositories.repositories[0].id.starts_with("app-"));

    let reopened = attempt_open_workspace(request.workspace_id).unwrap();
    assert_eq!(reopened.state.repo_root, request.code_directory);
    assert_eq!(
        reopened.state.planning_store.project_id,
        request.workspace_id
    );
    let _ = std::fs::remove_dir_all(parent);
}

#[test]
fn connecting_from_a_managed_code_subfolder_uses_its_root() {
    let _env = super::helpers::EnvSandbox::enter("managed-workspace-subfolder", false);
    let parent =
        std::env::temp_dir().join(format!("koolade-subfolder-parent-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&parent).unwrap();
    let mut spec = NewProjectSpec::new("Managed planning project", "managed-planning-project");
    spec.parent_dir = parent.clone();
    let created = attempt_create_project(&spec).unwrap();
    let root = created.state.repo_root.clone();
    let selected = root.join("src");
    std::fs::create_dir_all(&selected).unwrap();

    let reopened = attempt_connect(selected.to_str().unwrap()).unwrap();

    assert_eq!(reopened.state.repo_root, root);
    assert_eq!(
        reopened.state.planning_store.root,
        created.state.planning_store.root
    );
    assert!(!selected.join(".koolade-packet").exists());
    assert!(!root.join(".koolade-packet").exists());
    let _ = std::fs::remove_dir_all(parent);
}
