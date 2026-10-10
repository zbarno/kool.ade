use super::super::*;
use crate::error::AppError;

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
fn connecting_with_open_board_items_queues_an_initial_manager_review() {
    let _env = super::helpers::EnvSandbox::enter("initial-manager-review", false);
    let repo = super::helpers::local_source_repo("initial-manager-review");
    let item = crate::domain::OpenItem::new(
        "CLR-001".into(),
        crate::domain::Priority::Normal,
        crate::domain::ItemKind::Question,
        "Product".into(),
        None,
        "Choose a behavior".into(),
        "The implementation needs this decision".into(),
    );
    let items_path = crate::artifacts::repo_artifact(&repo, crate::artifacts::OPEN_ITEMS_FILE);
    std::fs::create_dir_all(items_path.parent().unwrap()).unwrap();
    std::fs::write(&items_path, crate::artifacts::items_io::serialize(&[item])).unwrap();

    let project = attempt_connect(repo.to_str().unwrap()).unwrap();
    assert!(
        project
            .activity
            .pending
            .iter()
            .any(|event| { event.contains("review existing board work") })
    );
    std::fs::remove_dir_all(repo).unwrap();
}

#[test]
fn connecting_with_injected_store_keeps_planning_out_of_the_code_checkout() {
    let _env = super::helpers::EnvSandbox::enter("injected-planning-store", false);
    let code = super::helpers::local_source_repo("injected-planning-store-code");
    let planning = std::env::temp_dir().join(format!(
        "koolade_injected_planning_store_{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&planning);
    std::fs::create_dir_all(&planning).unwrap();
    for args in [
        ["init", "-q", "-b", "main"].as_slice(),
        ["config", "user.name", "Koolade Test"].as_slice(),
        ["config", "user.email", "koolade@example.invalid"].as_slice(),
    ] {
        assert!(
            std::process::Command::new("git")
                .arg("-C")
                .arg(&planning)
                .args(args)
                .status()
                .unwrap()
                .success()
        );
    }
    let store = crate::artifacts::planning_store::PlanningStore::new(
        uuid::Uuid::new_v4(),
        &planning,
        crate::artifacts::planning_store::StoreMode::ManagedLocal,
    );

    let project = attempt_connect_with_store(code.to_str().unwrap(), store.clone()).unwrap();

    assert_eq!(project.state.repo_root, code);
    assert_eq!(project.state.planning_store, store);
    assert!(
        store
            .read(crate::artifacts::planning_store::paths::PRODUCT_INDEX)
            .is_ok()
    );
    assert!(!project.state.repo_root.join("planning/product").exists());
    assert!(
        !project
            .state
            .repo_root
            .join(".koolade-packet/planning/product")
            .exists()
    );

    let _ = std::fs::remove_dir_all(project.state.repo_root);
    let _ = std::fs::remove_dir_all(planning);
}

// ---- parse_github_url --------------------------------------------------
