use super::support::{StateHome, click, frame, git, visible_text};
use crate::{
    app::{
        root::{KooladeApp, Screen},
        welcome::attempt_connect_with_store,
    },
    artifacts::planning_store::PlanningStore,
    core::{managed_workspace::WorkspaceManager, planning_work::Work},
};
use std::path::Path;

fn code_checkout(root: &Path, branch: &str, remote: &str) {
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
    std::fs::write(root.join("README.md"), "same shared product\n").unwrap();
    git(Some(root), &["add", "README.md"]);
    git(Some(root), &["commit", "-qm", "main fixture"]);
    if branch != "main" {
        git(Some(root), &["checkout", "-qb", branch]);
        std::fs::write(root.join("branch.txt"), format!("{branch}\n")).unwrap();
        git(Some(root), &["add", "branch.txt"]);
        git(Some(root), &["commit", "-qm", "branch fixture"]);
    }
}

fn connected_app(code: &Path, store: PlanningStore) -> KooladeApp {
    let project = attempt_connect_with_store(&code.to_string_lossy(), store).unwrap();
    KooladeApp {
        screen: Screen::Connected(Box::new(project)),
        ..Default::default()
    }
}

fn planning_signature(
    app: &KooladeApp,
) -> Vec<(
    String,
    String,
    String,
    crate::core::planning_work::WorkStatus,
)> {
    let Screen::Connected(project) = &app.screen else {
        panic!("workspace should be connected")
    };
    project
        .planning_work
        .iter()
        .map(|work| {
            (
                work.uid.clone(),
                work.title.clone(),
                work.request.clone(),
                work.status,
            )
        })
        .collect()
}

fn approval_fingerprints(app: &KooladeApp) -> std::collections::BTreeMap<String, String> {
    let Screen::Connected(project) = &app.screen else {
        panic!("workspace should be connected")
    };
    crate::artifacts::task_docs::load_workflow(&project.state.planning_store)
        .unwrap()
        .approved_features
}

fn picker_entries(app: &mut KooladeApp, ctx: &egui::Context) -> Vec<String> {
    click(app, ctx, "Workspace");
    let entries = visible_text(&click(app, ctx, "Registered repositories"))
        .into_iter()
        .filter(|text| {
            (text.starts_with("Open ") && text.ends_with("in a new window"))
                || text == "No registered repositories"
        })
        .collect();
    click(app, ctx, "Workspace");
    entries
}

#[test]
fn two_operator_checkouts_keep_the_same_workspace_picker_and_board_after_branch_switch() {
    let _shield = crate::core::gitops::test_support::shield("workspace-branch-switch-ui");
    let base = std::env::temp_dir().join(format!(
        "koolade_branch_switch_ui_{}_{}",
        std::process::id(),
        uuid::Uuid::new_v4()
    ));
    let home_a = base.join("operator-a");
    let home_b = base.join("operator-b");
    std::fs::create_dir_all(&home_a).unwrap();
    std::fs::create_dir_all(&home_b).unwrap();
    let mut state_home = StateHome::new();
    state_home.set(&home_a);
    let manager_a = WorkspaceManager::new(&home_a);
    let workspace_a = manager_a.create("Shared branch workspace").unwrap();
    let mut work = Work::new(
        String::new(),
        "Shared planning card".into(),
        "Shared board content survives code branch changes".into(),
        "Visible planning evidence".into(),
    );
    work.source_branch = None;
    work.destination_branch = None;
    crate::core::planning_work::save(&workspace_a.store, std::slice::from_ref(&work)).unwrap();
    let feature_id = crate::artifacts::product_docs::next_feature_id(&workspace_a.store);
    let feature = format!(
        "# {feature_id}: Branch-stable feature\n\n**Status:** Ready\n\n## Intent\n\nKeep one planning authority.\n\n## Current Behavior\n\nBoard content is shared.\n\n## Desired Behavior\n\nChanging code branches leaves approved planning unchanged.\n\n## Scope\n\nWorkspace planning.\n\n## Affected Product Areas\n\nOverview.\n\n## Requirements\n\n- BRANCH-R1 (MUST). Keep the same approved contract.\n\n## Decisions and Assumptions\n\n- **A1 (fixture):** Branches belong to code repositories.\n\n## Acceptance Criteria\n\n1. Board and approval fingerprint remain stable across code branch changes.\n"
    );
    let feature_path = crate::artifacts::product_docs::document_path_for_update(
        &workspace_a.store,
        &format!("feature:{feature_id}"),
        &feature,
    )
    .unwrap();
    let feature = crate::artifacts::product_docs::identity::preserve_feature_identity(
        &feature_path,
        &feature_id,
        &feature,
    )
    .unwrap();
    let feature_identity = crate::domain::ArtifactIdentity::from_markdown(&feature)
        .unwrap()
        .unwrap();
    let feature = crate::domain::ChangeMetadata::write_markdown(
        &feature,
        &feature_identity,
        crate::domain::ChangeStatus::Ready,
    )
    .unwrap();
    std::fs::create_dir_all(feature_path.parent().unwrap()).unwrap();
    std::fs::write(&feature_path, &feature).unwrap();
    let mut workflow = crate::artifacts::task_docs::load_workflow(&workspace_a.store).unwrap();
    workflow.approved_features.insert(
        feature_id.clone(),
        crate::core::workflow::feature_contract(&feature),
    );
    crate::artifacts::task_docs::save_workflow(&workspace_a.store, &workflow).unwrap();
    let record = workspace_a.store.layout().work_record(&work.uid).unwrap();
    let relative = record
        .strip_prefix(&workspace_a.planning_repo)
        .unwrap()
        .to_string_lossy()
        .into_owned();
    crate::core::gitops::commit(
        &workspace_a.planning_repo,
        "Kool.ad/e: save shared board fixture",
        &[relative],
    )
    .unwrap();
    let manager_b = WorkspaceManager::new(&home_b);
    let workspace_b = manager_b.join(&workspace_a.planning_repo).unwrap();

    let code_remote = "https://example.test/team/api-service.git";
    let code_a = base.join("code-a");
    let code_b = base.join("code-b");
    code_checkout(&code_a, "main", code_remote);
    git(Some(&code_a), &["checkout", "-qb", "feature/auth"]);
    std::fs::write(code_a.join("feature.txt"), "feature/auth\n").unwrap();
    git(Some(&code_a), &["add", "feature.txt"]);
    git(Some(&code_a), &["commit", "-qm", "feature fixture"]);
    git(Some(&code_a), &["checkout", "-q", "main"]);
    code_checkout(&code_b, "release/1.2", code_remote);

    let registration =
        |checkout: &Path| crate::core::managed_workspace::CodeRepositoryRegistration {
            id: "api-service".into(),
            role: "API service".into(),
            remote: Some(code_remote.into()),
            checkout: checkout.to_path_buf(),
        };
    manager_a
        .register_code_repository(workspace_a.id, registration(&code_a))
        .unwrap();
    state_home.set(&home_b);
    manager_b
        .register_code_repository(workspace_b.id, registration(&code_b))
        .unwrap();

    state_home.set(&home_a);
    let mut app_a = connected_app(&code_a, manager_a.open(workspace_a.id).unwrap().store);
    state_home.set(&home_b);
    let mut app_b = connected_app(&code_b, manager_b.open(workspace_b.id).unwrap().store);
    assert!(crate::ui::Surface::is_git_repo(&app_a));
    assert!(crate::ui::Surface::is_git_repo(&app_b));
    assert_eq!(crate::ui::Surface::git_branch(&app_a), "main");
    assert_eq!(crate::ui::Surface::git_branch(&app_b), "release/1.2");
    assert_eq!(
        crate::ui::Surface::registered_repositories(&app_a),
        crate::ui::Surface::registered_repositories(&app_b)
    );
    assert_eq!(planning_signature(&app_a), planning_signature(&app_b));
    assert_eq!(approval_fingerprints(&app_a), approval_fingerprints(&app_b));

    let ctx_a = egui::Context::default();
    let ctx_b = egui::Context::default();
    let board_a = frame(&mut app_a, &ctx_a, Vec::new());
    let board_b = frame(&mut app_b, &ctx_b, Vec::new());
    assert!(
        visible_text(&board_a)
            .iter()
            .any(|text| text.contains("Shared planning card"))
    );
    assert!(
        visible_text(&board_b)
            .iter()
            .any(|text| text.contains("Shared planning card"))
    );
    assert_eq!(
        picker_entries(&mut app_a, &ctx_a),
        vec!["Open API service (api-service) in a new window"]
    );
    assert_eq!(
        picker_entries(&mut app_b, &ctx_b),
        vec!["Open API service (api-service) in a new window"]
    );

    git(Some(&code_a), &["checkout", "-q", "feature/auth"]);
    state_home.set(&home_a);
    let manager_a_store = manager_a.open(workspace_a.id).unwrap().store;
    app_a = connected_app(&code_a, manager_a_store);
    assert_eq!(crate::ui::Surface::git_branch(&app_a), "feature/auth");
    assert_eq!(
        crate::ui::Surface::registered_repositories(&app_a),
        crate::ui::Surface::registered_repositories(&app_b)
    );
    assert_eq!(planning_signature(&app_a), planning_signature(&app_b));
    assert_eq!(approval_fingerprints(&app_a), approval_fingerprints(&app_b));
    let switched = frame(&mut app_a, &ctx_a, Vec::new());
    assert!(
        visible_text(&switched)
            .iter()
            .any(|text| text.contains("Shared planning card"))
    );
    assert_eq!(
        picker_entries(&mut app_a, &ctx_a),
        picker_entries(&mut app_b, &ctx_b)
    );
    drop(app_a);
    drop(app_b);
    let _ = std::fs::remove_dir_all(base);
}
