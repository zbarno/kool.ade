use super::*;

#[test]
/// Production boundary: a saved answer whose worker failed is reported as
/// internal recovery and does not trigger another proactive user question.
/// Uses a temporary local Git fixture and synthetic messages; no Docker or
/// network is required.
fn failed_saved_answer_is_recovered_internally_without_a_repeat_question() {
    let _environment = crate::test_support::lock_process_environment();
    let mut app = fixture();
    let item = crate::domain::OpenItem::new(
        "CLR-037".into(),
        crate::domain::Priority::Blocking,
        crate::domain::ItemKind::Question,
        "General".into(),
        None,
        "Which branch should receive this task?".into(),
        "The task needs a destination.".into(),
    );
    let key = item.conversation_key().to_owned();
    if let Screen::Connected(project) = &mut app.screen {
        project.state.items.push(item);
        project.bind_task_conversation_identities();
        project.task_chats.ensure_loaded(&project.chat_slug);
        project
            .task_chats
            .append(
                &project.chat_slug,
                &key,
                vec![crate::domain::ChatMessage::new(
                    crate::domain::ChatRole::User,
                    "Use release/2.1.",
                    Some(key.clone()),
                )],
            )
            .unwrap();
        let mut failure = crate::domain::ChatMessage::new(
            crate::domain::ChatRole::System,
            "Task generation needs attention: the saved answer could not be processed.",
            Some(key.clone()),
        );
        crate::persistence::task_chats::TaskChats::mark_task_reply_outcome(&mut failure);
        project
            .task_chats
            .append(&project.chat_slug, &key, vec![failure])
            .unwrap();
    } else {
        panic!("fixture remains connected");
    }

    let ctx = egui::Context::default();
    app.tick(0.016, &ctx);
    frame(&mut app, &ctx, vec![]);
    frame(&mut app, &ctx, vec![]);
    assert_ne!(
        ctx.data_mut(|data| data.get_temp::<String>(egui::Id::new("koolade_selected_planning")))
            .as_deref(),
        Some("CLR-037"),
        "a worker failure must not ask the user to repeat a saved answer"
    );
    let Screen::Connected(project) = &app.screen else {
        panic!("fixture remains connected");
    };
    assert!(project.task_chats.has_failed_task_reply(&key));
    let manager_prompt = crate::app::manager::Manager::prompt_body(project, &[]);
    assert!(manager_prompt.contains("Internal recovery required"));
    assert!(manager_prompt.contains("without asking the user to repeat it"));
    let waiting = manager_prompt
        .split("Waiting on user: ")
        .nth(1)
        .unwrap()
        .split('\n')
        .next()
        .unwrap();
    assert!(!waiting.contains("CLR-037"));
}
