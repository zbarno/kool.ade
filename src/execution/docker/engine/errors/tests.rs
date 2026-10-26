use super::daemon_storage_capacity;

#[test]
fn docker_desktop_capacity_stays_unknown_for_an_accessible_absolute_path() {
    let root = tempfile::tempdir().unwrap();
    let root = root.path().to_str().unwrap();

    assert_eq!(
        daemon_storage_capacity(Some(root), true, true),
        (None, false)
    );
}
