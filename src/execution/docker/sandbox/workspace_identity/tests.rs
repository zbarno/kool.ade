use super::from_metadata;

#[test]
fn windows_file_identity_is_stable_and_distinguishes_sibling_directories() {
    let root = tempfile::tempdir().unwrap();
    let first = root.path().join("first");
    let second = root.path().join("second");
    std::fs::create_dir(&first).unwrap();
    std::fs::create_dir(&second).unwrap();

    let first_metadata = std::fs::symlink_metadata(&first).unwrap();
    let same_metadata = std::fs::symlink_metadata(&first).unwrap();
    let second_metadata = std::fs::symlink_metadata(&second).unwrap();

    let first_identity = from_metadata(&first_metadata, &first).unwrap();
    assert_eq!(
        first_identity,
        from_metadata(&same_metadata, &first).unwrap()
    );
    assert_ne!(
        first_identity,
        from_metadata(&second_metadata, &second).unwrap()
    );
}
