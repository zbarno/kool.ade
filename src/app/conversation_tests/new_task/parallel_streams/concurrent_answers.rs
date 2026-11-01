use super::*;

/// Production boundary: overlapping human task answers share a planning
/// revision, so one stale apply must retry inside its own conversation. A unit
/// test cannot cover independent app turns, shared-store contention, and
/// adoption. Uses a temporary local Git repository and synchronized fixture
/// harnesses; no Docker/network; setup takes a few seconds.
#[test]
fn concurrent_task_answers_apply_to_their_own_streams() {
    let root = std::env::temp_dir().join(format!(
        "koolade-parallel-success-{}-{}",
        std::process::id(),
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&root).unwrap();
    init_git(&root);
    let mut app = fixture();
    if let Screen::Connected(project) = &mut app.screen {
        let mut state = crate::core::state::PlannerState::load(&root).unwrap();
        state.bootstrap_missing().unwrap();
        let items = vec![
            crate::domain::OpenItem::new(
                "CLR-041".into(),
                crate::domain::Priority::High,
                crate::domain::ItemKind::Question,
                "Export".into(),
                None,
                "Which worker model should export use?".into(),
                "Export responsiveness depends on this choice.".into(),
            ),
            crate::domain::OpenItem::new(
                "CLR-042".into(),
                crate::domain::Priority::High,
                crate::domain::ItemKind::Question,
                "Login".into(),
                None,
                "Which session policy should login use?".into(),
                "Login security depends on this choice.".into(),
            ),
        ];
        let (changes, checks, _) =
            crate::artifacts::items_io::record_changes(&state.planning_store, &items, &[]).unwrap();
        let (_, revision) = state
            .planning_store
            .transaction_with_revision_and_record_revisions(
                &changes,
                Some(&state.baseline_planning_revision),
                &checks,
            )
            .unwrap();
        state.baseline_planning_revision = revision;
        project.state =
            crate::core::state::PlannerState::load_with_store(&root, &state.planning_store)
                .unwrap();
        project.bind_task_conversation_identities();
        project.refresh_git();
    }
    app.task_harness = Some(Box::new(StoppedHarness {
        wait_for_cancel: true,
    }));
    app.start_turn("Keep the main planning stream active");
    let main_key = match &app.screen {
        Screen::Connected(project) => project.active_planning_work.clone().unwrap(),
        _ => panic!("project remains connected"),
    };

    let tasks = [
        (
            "CLR-041",
            "Use a background worker to keep exports responsive.",
            "Use a background worker for export.",
            "Export uses a background worker to keep the interface responsive.",
        ),
        (
            "CLR-042",
            "Use a local cache while the login session remains valid.",
            "Use a local cache while the session is valid.",
            "Login uses a local cache while the session is valid.",
        ),
    ];
    let rendezvous = Arc::new((Mutex::new(0), std::sync::Condvar::new()));
    let mut keys = Vec::new();
    let mut captured = Vec::new();
    for (key, user_answer, assistant_answer, evidence) in tasks {
        let prompts = Arc::new(Mutex::new(Vec::new()));
        let reply = serde_json::json!({
            "schema_version": 1,
            "assistant_message": assistant_answer,
            "open_items_updated": [{"id": key, "evidence": evidence}]
        })
        .to_string();
        app.task_harness = Some(Box::new(SynchronizedReplyHarness {
            prompts: Arc::clone(&prompts),
            rendezvous: Arc::clone(&rendezvous),
            reply,
        }));
        if let Screen::Connected(project) = &mut app.screen {
            project
                .task_chats
                .drafts
                .insert(key.into(), user_answer.into());
        }
        app.submit_task_reply(key);
        let Screen::Connected(project) = &app.screen else {
            panic!("project remains connected");
        };
        assert!(project.task_turns.contains_key(key));
        keys.push(key.to_owned());
        captured.push((
            prompts,
            project
                .state
                .items
                .iter()
                .find(|item| item.id == key)
                .unwrap()
                .question
                .clone(),
            assistant_answer.to_owned(),
            evidence.to_owned(),
        ));
    }

    complete(&mut app);
    complete(&mut app);
    let mut prompt_counts = captured
        .iter()
        .map(|(prompts, _, _, _)| prompts.lock().unwrap().len())
        .collect::<Vec<_>>();
    prompt_counts.sort_unstable();
    assert_eq!(prompt_counts[0], 1);
    assert!(
        (2..=4).contains(&prompt_counts[1]),
        "one overlapping turn must refresh its planning context and retry internally, got {prompt_counts:?}"
    );

    let Screen::Connected(project) = &app.screen else {
        panic!("project remains connected");
    };
    assert!(project.active_turn.is_some());
    assert_eq!(
        project.active_planning_work.as_deref(),
        Some(main_key.as_str())
    );
    for (key, (prompts, question, answer, evidence)) in keys.iter().zip(captured.iter()) {
        for prompt in prompts.lock().unwrap().iter() {
            assert!(prompt.contains(question));
            for (_, other_question, _, _) in
                captured.iter().filter(|(_, other, _, _)| other != question)
            {
                assert!(!prompt.contains(other_question));
            }
        }
        let history = &project.task_chats.messages[key];
        assert!(
            history.iter().any(|message| {
                message.role == ChatRole::Agent && message.text == answer.as_str()
            })
        );
        assert!(history.iter().any(|message| {
            message.role == ChatRole::System
                && message
                    .id
                    .starts_with("internal-task-reply-outcome:applied:")
        }));
        assert!(!project.task_chats.has_pending_user_reply(key));
        assert!(
            project
                .state
                .items
                .iter()
                .find(|item| item.id == *key)
                .unwrap()
                .evidence
                .contains(evidence)
        );
    }
    assert!(app.chat_messages().is_empty());
    let main_turn = project.active_turn.as_ref().unwrap().clone();
    main_turn.request_cancel();
    complete(&mut app);
    std::fs::remove_dir_all(root).unwrap();
}
