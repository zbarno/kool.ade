use super::*;

#[test]
/// Production boundary: a copy-only edit to the same clarification does not
/// create another proactive prompt. A unit test cannot prove persisted event
/// deduplication and UI focus together; local Git fixture setup takes a few
/// seconds and needs no Docker or network.
fn changed_copy_of_the_same_human_item_does_not_repeat_its_question() {
    let _environment = crate::test_support::lock_process_environment();
    let mut app = fixture();
    let item = crate::domain::OpenItem::new(
        "CLR-031".into(),
        crate::domain::Priority::Blocking,
        crate::domain::ItemKind::Question,
        "General".into(),
        None,
        "Which branch should receive this task?".into(),
        "The task needs a destination.".into(),
    );
    if let Screen::Connected(project) = &mut app.screen {
        project.state.items.push(item);
    }
    let ctx = egui::Context::default();
    app.tick(0.016, &ctx);
    frame(&mut app, &ctx, vec![]);
    frame(&mut app, &ctx, vec![]);
    assert_eq!(
        ctx.data_mut(|data| data.get_temp::<String>(egui::Id::new("koolade_selected_planning")))
            .as_deref(),
        Some("CLR-031")
    );
    frame(
        &mut app,
        &ctx,
        vec![egui::Event::Key {
            key: egui::Key::Escape,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Default::default(),
        }],
    );
    if let Screen::Connected(project) = &mut app.screen {
        let item = project
            .state
            .items
            .iter_mut()
            .find(|item| item.id == "CLR-031")
            .unwrap();
        item.question = "What is the target branch?".into();
        item.reason = "A destination must be recorded.".into();
    }
    app.tick(0.016, &ctx);
    frame(&mut app, &ctx, vec![]);
    assert!(
        ctx.data_mut(|data| data.get_temp::<String>(egui::Id::new("koolade_selected_planning")))
            .is_none(),
        "copy edits must not manufacture a second action for the same item"
    );
    if let Screen::Connected(project) = &app.screen {
        assert_eq!(project.task_chats.messages["CLR-031"].len(), 1);
    }
}

#[test]
/// Production boundary: Manager prompt construction sees a saved answer and
/// removes that item from `Waiting on user` until an agent responds. A helper
/// unit test cannot prove Manager's assembled prompt; fixture setup uses local
/// Git repositories and in-memory chat, takes a few seconds, and needs no
/// Docker or network.
fn saved_human_reply_is_not_reported_as_waiting_on_the_user() {
    let _environment = crate::test_support::lock_process_environment();
    let mut app = fixture();
    let item = crate::domain::OpenItem::new(
        "CLR-032".into(),
        crate::domain::Priority::Blocking,
        crate::domain::ItemKind::Question,
        "General".into(),
        None,
        "Which branch should receive this task?".into(),
        "The task needs a destination.".into(),
    );
    if let Screen::Connected(project) = &mut app.screen {
        project.state.items.push(item);
        let task_key = "planning/tasks/fixture/001-task.md";
        project.queue.blocked.insert(
            task_key.into(),
            crate::core::implementation::Failure::other(
                "The user must choose whether to wait for provider capacity.",
            ),
        );
        project.task_chats.messages.insert(
            "CLR-032".into(),
            vec![crate::domain::chatlog::ChatMessage::new(
                crate::domain::chatlog::ChatRole::User,
                "Use release/2.1.",
                Some("CLR-032".into()),
            )],
        );
        project.task_chats.messages.insert(
            task_key.into(),
            vec![crate::domain::chatlog::ChatMessage::new(
                crate::domain::chatlog::ChatRole::User,
                "Wait until capacity is available.",
                None,
            )],
        );
    }

    let Screen::Connected(project) = &app.screen else {
        panic!("fixture remains connected")
    };
    let prompt = crate::app::manager::Manager::prompt_body(project, &[]);
    let waiting = prompt
        .split_once("Waiting on user: ")
        .unwrap()
        .1
        .split_once("\nSaved user replies awaiting internal processing:")
        .unwrap()
        .0;
    let pending = prompt
        .split_once("Saved user replies awaiting internal processing: ")
        .unwrap()
        .1
        .split_once("\nBlocked on external work or dependencies:")
        .unwrap()
        .0;
    assert!(!waiting.contains("CLR-032"));
    assert!(!waiting.contains("First task"));
    assert!(pending.contains("CLR-032"));
    assert!(pending.contains("First task"));

    let Screen::Connected(project) = &mut app.screen else {
        panic!("fixture remains connected")
    };
    project
        .task_chats
        .messages
        .get_mut("CLR-032")
        .unwrap()
        .push(crate::domain::chatlog::ChatMessage::new(
            crate::domain::chatlog::ChatRole::Agent,
            "Which release branch name should be used?",
            Some("CLR-032".into()),
        ));
    project
        .task_chats
        .messages
        .get_mut("planning/tasks/fixture/001-task.md")
        .unwrap()
        .push(crate::domain::chatlog::ChatMessage::new(
            crate::domain::chatlog::ChatRole::Agent,
            "Capacity is required before the task can resume.",
            None,
        ));
    let prompt = crate::app::manager::Manager::prompt_body(project, &[]);
    let waiting = prompt
        .split_once("Waiting on user: ")
        .unwrap()
        .1
        .split_once("\nSaved user replies awaiting internal processing:")
        .unwrap()
        .0;
    assert!(waiting.contains("CLR-032"));
    assert!(waiting.contains("First task"));
}

#[test]
/// Production boundary: proactive attention does not frame an automatic retry
/// as a human action. A routing unit test cannot prove no UI conversation is
/// opened; local Git fixture setup takes a few seconds and needs no Docker or
/// network.
fn automatic_retry_blocker_is_not_presented_as_a_user_action() {
    let _environment = crate::test_support::lock_process_environment();
    let mut app = fixture();
    let key = "planning/tasks/fixture/001-task.md";
    if let Screen::Connected(project) = &mut app.screen {
        project.queue.blocked.insert(
            key.into(),
            crate::core::implementation::Failure::new(
                crate::core::implementation::FailureKind::Harness,
                crate::core::implementation::RecoveryDisposition::AutomaticRetry,
                "Provider startup is retrying internally.",
            ),
        );
    }
    let ctx = egui::Context::default();
    app.tick(0.016, &ctx);
    frame(&mut app, &ctx, vec![]);
    frame(&mut app, &ctx, vec![]);
    assert!(
        ctx.data_mut(|data| data.get_temp::<String>(egui::Id::new("koolade_selected_task")))
            .is_none()
    );
    if let Screen::Connected(project) = &app.screen {
        assert!(!project.task_chats.messages.contains_key(key));
    }
}

#[test]
/// Production boundary: internal planning work remains board state and does
/// not produce a user-facing question or Manager debt. A routing unit test
/// cannot prove the UI and Manager agree; local Git fixture setup takes a few
/// seconds and needs no Docker or network.
fn internal_planning_attention_stays_on_the_board_without_asking_the_user() {
    let _environment = crate::test_support::lock_process_environment();
    let mut app = fixture();
    let key = "task-generation:fixture";
    let mut work = crate::core::planning_work::Work::new(
        key.into(),
        "Generate task stories".into(),
        "Prepare implementation stories for the approved feature.".into(),
        "The saved stories need a correction before generation can resume.".into(),
    );
    work.kind = crate::core::planning_work::WorkKind::TaskGeneration;
    work.status = crate::core::planning_work::WorkStatus::NeedsAttention;
    if let Screen::Connected(project) = &mut app.screen {
        project.planning_work.push(work);
    }
    let ctx = egui::Context::default();
    app.tick(0.016, &ctx);
    frame(&mut app, &ctx, vec![]);
    frame(&mut app, &ctx, vec![]);
    assert!(
        ctx.data_mut(|data| data.get_temp::<String>(egui::Id::new("koolade_selected_planning")))
            .is_none(),
        "internal task state must not create a human answer obligation"
    );
    if let Screen::Connected(project) = &app.screen {
        assert!(
            !project.task_chats.messages.contains_key(key),
            "agent-owned work must not receive a synthetic user question"
        );
        let prompt = crate::app::manager::Manager::prompt_body(project, &[]);
        assert!(prompt.contains("Internal planning work"));
        assert!(prompt.contains("status alone does not mean the operator owes an answer"));
        let waiting_on_user = prompt
            .lines()
            .find(|line| line.starts_with("Waiting on user: "))
            .expect("manager prompt includes the user action queue");
        assert!(!waiting_on_user.contains("Generate task stories"));
    }
}
