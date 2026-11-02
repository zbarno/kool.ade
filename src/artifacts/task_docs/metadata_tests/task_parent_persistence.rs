use super::*;

#[test]
fn explicit_task_parent_survives_story_save_and_board_reload() {
    let root = std::env::temp_dir().join(format!(
        "koolade-task-parent-reload-{}-{}",
        std::process::id(),
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&root).unwrap();
    init_git(&root);

    let batch = crate::core::workflow::TaskBatch {
        brief: crate::core::workflow::InterviewBrief {
            feature_name: "Task parent reload fixture".into(),
            ..Default::default()
        },
        specification: "# Task parent reload fixture".into(),
        feature_id: None,
        contract: None,
        branch_targets: None,
        task_routing: Default::default(),
        stories: vec![
            crate::core::workflow::TaskStory {
                title: "Parent task".into(),
                ..Default::default()
            },
            crate::core::workflow::TaskStory {
                title: "Nested child task".into(),
                parent_task_index: Some(1),
                ..Default::default()
            },
        ],
    };
    crate::artifacts::task_docs::save_progress(&root, "parent-reload", &batch, 2).unwrap();

    let loaded =
        crate::artifacts::task_docs::load_board(&root, &crate::core::workflow::Workflow::default());
    let parent = loaded
        .iter()
        .find(|document| document.title.ends_with("Parent task"))
        .unwrap();
    let child = loaded
        .iter()
        .find(|document| document.title.ends_with("Nested child task"))
        .unwrap();
    assert_eq!(
        child
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.parent_task_uid.as_deref()),
        Some(parent.identity.as_ref().unwrap().uid.as_str())
    );

    std::fs::remove_dir_all(root).unwrap();
}
