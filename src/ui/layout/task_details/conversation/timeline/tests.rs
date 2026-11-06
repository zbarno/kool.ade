use super::*;
use crate::domain::chatlog::ChatMessage;

#[test]
fn timeline_merges_human_conversation_and_scoped_worker_events_by_time() {
    // Production boundary: this is the Task Details timeline's merge adapter.
    // It proves the UI input combines durable chat and task-scoped activity in
    // one chronological sequence; a model test cannot cover this adapter.
    // No provider, Docker, SSH, or external setup is required.
    let mut progress = LiveProgress::default();
    progress.begin_task_run("task-a");
    progress.activity_events.clear();
    progress
        .activity_events
        .push(crate::harness::TaskActivityEventV1 {
            schema_version: 1,
            event_id: "run-a:tool:1".into(),
            sequence: 2,
            task_uid: "task-a".into(),
            run_id: "run-a".into(),
            attempt: 1,
            provider: Some("codex".into()),
            recorded_at_ms: 2_000,
            actor: TaskActivityActor::Worker,
            origin: TaskActivityOrigin::ProviderObserved,
            kind: "tool".into(),
            status: None,
            content: "Read source file".into(),
            visibility: crate::harness::TaskActivityVisibility::UserSafe,
        });
    let mut second_event = progress.activity_events[0].clone();
    second_event.event_id = "run-a:tool:0".into();
    second_event.sequence = 3;
    second_event.content = "Run validation".into();
    progress.activity_events.push(second_event);
    let mut messages = vec![ChatMessage::new(
        ChatRole::User,
        "Please update the task",
        None,
    )];
    messages[0].ts = chrono::DateTime::from_timestamp_millis(2_000).unwrap();
    progress.activity_events[0].recorded_at_ms = messages[0].ts.timestamp_millis();
    progress.activity_events[1].recorded_at_ms = messages[0].ts.timestamp_millis();

    let mut merged = entries(&messages, Some(&progress));
    merged.sort_by(chronological_order);

    assert_eq!(merged.len(), 3);
    assert_eq!(merged[0].text, "Read source file");
    assert_eq!(merged[0].sequence, Some(2));
    assert_eq!(merged[0].run, Some(1));
    assert_eq!(merged[0].origin, Some("Observed"));
    assert_eq!(merged[1].text, "Run validation");
    assert_eq!(merged[1].sequence, Some(3));
    assert_eq!(merged[2].category, Filter::Conversation);
    assert!(merged.iter().all(|entry| entry.at_ms == 2_000));
}
