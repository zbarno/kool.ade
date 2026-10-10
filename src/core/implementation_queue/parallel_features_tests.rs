use super::*;
use crate::artifacts::task_docs::TaskDocument;
use std::collections::{BTreeMap, BTreeSet};

fn task(path: &str, feature: &str, file: &str) -> TaskDocument {
    TaskDocument {
        path: path.into(),
        title: path.into(),
        text: format!(
            "# Task\n\nFeature ID: {feature}\n\n## Dependencies\nNone.\n\n## Affected files and components\n- {file}\n"
        ),
        identity: None,
        metadata: None,
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
