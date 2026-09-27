use super::*;

fn feature_repo(tag: &str, status: &str) -> (PathBuf, PathBuf, String) {
    let root = repo(tag);
    let relative = format!(
        "{}/CHG-001-search/specification.md",
        crate::artifacts::layout::canonical::CHANGES
    );
    let path = root.join(&relative);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let markdown =
        format!("# CHG-001: Search\n\n**Status:** {status}\n\n## Intent\n\nPersist searches.\n");
    fs::write(&path, &markdown).unwrap();
    (root, path, markdown)
}

#[test]
fn legacy_implementing_status_migrates_to_typed_metadata() {
    let (root, path, _) = feature_repo("legacy-implementing", "Implementing");
    run(&root).unwrap();
    let markdown = fs::read_to_string(path).unwrap();
    let identity = crate::domain::ArtifactIdentity::from_markdown(&markdown)
        .unwrap()
        .unwrap();
    let metadata = crate::domain::ChangeMetadata::require_markdown(&markdown).unwrap();
    assert_eq!(metadata.status, crate::domain::ChangeStatus::Implementing);
    assert_eq!(metadata.uid, identity.uid);
    assert_eq!(metadata.display_id, "CHG-001");
    let _ = fs::remove_dir_all(root);
}

#[test]
fn unknown_legacy_status_fails_before_mutating_the_change() {
    let (root, path, original) = feature_repo("unknown-status", "Ready whenever");
    assert!(run(&root).is_err());
    assert_eq!(fs::read_to_string(path).unwrap(), original);
    assert!(
        !root
            .join(crate::artifacts::layout::canonical::MANIFEST)
            .exists()
    );
    let _ = fs::remove_dir_all(root);
}
