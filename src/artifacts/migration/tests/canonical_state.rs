use super::*;

fn work_ledger(root: &Path) -> (PathBuf, PathBuf, Vec<u8>) {
    let source = root.join(crate::artifacts::layout::previous::WORK);
    let target = root.join(crate::artifacts::layout::canonical::WORK);
    fs::create_dir_all(source.parent().unwrap()).unwrap();
    fs::create_dir_all(target.parent().unwrap()).unwrap();
    let bytes = serde_json::to_vec_pretty(&serde_json::json!([{
        "key": "planning:preserve-this-card",
        "title": "Keep the planning card",
        "request": "The old canonical location is still authoritative until migrated.",
        "column": 2,
        "feature": null,
        "detail": "Card identity and history survive the layout correction."
    }]))
    .unwrap();
    (source, target, bytes)
}

fn add_origin(root: &Path) {
    git_ok(
        root,
        &[
            "remote",
            "add",
            "origin",
            "https://example.test/koolade.git",
        ],
    );
}

#[test]
fn migration_moves_previous_work_path_and_materializes_portable_repository_manifest() {
    let root = repo("canonical-state");
    add_origin(&root);
    let (source, target, bytes) = work_ledger(&root);
    fs::write(&source, &bytes).unwrap();
    commit_all(&root, "previous canonical work state");

    let changed = run(&root).unwrap();

    assert!(changed.contains(&crate::artifacts::layout::previous::WORK.into()));
    assert!(changed.contains(&crate::artifacts::layout::canonical::WORK.into()));
    assert!(!source.exists());
    assert_eq!(fs::read(&target).unwrap(), bytes);

    let manifest_path = root.join(crate::artifacts::layout::canonical::PROJECT_MANIFEST);
    let manifest: crate::core::project_repos::ProjectManifest =
        serde_json::from_slice(&fs::read(&manifest_path).unwrap()).unwrap();
    manifest.validate().unwrap();
    assert_eq!(manifest.repositories.len(), 1);
    assert_eq!(manifest.repositories[0].id, "root");
    assert_eq!(
        manifest.repositories[0].remote,
        "https://example.test/koolade.git"
    );

    let head = git_ok(&root, &["rev-parse", "HEAD"]);
    assert!(run(&root).unwrap().is_empty());
    assert_eq!(git_ok(&root, &["rev-parse", "HEAD"]), head);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn conflicting_previous_work_target_stops_before_writing_the_project_manifest() {
    let root = repo("canonical-state-conflict");
    add_origin(&root);
    let (source, target, bytes) = work_ledger(&root);
    fs::write(&source, &bytes).unwrap();
    fs::write(&target, b"different canonical state\n").unwrap();
    commit_all(&root, "conflicting work state");

    let error = run(&root).unwrap_err().to_string();

    assert!(error.contains("Migration conflict"));
    assert_eq!(fs::read(&source).unwrap(), bytes);
    assert_eq!(fs::read(&target).unwrap(), b"different canonical state\n");
    assert!(
        !root
            .join(crate::artifacts::layout::canonical::PROJECT_MANIFEST)
            .exists()
    );
    assert!(!common_dir(&root).unwrap().join(PENDING_NAME).exists());
    let _ = fs::remove_dir_all(root);
}
