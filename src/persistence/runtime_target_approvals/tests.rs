use super::*;

#[test]
fn approval_state_requires_operator_home_and_uses_it_over_shared_state_override() {
    assert!(operator_approval_root_from(None, None).is_err());
    let home = tempfile::tempdir().unwrap();
    let profile = tempfile::tempdir().unwrap();
    assert_eq!(
        operator_approval_root_from(
            Some(home.path().to_path_buf()),
            Some(profile.path().to_path_buf()),
        )
        .unwrap(),
        home.path().join(".koolade-packet")
    );
    assert_eq!(
        operator_approval_root_from(None, Some(profile.path().to_path_buf())).unwrap(),
        profile.path().join(".koolade-packet")
    );
}

#[test]
fn approval_state_rejects_a_home_inside_a_git_checkout() {
    let repo = tempfile::tempdir().unwrap();
    fs::create_dir(repo.path().join(".git")).unwrap();
    let error = operator_approval_root_from(Some(repo.path().to_path_buf()), None)
        .unwrap_err()
        .to_string();
    assert!(error.contains("inside a Git repository"));
}

#[cfg(unix)]
#[test]
fn approval_state_rejects_a_symlinked_home_inside_a_git_checkout() {
    let repo = tempfile::tempdir().unwrap();
    fs::create_dir(repo.path().join(".git")).unwrap();
    let link = tempfile::tempdir().unwrap();
    let home_link = link.path().join("home");
    std::os::unix::fs::symlink(repo.path(), &home_link).unwrap();
    let error = operator_approval_root_from(Some(home_link), None)
        .unwrap_err()
        .to_string();
    assert!(error.contains("inside a Git repository"));
}

#[cfg(unix)]
#[test]
fn approval_state_rejects_a_symlink_into_a_git_checkout() {
    let repo = tempfile::tempdir().unwrap();
    fs::create_dir(repo.path().join(".git")).unwrap();
    let home = tempfile::tempdir().unwrap();
    std::os::unix::fs::symlink(repo.path(), home.path().join(".koolade-packet")).unwrap();
    let error = operator_approval_root_from(Some(home.path().to_path_buf()), None)
        .unwrap_err()
        .to_string();
    assert!(error.contains("inside a Git repository") || error.contains("real directory"));
}

#[test]
fn one_user_receipt_can_bind_a_target_and_each_recipe_without_overwriting() {
    let home = tempfile::tempdir().unwrap();
    let _home = crate::test_support::KooladeHomeGuard::set(home.path());
    let target = binding();
    let mut build = target.clone();
    build.proposal_sha256 = "c".repeat(64);
    let mut start = target.clone();
    start.proposal_sha256 = "d".repeat(64);

    // Read an existing single-binding record from the pre-migration layout.
    let legacy_path = legacy_record_path(&operator_approval_root().unwrap(), &target).unwrap();
    let bytes = serde_json::to_vec(&Record {
        schema_version: 1,
        binding: target.clone(),
    })
    .unwrap();
    crate::artifacts::atomic_create_bytes(&legacy_path, &bytes).unwrap();
    set_private_file(&legacy_path).unwrap();

    save(&build).unwrap();
    save(&start).unwrap();

    assert!(contains(&target).unwrap());
    assert!(contains(&build).unwrap());
    assert!(contains(&start).unwrap());
    revoke(&build).unwrap();
    assert!(contains(&target).unwrap());
    assert!(!contains(&build).unwrap());
    assert!(contains(&start).unwrap());
}

fn binding() -> Binding {
    Binding {
        workspace_id: uuid::Uuid::new_v4().hyphenated().to_string(),
        task_id: uuid::Uuid::new_v4().hyphenated().to_string(),
        repository_id: "synthetic-runtime-qa".into(),
        source_sha: "a".repeat(40),
        target_id: "fixture-target".into(),
        proposal_sha256: "b".repeat(64),
        receipt_id: uuid::Uuid::new_v4().hyphenated().to_string(),
    }
}
