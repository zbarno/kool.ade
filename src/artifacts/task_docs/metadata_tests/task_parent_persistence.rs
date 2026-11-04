#[test]
fn explicit_task_parent_survives_story_save_and_board_reload() {
    let root = std::env::temp_dir().join(format!(
        "koolade-task-parent-reload-{}-{}",
        std::process::id(),
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&root).unwrap();

    let batch = crate::core::workflow::TaskBatch {
        brief: crate::core::workflow::InterviewBrief {
            feature_name: "Task parent reload fixture".into(),
            ..Default::default()
        },
        specification: "# Task parent reload fixture".into(),
        feature_id: None,
        contract: None,
        repository_git_intents: std::collections::BTreeMap::from([(
            "root".into(),
            crate::test_support::approved_git_intent("root", "main", &"a".repeat(40), "main"),
        )]),
        task_routing: Default::default(),
        stories: vec![
            crate::core::workflow::TaskStory {
                source_sha: Some("a".repeat(40)),
                title: "Parent task".into(),
                target_repository: "root".into(),
                ..Default::default()
            },
            crate::core::workflow::TaskStory {
                source_sha: Some("a".repeat(40)),
                title: "Nested child task".into(),
                target_repository: "root".into(),
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

#[test]
// Boundary: staged task batch persistence to reloaded task metadata. It checks
// structural parent linkage does not falsely grant cross-repository Git-intent
// inheritance, which a unit test of either metadata validator cannot prove.
// Dependencies: local planning store only; setup cost is a temporary directory.
fn child_on_another_repository_keeps_parent_link_without_false_git_inheritance() {
    let root = std::env::temp_dir().join(format!(
        "koolade-cross-repo-parent-{}-{}",
        std::process::id(),
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&root).unwrap();
    let root_sha = "a".repeat(40);
    let api_sha = "b".repeat(40);
    let batch = crate::core::workflow::TaskBatch {
        brief: crate::core::workflow::InterviewBrief {
            feature_name: "Cross repository task fixture".into(),
            ..Default::default()
        },
        specification: "# Cross repository task fixture".into(),
        feature_id: None,
        contract: None,
        repository_git_intents: std::collections::BTreeMap::from([
            (
                "root".into(),
                crate::test_support::approved_git_intent("root", "main", &root_sha, "main"),
            ),
            (
                "api".into(),
                crate::test_support::approved_git_intent("api", "release/2", &api_sha, "main"),
            ),
        ]),
        task_routing: Default::default(),
        stories: vec![
            crate::core::workflow::TaskStory {
                source_sha: Some(root_sha),
                title: "Root task".into(),
                target_repository: "root".into(),
                ..Default::default()
            },
            crate::core::workflow::TaskStory {
                source_sha: Some(api_sha),
                title: "API child task".into(),
                target_repository: "api".into(),
                parent_task_index: Some(1),
                ..Default::default()
            },
        ],
    };
    crate::artifacts::task_docs::save_progress(&root, "cross-repo-parent", &batch, 2).unwrap();
    let loaded =
        crate::artifacts::task_docs::load_board(&root, &crate::core::workflow::Workflow::default());
    let parent = loaded
        .iter()
        .find(|document| document.title.ends_with("Root task"))
        .unwrap();
    let child = loaded
        .iter()
        .find(|document| document.title.ends_with("API child task"))
        .unwrap();
    assert_eq!(
        child.metadata.as_ref().unwrap().parent_task_uid.as_deref(),
        Some(parent.identity.as_ref().unwrap().uid.as_str())
    );
    assert_eq!(
        child
            .metadata
            .as_ref()
            .unwrap()
            .git_intent
            .as_ref()
            .unwrap()
            .inherited_from_task_uid,
        None
    );
    std::fs::remove_dir_all(root).unwrap();
}
