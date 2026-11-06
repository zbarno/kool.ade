use super::super::{LivePost, LiveProgress, TaskActivityActor, TaskActivityOrigin};

fn post(id: (u64, usize), text: &str) -> LivePost {
    LivePost {
        id,
        kind: "tool".into(),
        text: text.into(),
        recorded_at_ms: None,
        run_id: None,
    }
}

#[test]
fn provider_snapshots_coalesce_by_run_and_keep_task_scoped_ordered_history() {
    // Production boundary: LiveProgress::update converts provider snapshots into
    // the operator-local event stream. This unit test covers stable event IDs,
    // first-observed timestamps, redaction and collision fencing. It needs no
    // external provider, Docker, SSH, or setup beyond an in-memory record.
    let mut task_a = LiveProgress::default();
    task_a.begin_task_run("task-a");
    let first = task_a
        .activity_events
        .iter()
        .find(|event| event.kind == "run" && event.status.as_deref() == Some("started"))
        .unwrap()
        .recorded_at_ms;
    task_a.update(LiveProgress {
        posts: vec![post((0, 0), "cargo test TOKEN=synthetic-secret")],
        ..Default::default()
    });
    let first_post_event = task_a
        .activity_events
        .iter()
        .find(|event| event.kind == "tool")
        .unwrap()
        .clone();
    task_a.update(LiveProgress {
        posts: vec![post((0, 0), "cargo test TOKEN=synthetic-secret completed")],
        ..Default::default()
    });
    assert_eq!(task_a.activity_events.len(), 2);
    let post_event = task_a
        .activity_events
        .iter()
        .find(|event| event.kind == "tool")
        .unwrap();
    assert_eq!(post_event.event_id, first_post_event.event_id);
    assert_eq!(post_event.recorded_at_ms, first_post_event.recorded_at_ms);
    assert_eq!(post_event.sequence, first_post_event.sequence);
    assert!(post_event.recorded_at_ms >= first);
    assert!(!post_event.content.contains("synthetic-secret"));
    assert_eq!(post_event.actor, TaskActivityActor::Worker);
    assert_eq!(post_event.origin, TaskActivityOrigin::ProviderObserved);
    let first_run_id = post_event.run_id.clone();
    let prior_sequence = post_event.sequence;
    assert!(
        task_a
            .activity_events
            .windows(2)
            .all(|events| events[0].recorded_at_ms <= events[1].recorded_at_ms)
    );
    let restored: LiveProgress =
        serde_json::from_slice(&serde_json::to_vec(&task_a).unwrap()).unwrap();
    assert_eq!(restored.activity_events, task_a.activity_events);

    task_a.begin_task_run("task-a");
    task_a.update(LiveProgress {
        posts: vec![post((0, 0), "second attempt")],
        ..Default::default()
    });
    assert_eq!(task_a.run_attempt, 2);
    assert_eq!(task_a.posts.len(), 2);
    assert_eq!(task_a.activity_events.last().unwrap().attempt, 2);
    assert!(task_a.activity_events.last().unwrap().sequence > prior_sequence);
    assert_ne!(task_a.activity_events.last().unwrap().run_id, first_run_id);

    let mut task_b = LiveProgress::default();
    task_b.begin_task_run("task-b");
    task_b.update(LiveProgress {
        posts: vec![post((0, 0), "other task")],
        ..Default::default()
    });
    assert!(
        task_b
            .activity_events
            .iter()
            .all(|event| event.task_uid == "task-b")
    );
    assert!(
        task_a
            .activity_events
            .iter()
            .all(|event| event.task_uid == "task-a")
    );
}

#[test]
fn provider_thinking_is_not_retained_as_operator_activity() {
    let mut progress = LiveProgress::default();
    progress.begin_task_run("task-a");
    progress.update(LiveProgress {
        thoughts: "private chain of thought".into(),
        posts: vec![LivePost {
            id: (1, 0),
            kind: "thinking".into(),
            text: "private provider thought".into(),
            recorded_at_ms: None,
            run_id: None,
        }],
        ..Default::default()
    });
    assert!(progress.thoughts.is_empty());
    assert!(progress.posts.is_empty());
    assert_eq!(progress.activity_events.len(), 1);
}

#[test]
fn high_volume_provider_output_is_bounded_before_ui_and_memory_retention() {
    let mut progress = LiveProgress::default();
    progress.begin_task_run("task-a");
    progress.update(LiveProgress {
        posts: vec![post((1, 0), &"x".repeat(10_000))],
        response: "r".repeat(40_000),
        ..Default::default()
    });

    assert!(progress.posts[0].text.chars().count() <= 2_001);
    assert!(progress.response.chars().count() <= 32_001);
    let post_event = progress
        .activity_events
        .iter()
        .find(|event| event.kind == "tool")
        .unwrap();
    assert!(post_event.content.chars().count() <= 2_001);
    let result_event = progress
        .activity_events
        .iter()
        .find(|event| event.kind == "response")
        .unwrap();
    assert!(result_event.content.chars().count() <= 32_001);
}

#[test]
fn retained_post_and_event_windows_are_bounded_and_marked() {
    let mut progress = LiveProgress::default();
    progress.begin_task_run("task-a");
    progress.update(LiveProgress {
        posts: (0..600)
            .map(|index| post((index, 0), &format!("update {index}")))
            .collect(),
        ..Default::default()
    });

    assert_eq!(progress.posts.len(), 500);
    assert_eq!(progress.activity_events.len(), 500);
    assert!(progress.activity_events_truncated_before_ms.is_some());
    assert!(progress.posts.first().unwrap().id.0 >= 100);
}
