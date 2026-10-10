use std::{fs, path::Path};

use crate::core::managed_workspace::{CreationFailurePoint, WorkspaceManager};

use super::support::{TestDirectory, git_ok};

#[test]
fn retries_recover_each_creation_boundary_without_allocating_another_id() {
    for (label, point) in [
        ("git-init", CreationFailurePoint::GitInit),
        ("initial-commit", CreationFailurePoint::InitialCommit),
        ("rename", CreationFailurePoint::Renamed),
        ("association", CreationFailurePoint::Associated),
    ] {
        let temp = TestDirectory::new(label);
        let manager = temp.manager("operator");
        let error = manager.create_with_failure("Retry me", point).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("injected workspace creation failure")
        );
        let id = only_pending_id(&manager);

        let recovered = manager.create("Ignored after recovery").unwrap();

        assert_eq!(recovered.id, id);
        assert!(manager.association_path(id).is_file());
        assert_eq!(
            git_ok(
                Some(&recovered.planning_repo),
                &["rev-parse", "--show-toplevel"]
            )
            .stdout
            .trim_ascii(),
            recovered.planning_repo.to_string_lossy().as_bytes()
        );
        assert!(!manager.project_dir(id).join("creation.json").exists());
        assert_eq!(
            git_ok(Some(&recovered.planning_repo), &["status", "--porcelain"]).stdout,
            b""
        );
    }
}

#[test]
fn open_fails_for_missing_associated_repository_without_creating_a_replacement() {
    let temp = TestDirectory::new("missing-associated-repo");
    let manager = temp.manager("operator");
    let workspace = manager.create("Will go missing").unwrap();
    fs::remove_dir_all(&workspace.planning_repo).unwrap();

    let error = manager.open(workspace.id).unwrap_err();

    assert!(format!("{error:#}").contains("missing or unreadable"));
    assert!(!workspace.planning_repo.exists());
    assert!(manager.association_path(workspace.id).is_file());
}

#[test]
fn opening_an_unknown_workspace_does_not_create_an_operator_directory() {
    let temp = TestDirectory::new("unknown-workspace");
    let manager = temp.manager("operator");
    let id = uuid::Uuid::new_v4();

    let error = manager.open(id).unwrap_err();

    assert!(format!("{error:#}").contains("no local association"));
    assert!(!manager.project_dir(id).exists());
}

#[test]
fn open_reports_corrupt_manifest_and_identity_mismatch() {
    let temp = TestDirectory::new("manifest-validation");
    let manager = temp.manager("operator");
    let workspace = manager.create("Manifest check").unwrap();
    let manifest_path = workspace.planning_repo.join("manifest.json");
    let original = fs::read(&manifest_path).unwrap();

    fs::write(&manifest_path, b"{").unwrap();
    assert!(
        format!("{:#}", manager.open(workspace.id).unwrap_err())
            .contains("planning manifest is invalid")
    );

    let mut manifest: serde_json::Value = serde_json::from_slice(&original).unwrap();
    manifest["workspaceId"] = serde_json::Value::String(uuid::Uuid::new_v4().to_string());
    fs::write(
        &manifest_path,
        serde_json::to_vec_pretty(&manifest).unwrap(),
    )
    .unwrap();
    let identity_error = manager.open(workspace.id).unwrap_err();
    assert!(
        format!("{identity_error:#}").contains("identity mismatch"),
        "{identity_error:#}"
    );
}

#[test]
fn open_fails_when_workspace_lock_is_held() {
    let temp = TestDirectory::new("locked");
    let manager = temp.manager("operator");
    let workspace = manager.create("Locked").unwrap();
    let lock_path = manager.project_dir(workspace.id).join(".workspace.lock");
    let lock = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(lock_path)
        .unwrap();
    lock.try_lock().unwrap();

    let error = manager.open(workspace.id).unwrap_err();
    assert!(error.to_string().contains("locked"));
}

fn only_pending_id(manager: &WorkspaceManager) -> uuid::Uuid {
    let entries = fs::read_dir(manager.home_for_test().join("projects")).unwrap();
    let directories = entries
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
        .collect::<Vec<_>>();
    assert_eq!(directories.len(), 1);
    let entry = &directories[0];
    uuid::Uuid::parse_str(&entry.file_name().to_string_lossy()).unwrap()
}

trait ManagerPaths {
    fn home_for_test(&self) -> &Path;
}

impl ManagerPaths for WorkspaceManager {
    fn home_for_test(&self) -> &Path {
        &self.home
    }
}
