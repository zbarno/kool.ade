use super::support::{TestDirectory, code_checkout, git_ok};
use crate::core::managed_workspace::{ExistingProject, ExistingProjectKind, SiblingPairSpec};

#[path = "existing_project/stale_mapping.rs"]
mod stale_mapping;

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
        planning_repo: None,
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
        planning_repo: None,
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
fn existing_sibling_pair_opens_with_its_existing_plan_repository() {
    // Production boundary: local Git join plus workspace checkout mapping.
    // A unit test cannot prove the existing dirty checkout stays untouched
    // while its separate planning repository is joined by manifest identity.
    // Uses two temporary local Git repos; no network, Docker, or SSH. Setup is
    // a few Git commands and has low cost.
    let temp = TestDirectory::new("existing-sibling-pair");
    let pair_parent = temp.path.join("code");
    std::fs::create_dir_all(&pair_parent).unwrap();
    let (code, _) = code_checkout(&temp.path, "kool.ade", false);
    git_ok(
        Some(&code),
        &[
            "-c",
            "user.name=Example Developer",
            "-c",
            "user.email=developer@example.invalid",
            "commit",
            "--allow-empty",
            "-m",
            "Existing code",
        ],
    );
    let code = code.canonicalize().unwrap();
    let mut spec = SiblingPairSpec::new("Koolade", "kool.ade");
    spec.parent_dir = pair_parent.clone();
    spec.code_already_exists = true;
    let original = temp
        .manager("pair-owner")
        .create_sibling_pair(&spec)
        .unwrap();
    let planning_repo = pair_parent.join("kool.ade.packet").canonicalize().unwrap();

    std::fs::write(code.join("staged.txt"), b"staged work").unwrap();
    git_ok(Some(&code), &["add", "staged.txt"]);
    std::fs::write(code.join("unstaged.txt"), b"unstaged work").unwrap();
    let head = git_ok(Some(&code), &["rev-parse", "HEAD"]).stdout;
    let status = git_ok(Some(&code), &["status", "--porcelain"]).stdout;
    let index = std::fs::read(code.join(".git/index")).unwrap();
    let config = std::fs::read(code.join(".git/config")).unwrap();

    let manager = temp.manager("operator");
    let request = ExistingProject {
        folder: code.clone(),
        planning_repo: Some(planning_repo.clone()),
        workspace_id: uuid::Uuid::new_v4(),
        repository_id: "selected-code".into(),
        title: "Koolade".into(),
        create_workspace: false,
    };
    let opened = manager
        .attach_existing_project_with_planning_repo(&request, Some(&planning_repo), false)
        .unwrap();

    assert_eq!(
        opened.id, original.id,
        "the planning manifest identity wins"
    );
    assert_eq!(opened.repositories.repositories.len(), 1);
    assert_eq!(opened.association.code_checkouts.len(), 1);
    assert_eq!(opened.association.code_checkouts[0].path, code);
    assert_eq!(head, git_ok(Some(&code), &["rev-parse", "HEAD"]).stdout);
    assert_eq!(index, std::fs::read(code.join(".git/index")).unwrap());
    assert_eq!(config, std::fs::read(code.join(".git/config")).unwrap());
    assert_eq!(
        status,
        git_ok(Some(&code), &["status", "--porcelain"]).stdout
    );
    assert_eq!(
        std::fs::read(code.join("staged.txt")).unwrap(),
        b"staged work"
    );
    assert_eq!(
        std::fs::read(code.join("unstaged.txt")).unwrap(),
        b"unstaged work"
    );
}

#[test]
fn attaching_a_sibling_pair_keeps_an_existing_root_mapping() {
    // Production boundary: attachment must not silently replace a separate
    // local code checkout already mapped to the planning repository's root.
    // Uses two temporary local Git repos; no network, Docker, or SSH. Setup is
    // a few Git commands and has low cost.
    let temp = TestDirectory::new("existing-pair-mapped-root");
    let pair_parent = temp.path.join("code");
    std::fs::create_dir_all(&pair_parent).unwrap();
    let (code, _) = code_checkout(&temp.path, "kool.ade", false);
    git_ok(
        Some(&code),
        &[
            "-c",
            "user.name=Example Developer",
            "-c",
            "user.email=developer@example.invalid",
            "commit",
            "--allow-empty",
            "-m",
            "Existing code",
        ],
    );
    let code = code.canonicalize().unwrap();
    let mut spec = SiblingPairSpec::new("Koolade", "kool.ade");
    spec.parent_dir = pair_parent;
    spec.code_already_exists = true;
    let original = temp
        .manager("pair-owner")
        .create_sibling_pair(&spec)
        .unwrap();
    let planning_repo = original.planning_repo.clone();

    let (alternate, _) = code_checkout(&temp.path, "alternate", false);
    git_ok(
        Some(&alternate),
        &[
            "-c",
            "user.name=Example Developer",
            "-c",
            "user.email=developer@example.invalid",
            "commit",
            "--allow-empty",
            "-m",
            "Alternate checkout",
        ],
    );
    let alternate = alternate.canonicalize().unwrap();
    let manager = temp.manager("operator");
    manager.join(&planning_repo).unwrap();
    manager
        .map_code_checkout(original.id, super::super::PAIR_CODE_REPO_ID, &alternate)
        .unwrap();

    let request = ExistingProject {
        folder: code.clone(),
        planning_repo: Some(planning_repo.clone()),
        workspace_id: original.id,
        repository_id: "selected-code".into(),
        title: "Koolade".into(),
        create_workspace: false,
    };
    let attached = manager
        .attach_existing_project_with_planning_repo(&request, Some(&planning_repo), false)
        .unwrap();

    assert_eq!(
        attached
            .association
            .code_checkouts
            .iter()
            .find(|checkout| checkout.repository_id == super::super::PAIR_CODE_REPO_ID)
            .unwrap()
            .path,
        alternate
    );
    assert!(
        attached
            .association
            .code_checkouts
            .iter()
            .any(|checkout| { checkout.repository_id == "selected-code" && checkout.path == code })
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
        planning_repo: None,
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
