use super::support::{TestDirectory, git_ok};
use crate::core::managed_workspace::ApplicationCreation;

#[test]
#[cfg(any(target_os = "linux", target_os = "macos"))]
fn application_creation_is_separate_and_retry_keeps_identities_and_user_files() {
    // Boundary: real local Git, staged folder publication and managed registry.
    // Unit validators cannot prove two-repository recovery. Two tiny repos,
    // no network, provider, Docker or package installation required.
    let temp = TestDirectory::new("application-creation");
    let manager = temp.manager("operator");
    let request = ApplicationCreation {
        transaction_id: uuid::Uuid::new_v4(),
        workspace_id: uuid::Uuid::new_v4(),
        title: "Garden notes".into(),
        intent: "Track planting dates".into(),
        code_directory: temp.path.canonicalize().unwrap().join("garden"),
    };
    let workspace = manager.create_application(&request).unwrap();
    assert_eq!(workspace.manifest.title.as_deref(), Some("Garden notes"));
    assert_ne!(workspace.planning_repo, request.code_directory);
    assert_eq!(workspace.repositories.repositories.len(), 1);
    let work = crate::core::planning_work::load(&workspace.store).unwrap();
    assert_eq!(work.len(), 1);
    assert_eq!(work[0].request, request.intent);
    let head = git_ok(Some(&workspace.planning_repo), &["rev-parse", "HEAD"]).stdout;
    std::fs::write(request.code_directory.join("notes.txt"), b"user work").unwrap();
    let reopened = manager.create_application(&request).unwrap();
    assert_eq!(workspace.id, reopened.id);
    assert_eq!(
        crate::core::planning_work::load(&reopened.store)
            .unwrap()
            .len(),
        1
    );
    assert_eq!(workspace.repositories, reopened.repositories);
    assert_eq!(
        head,
        git_ok(Some(&reopened.planning_repo), &["rev-parse", "HEAD"]).stdout
    );
    assert_eq!(
        std::fs::read(request.code_directory.join("notes.txt")).unwrap(),
        b"user work"
    );
    let mut other = request.clone();
    other.transaction_id = uuid::Uuid::new_v4();
    other.workspace_id = uuid::Uuid::new_v4();
    assert!(manager.create_application(&other).is_err());
}

#[test]
fn creation_refuses_unowned_staging_repository_without_changing_it() {
    let temp = TestDirectory::new("occupied-staging");
    let manager = temp.manager("operator");
    let request = ApplicationCreation {
        transaction_id: uuid::Uuid::new_v4(),
        workspace_id: uuid::Uuid::new_v4(),
        title: "Garden".into(),
        intent: "Planting dates".into(),
        code_directory: temp.path.canonicalize().unwrap().join("garden"),
    };
    let staging = temp
        .path
        .join(format!(".koolade-create-{}", request.transaction_id));
    std::fs::create_dir(&staging).unwrap();
    git_ok(Some(&staging), &["init", "--quiet"]);
    git_ok(
        Some(&staging),
        &[
            "remote",
            "add",
            "origin",
            "https://example.invalid/project.git",
        ],
    );
    let config = std::fs::read(staging.join(".git/config")).unwrap();
    assert!(manager.create_application(&request).is_err());
    assert_eq!(config, std::fs::read(staging.join(".git/config")).unwrap());
    assert!(!request.code_directory.exists());
}
