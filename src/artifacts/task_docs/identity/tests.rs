use super::*;

#[test]
fn task_uid_survives_file_move_and_title_change() {
    let root = std::env::temp_dir().join(format!(
        "koolade_task_identity_{}-{}",
        std::process::id(),
        chrono::Utc::now().timestamp_nanos_opt().unwrap()
    ));
    std::fs::create_dir_all(&root).unwrap();
    let old_path = root.join("BATCH-001-TASK-001-original-title.md");
    let original = "# Original title\n\nTask context.\n";
    let created = with_path_identity(
        &old_path,
        original,
        "BATCH-001-TASK-001",
        "Original title",
        None,
    )
    .unwrap();
    std::fs::write(&old_path, &created).unwrap();
    let before = ArtifactIdentity::from_markdown(&created).unwrap().unwrap();
    let new_path = root.join("renamed-task.md");
    std::fs::rename(&old_path, &new_path).unwrap();

    let renamed = "# Renamed task\n\nTask context.\n";
    let updated = with_path_identity(
        &new_path,
        renamed,
        "ignored-new-display-id",
        "Renamed task",
        None,
    )
    .unwrap();
    let after = ArtifactIdentity::from_markdown(&updated).unwrap().unwrap();
    assert_eq!(after.uid, before.uid);
    assert_eq!(after.display_id, before.display_id);
    assert_eq!(after.title, "Renamed task");
    assert_eq!(visible_content(&updated), renamed);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn moved_batch_and_renamed_task_are_resolved_by_uid() {
    let root = std::env::temp_dir().join(format!(
        "koolade_batch_identity_{}-{}",
        std::process::id(),
        chrono::Utc::now().timestamp_nanos_opt().unwrap()
    ));
    let task_root = root.join(crate::artifacts::koolade::KOOLADE_TASKS_DIR);
    let old_dir = task_root.join("old-batch-name");
    let new_dir = task_root.join("renamed-batch");
    std::fs::create_dir_all(&old_dir).unwrap();
    let batch_identity = new_batch_identity("Saved searches");
    let index = embed_identity("# Saved searches — task stories\n", &batch_identity).unwrap();
    std::fs::write(old_dir.join("README.md"), index).unwrap();
    let old_task = old_dir.join("001-original-title.md");
    let task_text = with_path_identity(
        &old_task,
        "# Original title\n\nTask context.\n",
        "001-original-title",
        "Original title",
        Some(&batch_identity.uid),
    )
    .unwrap();
    let task_identity = ArtifactIdentity::from_markdown(&task_text)
        .unwrap()
        .unwrap();
    std::fs::write(&old_task, task_text).unwrap();
    std::fs::rename(&old_dir, &new_dir).unwrap();
    std::fs::rename(
        new_dir.join("001-original-title.md"),
        new_dir.join("renamed-task.md"),
    )
    .unwrap();

    let mut workflow = crate::core::workflow::Workflow::default();
    workflow
        .task_batches
        .push(crate::core::workflow::TaskBatchRef {
            identity: Some(batch_identity),
            feature: "Saved searches".into(),
            directory: format!(
                "{}/old-batch-name",
                crate::artifacts::koolade::KOOLADE_TASKS_DIR
            ),
            count: 1,
            created_at_ms: 0,
        });
    let documents = crate::artifacts::task_docs::load_board(&root, &workflow);
    assert_eq!(documents.len(), 1);
    assert!(documents[0].path.ends_with("renamed-batch/renamed-task.md"));
    assert_eq!(documents[0].identity.as_ref(), Some(&task_identity));
    assert!(!documents[0].text.contains("koolade-artifact-id"));
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn legacy_store_resolves_store_relative_batch_directory() {
    let root = std::env::temp_dir().join(format!(
        "koolade_relative_batch_{}-{}",
        std::process::id(),
        uuid::Uuid::new_v4()
    ));
    let directory = root
        .join(crate::artifacts::layout::canonical::ROOT)
        .join("planning/tasks/shared-batch");
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::write(directory.join("README.md"), "# Shared batch\n").unwrap();

    let batch = crate::core::workflow::TaskBatchRef {
        identity: None,
        feature: "Shared batch".into(),
        directory: "planning/tasks/shared-batch".into(),
        count: 0,
        created_at_ms: 0,
    };
    assert_eq!(
        resolve_batch_directory(&root, &batch).as_deref(),
        Some(".koolade-packet/planning/tasks/shared-batch")
    );

    std::fs::remove_dir_all(root).unwrap();
}
