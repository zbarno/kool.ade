use super::*;

#[test]
// Production boundary: this verifies mode-only output through the Windows
// file transaction and real Git index update. A helper test cannot prove the
// repository records mode 100755. Requires local Git and a temp directory;
// no Docker or network setup is needed.
fn tracked_mode_only_change_updates_git_index() {
    let fixture = tempfile::tempdir().unwrap();
    fs::write(fixture.path().join("script.sh"), b"same contents").unwrap();
    let update = returned_mode("script.sh", b"same contents", 0o755);

    apply_test(
        fixture.path(),
        &baseline("script.sh", b"same contents"),
        &update,
        None,
        |_| false,
    )
    .unwrap();

    assert_eq!(
        git_index_executable(fixture.path(), "script.sh"),
        Some(true)
    );
    assert_eq!(
        fs::read(fixture.path().join("script.sh")).unwrap(),
        b"same contents"
    );
}

#[test]
// Production boundary: a tracked executable bit removed by the container is
// committed as Git mode 100644 while the Windows file remains readable and
// writable. Requires local Git and a temp directory, with no Docker/network.
fn tracked_executable_removal_updates_git_index() {
    let fixture = tempfile::tempdir().unwrap();
    fs::write(fixture.path().join("script.sh"), b"same contents").unwrap();
    initialize_git(fixture.path());
    set_git_index_executable(fixture.path(), "script.sh", true);
    let update = returned_mode("script.sh", b"same contents", 0o644);

    apply_initialized_test(
        fixture.path(),
        &baseline_mode("script.sh", b"same contents", 0o755),
        &update,
        None,
        |_| false,
    )
    .unwrap();

    assert_eq!(
        git_index_executable(fixture.path(), "script.sh"),
        Some(false)
    );
    assert_eq!(
        fs::read(fixture.path().join("script.sh")).unwrap(),
        b"same contents"
    );
}

#[test]
// Production boundary: an injected failure after `update-index --chmod`
// must restore both the previous file and its index mode. The real Git index
// is necessary to prove rollback. Requires local Git and a temp directory.
fn failed_file_commit_rolls_back_git_mode_and_workspace_content() {
    let fixture = tempfile::tempdir().unwrap();
    fs::write(fixture.path().join("script.sh"), b"baseline").unwrap();
    let update = returned_mode("script.sh", b"container", 0o755);

    let result = apply_test(
        fixture.path(),
        &baseline("script.sh", b"baseline"),
        &update,
        None,
        |checkpoint| matches!(checkpoint, Checkpoint::AfterIndexModeUpdate { .. }),
    );

    assert!(result.is_err());
    assert_eq!(
        git_index_executable(fixture.path(), "script.sh"),
        Some(false)
    );
    assert_eq!(
        fs::read(fixture.path().join("script.sh")).unwrap(),
        b"baseline"
    );
    assert!(!contains_recovery_file(
        fixture.path(),
        ".koolade-conflict-",
        b"container"
    ));
}

#[test]
// Production boundary: an untracked executable output must be rejected
// before stage-out mutates the workspace. Only a real Git index and the
// Windows stage-out path prove trackedness is checked before writes.
fn new_executable_output_is_rejected_before_workspace_changes() {
    let fixture = tempfile::tempdir().unwrap();
    let update = returned_mode("new-script.sh", b"container", 0o755);

    let result = apply_test(fixture.path(), &Manifest::new(), &update, None, |_| {
        panic!("untracked executable output must be rejected before writes")
    });

    assert!(result.is_err());
    assert!(!fixture.path().join("new-script.sh").exists());
    assert_eq!(git_index_executable(fixture.path(), "new-script.sh"), None);
}
