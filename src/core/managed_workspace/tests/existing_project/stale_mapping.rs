use super::super::support::{TestDirectory, code_checkout, git_ok};
use crate::core::managed_workspace::{
    CodeRepositoryRegistration, ExistingProject, PAIR_CODE_REPO_ID, SiblingPairSpec,
};

#[test]
fn stale_sibling_snapshot_does_not_replace_a_new_root_mapping() {
    // Production boundary: the locked mapping update must re-read the local
    // association after an onboarding snapshot becomes stale. Two local Git
    // repos, no network, Docker, or SSH; setup cost is a few Git commands.
    let temp = TestDirectory::new("existing-pair-stale-mapping");
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
    let manager = temp.manager("operator");
    let stale_snapshot = manager.join(&original.planning_repo).unwrap();
    assert!(
        !stale_snapshot
            .association
            .code_checkouts
            .iter()
            .any(|mapping| mapping.repository_id == PAIR_CODE_REPO_ID)
    );

    let (other_checkout, _) = code_checkout(&temp.path, "other-checkout", false);
    let other_checkout = other_checkout.canonicalize().unwrap();
    manager
        .map_code_checkout(original.id, PAIR_CODE_REPO_ID, &other_checkout)
        .unwrap();

    assert!(
        manager
            .map_existing_checkout_if_available(original.id, Some(PAIR_CODE_REPO_ID), &code,)
            .unwrap()
            .is_none(),
        "a mapping added after the snapshot prevents root reuse"
    );
    let current = manager.open(original.id).unwrap();
    assert_eq!(
        current
            .association
            .code_checkouts
            .iter()
            .find(|mapping| mapping.repository_id == PAIR_CODE_REPO_ID)
            .unwrap()
            .path,
        other_checkout
    );
}

#[test]
fn retry_keeps_an_existing_request_id_checkout_mapping() {
    // A previous partial attempt or another window may have registered this
    // request ID elsewhere. Retrying attachment must report the conflict
    // without replacing either that mapping or the sibling pair's root.
    // Local Git only, no network, Docker, or SSH; a few Git commands.
    let temp = TestDirectory::new("existing-pair-stale-request-id");
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
    let manager = temp.manager("operator");
    manager.join(&original.planning_repo).unwrap();

    let (alternate_root, _) = code_checkout(&temp.path, "alternate-root", false);
    let alternate_root = alternate_root.canonicalize().unwrap();
    manager
        .map_code_checkout(original.id, PAIR_CODE_REPO_ID, &alternate_root)
        .unwrap();
    let (alternate_request, _) = code_checkout(&temp.path, "alternate-request", false);
    let alternate_request = alternate_request.canonicalize().unwrap();
    manager
        .register_code_repository(
            original.id,
            CodeRepositoryRegistration {
                id: "selected-code".into(),
                role: "application".into(),
                remote: None,
                checkout: alternate_request.clone(),
            },
        )
        .unwrap();

    let request = ExistingProject {
        folder: code.clone(),
        planning_repo: Some(original.planning_repo.clone()),
        workspace_id: original.id,
        repository_id: "selected-code".into(),
        title: "Koolade".into(),
        create_workspace: false,
    };
    assert!(
        manager
            .attach_existing_project_with_planning_repo(
                &request,
                Some(&original.planning_repo),
                false,
            )
            .is_err()
    );

    let current = manager.open(original.id).unwrap();
    assert_eq!(
        current
            .association
            .code_checkouts
            .iter()
            .find(|mapping| mapping.repository_id == PAIR_CODE_REPO_ID)
            .unwrap()
            .path,
        alternate_root
    );
    assert_eq!(
        current
            .association
            .code_checkouts
            .iter()
            .find(|mapping| mapping.repository_id == "selected-code")
            .unwrap()
            .path,
        alternate_request
    );
    assert!(
        !current
            .association
            .code_checkouts
            .iter()
            .any(|mapping| mapping.path == code)
    );
}
