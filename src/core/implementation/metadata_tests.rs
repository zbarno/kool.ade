use super::*;

#[test]
fn task_repository_routing_requires_current_metadata() {
    let mut identity = crate::domain::ArtifactIdentity::new("TASK-001", "Task");
    identity.parent_uid = Some(uuid::Uuid::new_v4().to_string());
    let metadata =
        crate::artifacts::task_docs::TaskMetadata::new(&identity, "api", vec![]).unwrap();
    assert_eq!(task_repository_id(Some(&metadata)).unwrap(), "api");
    assert_eq!(
        task_repository_id(None).unwrap_err().to_string(),
        "Task story has no current repository metadata"
    );
}
