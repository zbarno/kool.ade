#[path = "completion/replay_integrity.rs"]
mod replay_integrity;
#[path = "completion/spec_sync.rs"]
mod spec_sync;
#[path = "completion/support.rs"]
mod support;

use super::Fixture;
use crate::core::lifecycle::LifecycleStore;
#[path = "completion/done_gates.rs"]
mod done_gates;
#[path = "completion/pr_authorization.rs"]
mod pr_authorization;
#[path = "completion/provider_refresh.rs"]
mod provider_refresh;

#[test]
fn replay_does_not_restore_parent_without_subtask_projection() {
    let fixture = Fixture::new();
    let store = fixture.store();
    let lifecycle = LifecycleStore::new(&store);
    let (feature_uid, task_uid, _child_uid, _) = support::create_feature_graph(&lifecycle);
    let parent_path = fixture
        .0
        .join(format!("state/lifecycle_tasks/{task_uid}.json"));
    let subtask_event = lifecycle
        .events_for_feature(&feature_uid)
        .unwrap()
        .into_iter()
        .find(|event| {
            matches!(
                &event.event,
                crate::core::lifecycle::LifecycleEventKind::SubtaskCreated { parent_task_uid }
                    if parent_task_uid == &task_uid
            )
        })
        .unwrap();
    let event_path = fixture
        .0
        .join(format!("state/lifecycle_events/{}.json", subtask_event.uid));
    let mut event: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&event_path).unwrap()).unwrap();
    event.as_object_mut().unwrap().remove("parentTaskAfter");
    std::fs::write(&event_path, serde_json::to_vec(&event).unwrap()).unwrap();
    std::fs::remove_file(&parent_path).unwrap();

    assert!(lifecycle.replay_feature(&feature_uid).is_err());
    assert!(!parent_path.exists());
}

#[path = "completion/review_regressions.rs"]
mod review_regressions;

#[path = "completion/publication_retries.rs"]
mod publication_retries;
