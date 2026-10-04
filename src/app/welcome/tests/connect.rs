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

// ---- parse_github_url --------------------------------------------------
