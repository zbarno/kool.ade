use super::*;

#[test]
fn previous_koolade_root_maps_into_the_distinct_live_root() {
    let previous_path = format!(
        "{}/config/project.md",
        crate::artifacts::layout::previous::ROOT
    );
    assert_eq!(
        plan::destination(&previous_path).unwrap(),
        crate::artifacts::layout::canonical::PROJECT_CONFIG
    );
    assert_ne!(
        crate::artifacts::layout::previous::ROOT,
        crate::artifacts::layout::canonical::ROOT
    );
}

#[test]
fn migration_moves_legacy_bytes_checkpoints_only_its_paths_and_is_idempotent() {
    let root = repo("success");
    let files = [
        ("planning/open-items.md", b"open items\n".as_slice()),
        (
            "planning/features/CHG-001-search/specification.md",
            b"feature\n",
        ),
        (".planner/config.md", b"project settings\n"),
        (".planner/workflow.json", b"{\"taskBatches\":[]}\n"),
        ("adr/decision.md", b"decision\n"),
        (
            "adr/implement-cache-ticket.md",
            b"historical implementation record\n",
        ),
        (
            "adr/ADR-004-storage-policy.md",
            b"durable architecture decision\n",
        ),
        ("SPECIFICATION.md", b"historical specification\n"),
    ];
    for (path, bytes) in files {
        let path = root.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, bytes).unwrap();
    }
    commit_all(&root, "legacy artifacts");
    fs::write(root.join("operator-staged.txt"), "keep staged").unwrap();
    git_ok(&root, &["add", "operator-staged.txt"]);

    let changed = run(&root).unwrap();
    assert!(changed.contains(&crate::artifacts::layout::canonical::MANIFEST.into()));
    for (source, bytes) in files {
        let target = root.join(plan::destination(source).unwrap());
        assert_eq!(fs::read(target).unwrap(), bytes);
        assert!(
            !root.join(source).exists(),
            "legacy source remains: {source}"
        );
    }
    assert_eq!(
        plan::destination("adr/implement-cache-ticket.md").unwrap(),
        ".koolade-packet/planning/archive/implementation-decisions/implement-cache-ticket.md"
    );
    assert_eq!(
        plan::destination("adr/ADR-004-storage-policy.md").unwrap(),
        ".koolade-packet/planning/decisions/ADR-004-storage-policy.md"
    );
    let manifest: serde_json::Value = serde_json::from_slice(
        &fs::read(root.join(crate::artifacts::layout::canonical::MANIFEST)).unwrap(),
    )
    .unwrap();
    assert_eq!(manifest["schemaVersion"], SCHEMA_VERSION);
    assert_eq!(manifest["product"], "Koolade");
    let committed = git_ok(
        &root,
        &["diff-tree", "--no-commit-id", "--name-only", "-r", "HEAD"],
    );
    assert!(!committed.contains("operator-staged.txt"));
    assert_eq!(
        git_ok(&root, &["diff", "--cached", "--name-only"]),
        "operator-staged.txt",
        "migration must leave unrelated staged work in the user's index"
    );
    let head = git_ok(&root, &["rev-parse", "HEAD"]);
    assert!(run(&root).unwrap().is_empty());
    assert_eq!(git_ok(&root, &["rev-parse", "HEAD"]), head);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn conflicting_target_aborts_the_complete_plan_before_any_move() {
    let root = repo("conflict");
    fs::create_dir_all(root.join("planning/imports")).unwrap();
    fs::write(root.join("planning/open-items.md"), "legacy value").unwrap();
    fs::write(root.join("planning/imports/reference.md"), "must stay put").unwrap();
    let target = root.join(crate::artifacts::layout::canonical::OPEN_ITEMS);
    fs::create_dir_all(target.parent().unwrap()).unwrap();
    fs::write(&target, "different target value").unwrap();

    let error = run(&root).unwrap_err().to_string();
    assert!(error.contains("Migration conflict"));
    assert_eq!(
        fs::read_to_string(root.join("planning/open-items.md")).unwrap(),
        "legacy value"
    );
    assert_eq!(
        fs::read_to_string(root.join("planning/imports/reference.md")).unwrap(),
        "must stay put"
    );
    assert_eq!(
        fs::read_to_string(target).unwrap(),
        "different target value"
    );
    assert!(
        !root
            .join(crate::artifacts::layout::canonical::MANIFEST)
            .exists()
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn restart_finishes_checkpoint_after_files_and_manifest_are_written() {
    let root = repo("restart");
    fs::create_dir_all(root.join("planning")).unwrap();
    fs::write(
        root.join("planning/open-items.md"),
        "items survive restart\n",
    )
    .unwrap();
    commit_all(&root, "legacy artifacts");
    let hooks = root.join("blocked-hooks");
    fs::create_dir_all(&hooks).unwrap();
    let hook = hooks.join("pre-commit");
    fs::write(&hook, "#!/bin/sh\nexit 1\n").unwrap();
    fs::set_permissions(&hook, fs::Permissions::from_mode(0o755)).unwrap();
    git_ok(
        &root,
        &["config", "core.hooksPath", hooks.to_str().unwrap()],
    );

    assert!(run(&root).is_err());
    assert_eq!(
        fs::read_to_string(root.join(crate::artifacts::layout::canonical::OPEN_ITEMS)).unwrap(),
        "items survive restart\n"
    );
    assert!(!root.join("planning/open-items.md").exists());
    assert!(
        root.join(crate::artifacts::layout::canonical::MANIFEST)
            .exists()
    );
    assert!(common_dir(&root).unwrap().join(PENDING_NAME).exists());

    fs::remove_file(hook).unwrap();
    run(&root).unwrap();
    assert!(!common_dir(&root).unwrap().join(PENDING_NAME).exists());
    let committed = git_ok(
        &root,
        &["diff-tree", "--no-commit-id", "--name-only", "-r", "HEAD"],
    );
    assert!(committed.contains(crate::artifacts::layout::canonical::OPEN_ITEMS));
    assert!(committed.contains(crate::artifacts::layout::canonical::MANIFEST));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn legacy_transaction_is_rolled_back_before_its_artifacts_are_migrated() {
    let root = repo("legacy-transaction");
    let legacy = root.join("planning/open-items.md");
    fs::create_dir_all(legacy.parent().unwrap()).unwrap();
    fs::write(&legacy, "partially written").unwrap();
    let journal = common_dir(&root)
        .unwrap()
        .join("koolade-planning-transaction.json");
    fs::write(
        &journal,
        r#"{"entries":[{"path":"planning/open-items.md","before":"original bytes","after":"partially written"}]}"#,
    )
    .unwrap();

    assert!(crate::artifacts::migration::recover_transaction(&root).unwrap());
    assert_eq!(fs::read_to_string(&legacy).unwrap(), "original bytes");
    run(&root).unwrap();
    assert_eq!(
        fs::read_to_string(root.join(crate::artifacts::layout::canonical::OPEN_ITEMS)).unwrap(),
        "original bytes"
    );
    assert!(!legacy.exists());
    assert!(!journal.exists());
    let _ = fs::remove_dir_all(root);
}
