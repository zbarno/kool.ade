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
