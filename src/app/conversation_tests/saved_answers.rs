use super::*;

/// Production boundary: a malformed provider response becomes a bounded durable
/// recovery and is dispatched/applied using the original user message. Store
/// tests cannot prove adoption and redispatch together. Temporary Git and mock
/// provider only; no Docker/network; ordinary low-cost application fixture.
#[test]
fn malformed_saved_reply_recovers_and_applies_without_resubmission() {
    let mut app = fixture();
    let key;
    if let Screen::Connected(project) = &mut app.screen {
        let mut work = crate::core::planning_work::Work::new(
            "planning:recovery".into(),
            "Investigate storage".into(),
            "Explain local storage".into(),
            "Investigating".into(),
        );
        work.kind = crate::core::planning_work::WorkKind::Question;
        key = work.key.clone();
        project.planning_work.push(work);
        project.save_planning_work().unwrap();
        project
            .task_chats
            .drafts
            .insert(key.clone(), "Explain SQLite durability".into());
    } else {
        panic!("connected");
    }
    app.task_harness = Some(Box::new(ReplyHarness {
        prompts: Arc::new(Mutex::new(Vec::new())),
        reply: "invalid response without an envelope".into(),
    }));
    app.submit_task_reply(&key);
    complete(&mut app);
    let Screen::Connected(project) = &app.screen else {
        panic!("connected");
    };
    let user_id = project.task_chats.messages[&key]
        .iter()
        .find(|message| message.role == ChatRole::User)
        .unwrap()
        .id
        .clone();
    assert!(project.task_chats.has_pending_user_reply(&key));
    assert!(project.task_chats.has_failed_task_reply(&key));
    let prompts = Arc::new(Mutex::new(Vec::new()));
    app.task_harness = Some(Box::new(ReplyHarness { prompts: prompts.clone(), reply: serde_json::json!({
        "schema_version":2,"assistant_message":"SQLite durability explained.","open_items_added":[],"open_items_updated":[],"open_items_resolved":[]
    }).to_string() }));
    app.retry_pending_task_reply();
    complete(&mut app);
    let Screen::Connected(project) = &app.screen else {
        panic!("connected");
    };
    assert!(!project.task_chats.has_pending_user_reply(&key));
    assert!(!project.task_chats.has_failed_task_reply(&key));
    let messages = &project.task_chats.messages[&key];
    assert_eq!(
        messages
            .iter()
            .filter(|message| message.role == ChatRole::User && !message.clarification)
            .count(),
        1
    );
    assert_eq!(
        messages
            .iter()
            .find(|message| message.role == ChatRole::User)
            .unwrap()
            .id,
        user_id
    );
    assert!(messages.iter().any(|message| {
        message
            .id
            .starts_with("internal-task-reply-outcome:applied:")
    }));
    assert!(prompts.lock().unwrap()[0].contains("Repair the reported response format"));
}
