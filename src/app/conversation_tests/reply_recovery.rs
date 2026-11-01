use super::*;

/// Production boundary: an answer persisted before a process interruption is
/// claimed after its lease expires and resumes in its original task stream.
/// A store unit test cannot prove app startup dispatch and planning adoption
/// together. Uses temporary local planning files; no Docker or network; low
/// setup cost.
#[test]
fn expired_saved_reply_is_resumed_after_restart_without_asking_again() {
    let root = std::env::temp_dir().join(format!(
        "koolade_reply_recovery_{}-{}",
        std::process::id(),
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&root).unwrap();
    init_git(&root);
    let key;
    let slug = root.join("runtime").to_string_lossy().into_owned();
    let mut interrupted = fixture();
    let user_id;
    if let Screen::Connected(project) = &mut interrupted.screen {
        project.state = crate::core::state::PlannerState::load(&root).unwrap();
        project.state.bootstrap_missing().unwrap();
        project.chat_slug = slug.clone();
        let mut work = crate::core::planning_work::Work::new(
            String::new(),
            "Answer the storage question".into(),
            "Why does this project use SQLite?".into(),
            "Building project context before planning".into(),
        );
        work.kind = crate::core::planning_work::WorkKind::Question;
        work.key = format!("task:{}", work.uid);
        key = work.key.clone();
        project.planning_work.push(work);
        project.save_planning_work().unwrap();
        let user = ChatMessage::new(
            ChatRole::User,
            "The local transactional store is required.",
            Some(key.clone()),
        );
        user_id = user.id.clone();
        let mut expired_claim = ChatMessage::new(
            ChatRole::System,
            "Kool.ad/e is processing this saved task reply internally.",
            Some(key.clone()),
        );
        expired_claim.id = format!("internal-task-reply-claim:{user_id}:expired");
        expired_claim.ts = chrono::Utc::now() - chrono::Duration::minutes(16);
        project
            .task_chats
            .append(&slug, &key, vec![user, expired_claim])
            .unwrap();
    } else {
        panic!("project remains connected");
    }
    drop(interrupted);

    let mut restarted = fixture();
    if let Screen::Connected(project) = &mut restarted.screen {
        project.state = crate::core::state::PlannerState::load(&root).unwrap();
        project.planning_work =
            crate::core::planning_work::load(&project.state.planning_store).unwrap();
        project.chat_slug = slug.clone();
    }
    restarted.task_harness = Some(Box::new(ReplyHarness {
        prompts: Arc::new(Mutex::new(Vec::new())),
        reply: serde_json::json!({
            "schema_version": 2,
            "assistant_message": "SQLite keeps local project state transactional.",
            "document_updates": [],
            "open_items_added": [],
            "open_items_updated": [],
            "open_items_resolved": [],
            "next_question_id": null,
            "requested_action": null
        })
        .to_string(),
    }));

    restarted.retry_pending_task_reply();
    let Screen::Connected(project) = &restarted.screen else {
        panic!("project remains connected");
    };
    assert!(project.task_turns.contains_key(&key));
    assert_eq!(
        project
            .task_turn_user_message_ids
            .get(&key)
            .map(String::as_str),
        Some(user_id.as_str())
    );
    complete(&mut restarted);
    let Screen::Connected(project) = &restarted.screen else {
        panic!("project remains connected");
    };
    assert_eq!(
        project
            .planning_work
            .iter()
            .find(|work| work.key == key)
            .unwrap()
            .status,
        crate::core::planning_work::WorkStatus::Done
    );
    assert!(!project.task_chats.has_pending_user_reply(&key));
    assert!(project.task_chats.messages[&key].iter().any(|message| {
        message.role == ChatRole::Agent && message.text.contains("SQLite keeps local")
    }));
    std::fs::remove_dir_all(root).unwrap();
}
