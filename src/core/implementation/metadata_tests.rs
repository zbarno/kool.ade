use super::*;

#[test]
fn repository_routing_uses_packet_metadata_before_legacy_markdown_labels() {
    let manifest = crate::core::project_repos::ProjectManifest {
        repositories: vec![
            crate::core::project_repos::Repository {
                id: "root".into(),
                role: "Planning root".into(),
                remote: "https://example.test/root".into(),
                display_name: None,
            },
            crate::core::project_repos::Repository {
                id: "api".into(),
                role: "API".into(),
                remote: "https://example.test/api".into(),
                display_name: None,
            },
            crate::core::project_repos::Repository {
                id: "web".into(),
                role: "Web client".into(),
                remote: "https://example.test/web".into(),
                display_name: None,
            },
        ],
    };
    let mut identity = crate::domain::ArtifactIdentity::new("TASK-001", "Task");
    identity.parent_uid = Some(uuid::Uuid::new_v4().to_string());
    let metadata =
        crate::artifacts::task_docs::TaskMetadata::new(&identity, "api", vec![]).unwrap();
    let markdown = "# Task\n\nRepository: web\n";
    assert_eq!(
        task_repository_id(markdown, Some(&metadata), &manifest).unwrap(),
        "api"
    );
    let late_legacy_target = format!("# Legacy\n{}Repository: web\n", "\n".repeat(40));
    assert_eq!(
        task_repository_id(&late_legacy_target, None, &manifest)
            .unwrap_err()
            .to_string(),
        "Multi-repository legacy task lacks a repository target"
    );
    assert_eq!(
        task_repository_id("# Legacy\n\nRepository: web\n", None, &manifest).unwrap(),
        "web"
    );
}
