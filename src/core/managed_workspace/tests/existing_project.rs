use super::support::{TestDirectory, code_checkout, git_ok};
use crate::core::managed_workspace::{ExistingProject, ExistingProjectKind};

#[test]
fn plain_folder_requires_consent_and_keeps_files_uncommitted() {
    // Real Git initialization/managed mapping boundary. Local Git only; two
    // tiny repos. A validator test cannot prove no app files were committed.
    let temp = TestDirectory::new("existing-plain");
    let folder = temp.path.join("application");
    std::fs::create_dir(&folder).unwrap();
    std::fs::write(folder.join("notes.txt"), b"existing work").unwrap();
    let folder = folder.canonicalize().unwrap();
    let manager = temp.manager("operator");
    let request = ExistingProject {
        folder: folder.clone(),
        workspace_id: uuid::Uuid::new_v4(),
        repository_id: "application".into(),
        title: "Existing notes".into(),
        create_workspace: true,
    };
    assert_eq!(
        manager.inspect_existing_project(&folder).unwrap(),
        ExistingProjectKind::NeedsHistory
    );
    assert!(manager.attach_existing_project(&request, false).is_err());
    assert!(!folder.join(".git").exists());
    let workspace = manager.attach_existing_project(&request, true).unwrap();
    assert_eq!(workspace.association.code_checkouts[0].path, folder);
    assert_eq!(
        std::fs::read(folder.join("notes.txt")).unwrap(),
        b"existing work"
    );
    assert_eq!(git_ok(Some(&folder), &["ls-files"]).stdout, b"");
}

#[test]
fn existing_checkout_preserves_head_index_configuration_and_changes() {
    // Real dirty checkout attachment boundary, no external services. The
    // repository/index/config bytes prove registration did not rewrite code.
    let temp = TestDirectory::new("existing-tracked");
    let (folder, _) = code_checkout(&temp.path, "application", false);
    git_ok(
        Some(&folder),
        &[
            "-c",
            "user.name=Example Developer",
            "-c",
            "user.email=developer@example.invalid",
            "commit",
            "--allow-empty",
            "-m",
            "Initial test commit",
        ],
    );
    let folder = folder.canonicalize().unwrap();
    std::fs::write(folder.join("staged.txt"), b"staged work").unwrap();
    git_ok(Some(&folder), &["add", "staged.txt"]);
    std::fs::write(folder.join("unstaged.txt"), b"unstaged work").unwrap();
    let head = git_ok(Some(&folder), &["rev-parse", "HEAD"]).stdout;
    let index = std::fs::read(folder.join(".git/index")).unwrap();
    let config = std::fs::read(folder.join(".git/config")).unwrap();
    let manager = temp.manager("operator");
    let request = ExistingProject {
        folder: folder.clone(),
        workspace_id: uuid::Uuid::new_v4(),
        repository_id: "application".into(),
        title: "Existing app".into(),
        create_workspace: true,
    };
    manager.attach_existing_project(&request, false).unwrap();
    assert_eq!(head, git_ok(Some(&folder), &["rev-parse", "HEAD"]).stdout);
    assert_eq!(index, std::fs::read(folder.join(".git/index")).unwrap());
    assert_eq!(config, std::fs::read(folder.join(".git/config")).unwrap());
    assert_eq!(
        std::fs::read(folder.join("unstaged.txt")).unwrap(),
        b"unstaged work"
    );
}

#[test]
fn selected_external_planning_checkout_cannot_be_nested_in_application() {
    // Real managed planning join and Git boundary, no network. A canonical
    // state-root-only check cannot protect externally joined planning repos.
    let temp = TestDirectory::new("nested-planning");
    let source = temp.manager("source").create("Nested plans").unwrap();
    let folder = temp.path.join("application");
    std::fs::create_dir(&folder).unwrap();
    let nested = folder.join("plans");
    std::fs::rename(&source.planning_repo, &nested).unwrap();
    let manager = temp.manager("operator");
    manager.join(&nested).unwrap();
    let request = ExistingProject {
        folder: folder.canonicalize().unwrap(),
        workspace_id: source.id,
        repository_id: "application".into(),
        title: "App".into(),
        create_workspace: false,
    };
    let before = git_ok(Some(&nested), &["rev-parse", "HEAD"]).stdout;
    assert!(manager.attach_existing_project(&request, true).is_err());
    assert!(!folder.join(".git").exists());
    assert_eq!(before, git_ok(Some(&nested), &["rev-parse", "HEAD"]).stdout);
}
