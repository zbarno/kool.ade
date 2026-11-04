use super::*;
use crate::artifacts::task_docs::TaskDocument;
use std::collections::{BTreeMap, BTreeSet};

fn task(path: &str, feature: &str, file: &str) -> TaskDocument {
    let uid = uuid::Uuid::new_v4().to_string();
    let mut identity = crate::domain::ArtifactIdentity::new(path, path);
    identity.uid = uid;
    identity.parent_uid = Some(uuid::Uuid::from_u128(10_000).to_string());
    let metadata = crate::artifacts::task_docs::TaskMetadata::new(&identity, "root", vec![])
        .expect("valid fixture task metadata");
    TaskDocument {
        path: path.into(),
        title: path.into(),
        text: format!(
            "# Task\n\nFeature ID: {feature}\n\n## Dependencies\nNone.\n\n## Affected files and components\n- {file}\n"
        ),
        identity: Some(identity),
        metadata: Some(metadata),
        task_state: None,

        metadata_error: None,
    }
}

#[test]
fn independent_tasks_from_different_features_can_fill_parallel_slots() {
    let docs = vec![
        task("tasks/a.md", "CHG-001", "src/a.rs"),
        task("tasks/b.md", "CHG-002", "src/b.rs"),
    ];
    let states = BTreeMap::new();
    let excluded = BTreeSet::new();
    let first = next_ready_ticket_with_running_scopes(&docs, &states, &excluded, &BTreeSet::new())
        .unwrap()
        .unwrap();
    let running = BTreeSet::from([first.clone()]);
    let second = next_ready_ticket_with_running_scopes(&docs, &states, &excluded, &running)
        .unwrap()
        .unwrap();
    assert_ne!(first, second);
}
