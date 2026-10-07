use super::*;

#[test]
fn ignored_progress_is_migrated_but_excluded_from_git_checkpoint() {
    let root = repo("ignored-progress");
    let batch = "planning/tasks/demo-batch";
    let story = format!("{batch}/001-demo.md");
    let progress = format!("{batch}/.koolade-progress.json");
    fs::write(
        root.join(".gitignore"),
        ".koolade-packet/planning/tasks/**/.koolade-progress.json\n",
    )
    .unwrap();
    fs::create_dir_all(root.join(batch)).unwrap();
    fs::write(
        root.join(&story),
        "# Demo task\n\n## Acceptance criteria\n\n- Preserve local progress.\n",
    )
    .unwrap();
    fs::write(root.join(&progress), r#"{"feature":"Demo batch"}"#).unwrap();
    commit_all(&root, "legacy task artifacts");

    let migrated_progress = format!("{}/{progress}", crate::artifacts::layout::canonical::ROOT);
    let migrated_story = format!("{}/{story}", crate::artifacts::layout::canonical::ROOT);
    let migrated_progress_path = root.join(&migrated_progress);

    run(&root).expect("ignored local progress should not block artifact migration");

    let progress_value: serde_json::Value =
        serde_json::from_slice(&fs::read(&migrated_progress_path).unwrap()).unwrap();
    assert!(progress_value.get("identity").is_some());
    assert!(!root.join(&progress).exists());
    assert!(!root.join(&story).exists());
    let committed = git_ok(
        &root,
        &["diff-tree", "--no-commit-id", "--name-only", "-r", "HEAD"],
    );
    assert!(committed.contains(&migrated_story));
    assert!(committed.contains(crate::artifacts::layout::canonical::MANIFEST));
    assert!(!committed.contains(&migrated_progress));
    assert!(
        git(&root, &["check-ignore", "-q", &migrated_progress])
            .status
            .success()
    );
    assert!(!common_dir(&root).unwrap().join(PENDING_NAME).exists());

    let _ = fs::remove_dir_all(root);
}
