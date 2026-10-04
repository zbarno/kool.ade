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
    };
    assert!(bad_repo.validate(None).is_err());

    let markdown =
        "# Task\n\n[not a dependency](other.md)\n\n## Dependencies\n\n- [Task](001-task.md)\n";
    assert_eq!(legacy_dependencies(markdown).unwrap(), vec!["001-task.md"]);
}

#[test]
fn foreign_legacy_front_matter_loads_without_becoming_koolade_metadata() {
    let markdown = "---\ntitle: Legacy task\nowner: team\n---\n\n# Legacy task\n";
    assert!(parse(markdown).unwrap().is_none());
    assert!(visible_content(markdown).starts_with("# Legacy task"));
}
