use super::*;

/// Production adoption boundary: an outcome for an older saved task answer is
/// discarded after refreshing shared chat history. Turn-level locking tests
/// cannot prove stale model text is kept out of the task transcript. Uses the
/// local board fixture and temp chat store; no provider, Docker, SSH, or
/// network dependencies; low setup cost.
#[test]
fn newer_saved_task_reply_discards_an_older_turn_outcome() {
    let mut app = crate::app::root::board_tests::fixture();
    let key = "CLR-001";
    let old = ChatMessage::new(ChatRole::User, "Use the release branch", Some(key.into()));
    let old_id = old.id.clone();
    let current = ChatMessage::new(ChatRole::User, "Use the stable branch", Some(key.into()));
    if let Screen::Connected(project) = &mut app.screen {
        project
            .task_chats
            .append(&project.chat_slug, key, vec![old, current.clone()])
            .unwrap();
        project
            .task_turn_user_message_ids
            .insert(key.into(), old_id);
    } else {
        panic!("fixture must be connected");
    }

    app.apply_completed_turns(
        None,
        vec![(
            key.into(),
            "run".into(),
            "session".into(),
            TurnOutcome::Rejected {
                problems: vec!["stale result".into()],
                final_text: "The release branch was selected.".into(),
                elapsed: Duration::ZERO,
            },
        )],
    );

    let Screen::Connected(project) = &app.screen else {
        unreachable!()
    };
    let messages = &project.task_chats.messages[key];
    assert!(messages.iter().any(|message| message.id == current.id));
    assert!(
        !messages
            .iter()
            .any(|message| message.text == "The release branch was selected.")
    );
    assert!(
        project
            .activity
            .pending
            .iter()
            .any(|event| { event.contains("Discarded an older response") && event.contains(key) })
    );
}

/// A completed planning transaction is already durable and must be reconciled
/// even if a newer reply lands before UI adoption. The old model prose must not
/// appear as an answer to the newer reply. Uses only the local board fixture.
#[test]
fn newer_saved_task_reply_keeps_an_older_applied_outcome_without_stale_prose() {
    let mut app = crate::app::root::board_tests::fixture();
    let key = "planning:test-question";
    let old = ChatMessage::new(ChatRole::User, "Use the release branch", Some(key.into()));
    let old_id = old.id.clone();
    let current = ChatMessage::new(ChatRole::User, "Use the stable branch", Some(key.into()));
    let state = if let Screen::Connected(project) = &mut app.screen {
        project
            .task_chats
            .append(&project.chat_slug, key, vec![old, current.clone()])
            .unwrap();
        project
            .task_turn_user_message_ids
            .insert(key.into(), old_id);
        let mut work = crate::core::planning_work::Work::new(
            key.into(),
            "Test question".into(),
            "Resolve the current project question.".into(),
            "Original internal context.".into(),
        );
        work.kind = crate::core::planning_work::WorkKind::Question;
        work.status = crate::core::planning_work::WorkStatus::NeedsAttention;
        project.planning_work.push(work);
        project.state.clone()
    } else {
        panic!("fixture must be connected");
    };
    let normalized = crate::core::validation::NormalizedTurn {
        assistant_message: "The release branch was selected.".into(),
        change_summary: None,
        spec_markdown: None,
        document_updates: Vec::new(),
        planning_tasks: Vec::new(),
        change_status_updates: Default::default(),
        additional_planning_artifacts: Vec::new(),
        added: Vec::new(),
        updates: Vec::new(),
        resolved: Vec::new(),
        next_question_id: None,
        requested_action: None,
        follow_up_task: None,
        warnings: Vec::new(),
        workflow: None,
        task_batch: None,
        plan_comparison: None,
    };

    app.apply_completed_turns(
        None,
        vec![(
            key.into(),
            "run".into(),
            "session".into(),
            TurnOutcome::Applied {
                state: Box::new(state),
                receipt: crate::core::apply::ApplyReceipt {
                    spec_written: false,
                    items_written: false,
                    commit_message: "planner: apply earlier saved reply".into(),
                    repo_relative_paths: Vec::new(),
                    synthesized_open_items: Vec::new(),
                },
                normalized: Box::new(normalized),
                commit_result: Ok("abcdef0123456789".into()),
                elapsed: Duration::ZERO,
                stderr_tail: String::new(),
            },
        )],
    );

    let Screen::Connected(project) = &app.screen else {
        unreachable!()
    };
    let messages = &project.task_chats.messages[key];
    assert!(messages.iter().any(|message| message.id == current.id));
    assert!(messages.iter().any(|message| {
        message.text.contains("earlier saved reply was applied")
            && message.text.contains("newer reply remains saved")
    }));
    assert!(
        !messages
            .iter()
            .any(|message| message.text == "The release branch was selected.")
    );
    let work = project
        .planning_work
        .iter()
        .find(|work| work.key == key)
        .unwrap();
    assert!(
        work.detail
            .contains("newer reply remains saved for internal processing")
    );
    assert!(!work.detail.contains("The release branch was selected."));
    assert!(
        !project
            .activity
            .pending
            .iter()
            .any(|event| event.contains("Discarded an older response"))
    );
}

/// Production task-chat adoption must preserve the clarification-only marker
/// so it cannot suppress an older saved-answer retry. A retry unit test cannot
/// prove the marker survives TurnOutcome adoption. Uses the local board fixture
/// only, with no provider, Docker, SSH, or network dependencies.
#[test]
fn read_only_task_discussion_does_not_complete_a_saved_reply_retry() {
    let mut app = crate::app::root::board_tests::fixture();
    let key = "CLR-001";
    let user = ChatMessage::new(ChatRole::User, "Use release branch", Some(key.into()));
    let user_id = user.id.clone();
    let mut retry = ChatMessage::new(ChatRole::System, "Retry internally", None);
    crate::persistence::task_chats::TaskChats::mark_planning_conflict_retry(&mut retry);
    let mut clarification = ChatMessage::new(ChatRole::User, "What does release mean?", None);
    clarification.clarification = true;
    let Screen::Connected(mut project) = std::mem::replace(&mut app.screen, Screen::Welcome) else {
        unreachable!()
    };
    project
        .task_chats
        .append(
            &project.chat_slug,
            key,
            vec![user.clone(), retry, clarification],
        )
        .unwrap();
    project
        .task_turn_user_message_ids
        .insert(key.into(), user_id);
    let outcome = TurnOutcome::Discussion {
        final_text: "Release means the stable branch.".into(),
        clarification: true,
        elapsed: Duration::ZERO,
    };

    app.adopt_turn(&mut project, outcome, Some(key), None, false);
    let messages = &project.task_chats.messages[key];
    assert!(messages.iter().any(|message| {
        message.role == ChatRole::Agent
            && message.clarification
            && message.text == "Release means the stable branch."
    }));
    assert!(project.task_chats.has_pending_user_reply(key));
    assert_eq!(
        project.task_chats.pending_planning_conflict_retries(key),
        Some(1)
    );
}
