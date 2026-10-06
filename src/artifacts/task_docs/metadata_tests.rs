use super::*;

fn identified_story() -> (crate::domain::ArtifactIdentity, String, String) {
    let batch = crate::domain::ArtifactIdentity::new("BATCH-001", "Batch");
    let mut task = crate::domain::ArtifactIdentity::new("TASK-001", "Story");
    task.parent_uid = Some(batch.uid.clone());
    (task, batch.uid, uuid::Uuid::new_v4().to_string())
}

#[test]
fn execution_metadata_round_trips_and_body_formatting_cannot_change_it() {
    let (identity, batch_uid, dependency) = identified_story();
    let metadata = TaskMetadata::new(&identity, "api", vec![dependency.clone()]).unwrap();
    let markdown = embed(
        "# Story\n\nRepository: web\n\n## Dependencies\n\nNone.\n",
        &metadata,
    )
    .unwrap();
    let parsed = parse(&markdown).unwrap().unwrap();
    assert_eq!(parsed, metadata);
    assert_eq!(parsed.batch_uid, batch_uid);
    assert_eq!(parsed.dependency_uids, vec![dependency]);
    assert!(visible_content(&markdown).contains("Repository: web"));
    assert!(!visible_content(&markdown).starts_with("---"));

    let edited_body = markdown
        .replace("Repository: web", "Repository: root")
        .replace("## Dependencies", "## Prerequisites");
    assert_eq!(parse(&edited_body).unwrap(), Some(metadata));
}

#[test]
fn invalid_metadata_fails_closed_and_legacy_parser_is_bounded_to_dependency_section() {
    let malformed = "---\nkoolade-task: {broken}\n---\n# Story\n";
    assert!(parse(malformed).is_err());
    let identity = crate::domain::ArtifactIdentity::new("TASK-1", "Story");
    let bad_repo = TaskMetadata {
        schema_version: 1,
        uid: identity.uid.clone(),
        batch_uid: uuid::Uuid::new_v4().to_string(),
        repository_id: "../api".into(),
        dependency_uids: vec![],
        source_branch: None,
        destination_branch: None,
    };
    assert!(bad_repo.validate(None).is_err());

    let markdown =
        "# Task\n\n[not a dependency](other.md)\n\n## Dependencies\n\n- [Task](001-task.md)\n";
    assert_eq!(legacy_dependencies(markdown).unwrap(), vec!["001-task.md"]);
}

#[test]
fn branch_targets_round_trip_and_legacy_metadata_keeps_optional_intent_empty() {
    let (identity, _, _) = identified_story();
    let targets = crate::core::workflow::BranchTargets {
        source: "feature/platform-refactor".into(),
        destination: "integration".into(),
    };
    let metadata = TaskMetadata::new(&identity, "root", vec![])
        .unwrap()
        .with_branch_targets(Some(&targets))
        .unwrap();
    assert_eq!(
        metadata.source_branch.as_deref(),
        Some("feature/platform-refactor")
    );
    assert_eq!(metadata.destination_branch.as_deref(), Some("integration"));

    let legacy = serde_json::json!({
        "schemaVersion": 1,
        "uid": identity.uid,
        "batchUid": identity.parent_uid.unwrap(),
        "repositoryId": "root",
        "dependencyUids": []
    });
    let legacy: TaskMetadata = serde_json::from_value(legacy).unwrap();
    legacy.validate(None).unwrap();
    assert_eq!(legacy.source_branch, None);
    assert_eq!(legacy.destination_branch, None);
}

#[test]
fn generated_task_stories_copy_the_batch_source_and_destination() {
    let root = std::env::temp_dir().join(format!(
        "koolade-branch-metadata-{}-{}",
        std::process::id(),
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&root).unwrap();
    let targets = crate::core::workflow::BranchTargets {
        source: "release/2.1".into(),
        destination: "integration".into(),
    };
    let batch = crate::core::workflow::TaskBatch {
        brief: crate::core::workflow::InterviewBrief {
            feature_name: "Branch target feature".into(),
            ..Default::default()
        },
        specification: "# Branch target feature".into(),
        feature_id: None,
        contract: None,
        branch_targets: Some(targets),
        stories: vec![crate::core::workflow::TaskStory {
            title: "Implement branch selection".into(),
            ..Default::default()
        }],
    };
    crate::artifacts::task_docs::save_progress(&root, "branch-run", &batch, 1).unwrap();
    let docs =
        crate::artifacts::task_docs::load_board(&root, &crate::core::workflow::Workflow::default());
    let metadata = docs[0].metadata.as_ref().unwrap();
    assert_eq!(metadata.source_branch.as_deref(), Some("release/2.1"));
    assert_eq!(metadata.destination_branch.as_deref(), Some("integration"));
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn foreign_legacy_front_matter_loads_without_becoming_koolade_metadata() {
    let markdown = "---\ntitle: Legacy task\nowner: team\n---\n\n# Legacy task\n";
    assert!(parse(markdown).unwrap().is_none());
    assert!(visible_content(markdown).starts_with("# Legacy task"));
}
