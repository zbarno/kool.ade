use super::*;
#[test]
fn interrupted_document_set_restores_original_bytes() {
    let root = std::env::temp_dir().join(format!("koolade_tx_{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join(".koolade-packet/planning/product")).unwrap();
    assert!(
        std::process::Command::new("git")
            .args(["init", "-q"])
            .current_dir(&root)
            .status()
            .unwrap()
            .success()
    );
    let a = root.join(".koolade-packet/planning/product/01-vision.md");
    let b = root.join(".koolade-packet/planning/product/02-scope.md");
    fs::write(&a, "old a").unwrap();
    fs::write(&b, "old b").unwrap();
    let changes = vec![
        (
            ".koolade-packet/planning/product/01-vision.md".into(),
            "new a".into(),
        ),
        (
            ".koolade-packet/planning/product/02-scope.md".into(),
            "new b".into(),
        ),
    ];
    assert!(apply_with_limit(&root, &changes, Some(1)).is_err());
    assert_eq!(fs::read_to_string(a).unwrap(), "old a");
    assert_eq!(fs::read_to_string(b).unwrap(), "old b");
    assert!(!journal_path(&root).unwrap().exists());
    assert_eq!(apply(&root, &changes).unwrap().len(), 2);
    let _ = fs::remove_dir_all(root);
}
#[test]
fn interrupted_plan_adoption_bundle_recovers_after_restart() {
    let root = std::env::temp_dir().join(format!(
        "koolade_tx_recover_{}-{}",
        std::process::id(),
        chrono::Utc::now().timestamp_nanos_opt().unwrap()
    ));
    fs::create_dir_all(root.join(".koolade-packet/planning/changes")).unwrap();
    fs::create_dir_all(root.join(".koolade-packet/planning/decisions")).unwrap();
    fs::create_dir_all(root.join(".koolade-packet/state")).unwrap();
    assert!(
        std::process::Command::new("git")
            .args(["init", "-q"])
            .current_dir(&root)
            .status()
            .unwrap()
            .success()
    );
    let feature = root.join(".koolade-packet/planning/changes/F7/specification.md");
    let adr = root.join(".koolade-packet/planning/decisions/ADR-001.md");
    let workflow = root.join(".koolade-packet/state/workflow.json");
    fs::create_dir_all(feature.parent().unwrap()).unwrap();
    fs::write(&feature, "old feature").unwrap();
    fs::write(&workflow, "old workflow").unwrap();
    let entries = vec![
        Entry {
            path: ".koolade-packet/planning/changes/F7/specification.md".into(),
            before: Some("old feature".into()),
            after: "adopted Plan B".into(),
        },
        Entry {
            path: ".koolade-packet/planning/decisions/ADR-001.md".into(),
            before: None,
            after: "Plan B decision record".into(),
        },
        Entry {
            path: ".koolade-packet/state/workflow.json".into(),
            before: Some("old workflow".into()),
            after: "adopted comparison record".into(),
        },
    ];
    fs::write(
        journal_path(&root).unwrap(),
        serde_json::to_vec(&Journal {
            root: root_identity(&root).unwrap(),
            entries,
        })
        .unwrap(),
    )
    .unwrap();
    fs::write(&feature, "adopted Plan B").unwrap();
    fs::write(&adr, "Plan B decision record").unwrap();
    assert!(recover(&root).unwrap());
    assert_eq!(fs::read_to_string(&feature).unwrap(), "old feature");
    assert_eq!(fs::read_to_string(&workflow).unwrap(), "old workflow");
    assert!(!adr.exists());
    assert!(!journal_path(&root).unwrap().exists());
    assert!(!recover(&root).unwrap());
    let _ = fs::remove_dir_all(root);
}

#[test]
fn linked_worktrees_use_distinct_legacy_transaction_state() {
    let parent = std::env::temp_dir().join(format!(
        "koolade_tx_worktrees_{}-{}",
        std::process::id(),
        chrono::Utc::now().timestamp_nanos_opt().unwrap()
    ));
    let primary = parent.join("primary");
    let linked = parent.join("linked");
    fs::create_dir_all(&primary).unwrap();
    assert!(
        std::process::Command::new("git")
            .args(["init", "-q"])
            .current_dir(&primary)
            .status()
            .unwrap()
            .success()
    );
    assert!(
        std::process::Command::new("git")
            .args([
                "-c",
                "user.name=Test User",
                "-c",
                "user.email=test@example.invalid",
                "commit",
                "--allow-empty",
                "-m",
                "fixture",
            ])
            .current_dir(&primary)
            .status()
            .unwrap()
            .success()
    );
    assert!(
        std::process::Command::new("git")
            .args([
                "worktree",
                "add",
                "-q",
                "-b",
                "linked",
                linked.to_str().unwrap(),
            ])
            .current_dir(&primary)
            .status()
            .unwrap()
            .success()
    );

    assert_ne!(
        journal_path(&primary).unwrap(),
        journal_path(&linked).unwrap()
    );
    let primary_lock = transaction_lock(&primary).unwrap();
    let linked_lock = transaction_lock(&linked).unwrap();
    drop(primary_lock);
    drop(linked_lock);
    let _ = fs::remove_dir_all(parent);
}
