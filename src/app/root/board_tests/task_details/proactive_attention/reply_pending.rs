use super::*;

#[path = "reply_pending/failed_answer.rs"]
mod failed_answer;

#[test]
/// Production boundary: proactive attention checks the latest shared task
/// history before presenting a human question. A unit test cannot prove the
/// peer-window refresh and UI focus together; the local Git fixture takes a
/// few seconds and needs no Docker or network.
fn a_saved_peer_window_reply_is_not_asked_again() {
    let _environment = crate::test_support::lock_process_environment();
    let mut app = fixture();
    let item = crate::domain::OpenItem::new(
        "CLR-034".into(),
        crate::domain::Priority::Blocking,
        crate::domain::ItemKind::Question,
        "General".into(),
        None,
        "Which branch should receive this task?".into(),
        "The task needs a destination.".into(),
    );
    let uid = item.uid.clone().unwrap();
    let key = item.conversation_key().to_owned();
    let slug = if let Screen::Connected(project) = &mut app.screen {
        project.state.items.push(item);
        project.chat_slug.clone()
    } else {
        panic!("fixture remains connected")
    };

    let mut peer = crate::persistence::task_chats::TaskChats::default();
    peer.bind_conversation_identities(&[(key.clone(), uid)], &[])
        .unwrap();
    peer.ensure_loaded(&slug);
    peer.append(
        &slug,
        &key,
        vec![crate::domain::chatlog::ChatMessage::new(
            crate::domain::chatlog::ChatRole::User,
            "Use release/2.1.",
            Some(key.clone()),
        )],
    )
    .unwrap();

    let ctx = egui::Context::default();
    app.tick(0.016, &ctx);
    frame(&mut app, &ctx, vec![]);
    frame(&mut app, &ctx, vec![]);
    assert!(
        ctx.data_mut(|data| data.get_temp::<String>(egui::Id::new("koolade_selected_planning")))
            .is_none(),
        "a question with a saved user reply must not be surfaced again"
    );
    if let Screen::Connected(project) = &app.screen {
        let messages = &project.task_chats.messages[&key];
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].role, crate::domain::chatlog::ChatRole::User);
        assert_eq!(messages[0].text, "Use release/2.1.");
    }
}

#[test]
/// Production boundary: a legacy clarification is detached on upgrade, then
/// the current UID receives its own prompt record. UI focus and persistence
/// need local Git fixtures but no Docker or network.
fn legacy_clarification_history_is_detached_and_current_uid_is_prompted() {
    let _environment = crate::test_support::lock_process_environment();
    let mut app = fixture();
    let item = crate::domain::OpenItem::new(
        "CLR-033".into(),
        crate::domain::Priority::Blocking,
        crate::domain::ItemKind::Question,
        "General".into(),
        None,
        "Which branch should receive this task?".into(),
        "The task needs a destination.".into(),
    );
    let legacy_prompt_id = legacy_clarification_prompt_id(&item);
    let key = item.conversation_key().to_owned();
    let slug = if let Screen::Connected(project) = &mut app.screen {
        project.state.items.push(item);
        project.chat_slug.clone()
    } else {
        panic!("fixture remains connected")
    };
    // Simulate an older version that saved this prompt under its display key
    // without a board artifact UID.
    let mut legacy = crate::persistence::task_chats::TaskChats::default();
    legacy.ensure_loaded(&slug);
    legacy
        .append(
            &slug,
            &key,
            vec![crate::domain::chatlog::ChatMessage {
                id: legacy_prompt_id,
                role: crate::domain::chatlog::ChatRole::Agent,
                text: "Previously presented clarification".into(),
                ref_item: Some(key.clone()),
                lease_renewed_at: None,
                answer_context: None,
                clarification: false,
                ts: chrono::Utc::now(),
            }],
        )
        .unwrap();
    let ctx = egui::Context::default();
    app.tick(0.016, &ctx);
    frame(&mut app, &ctx, vec![]);
    frame(&mut app, &ctx, vec![]);
    assert_eq!(
        ctx.data_mut(|data| data.get_temp::<String>(egui::Id::new("koolade_selected_planning")))
            .as_deref(),
        Some("CLR-033"),
        "a detached legacy prompt must not claim an unknown current identity"
    );
    if let Screen::Connected(project) = &app.screen {
        assert_eq!(project.task_chats.messages["@legacy:CLR-033"].len(), 1);
        assert!(
            project.task_chats.messages["CLR-033"]
                .iter()
                .any(|message| message.id.starts_with("needs-attention:clarification:"))
        );
    }
}

#[test]
/// Production boundary: a prompt detached from a reused CLR display ID must
/// not suppress a new identity even when the question copy is identical. The
/// UI integration test uses persisted history and attention focus with local
/// Git only.
fn detached_legacy_prompt_for_reused_clr_id_does_not_hide_new_question() {
    let _environment = crate::test_support::lock_process_environment();
    let mut app = fixture();
    let old_item = crate::domain::OpenItem::new(
        "CLR-036".into(),
        crate::domain::Priority::Blocking,
        crate::domain::ItemKind::Question,
        "General".into(),
        None,
        "Which branch should receive this task?".into(),
        "The task needs a destination.".into(),
    );
    let legacy_prompt_id = legacy_clarification_prompt_id(&old_item);
    let current_item = crate::domain::OpenItem::new(
        "CLR-036".into(),
        crate::domain::Priority::Blocking,
        crate::domain::ItemKind::Question,
        "General".into(),
        None,
        "Which branch should receive this task?".into(),
        "The task needs a destination.".into(),
    );
    assert_ne!(old_item.uid, current_item.uid);
    let key = current_item.conversation_key().to_owned();
    let slug = if let Screen::Connected(project) = &mut app.screen {
        project.state.items.push(current_item);
        project.chat_slug.clone()
    } else {
        panic!("fixture remains connected")
    };
    let mut legacy = crate::persistence::task_chats::TaskChats::default();
    legacy.ensure_loaded(&slug);
    legacy
        .append(
            &slug,
            &key,
            vec![crate::domain::chatlog::ChatMessage {
                id: legacy_prompt_id,
                role: crate::domain::chatlog::ChatRole::Agent,
                text: "Previously presented question for the original item".into(),
                ref_item: Some(key.clone()),
                lease_renewed_at: None,
                answer_context: None,
                clarification: false,
                ts: chrono::Utc::now(),
            }],
        )
        .unwrap();

    let ctx = egui::Context::default();
    app.tick(0.016, &ctx);
    frame(&mut app, &ctx, vec![]);
    frame(&mut app, &ctx, vec![]);
    assert_eq!(
        ctx.data_mut(|data| data.get_temp::<String>(egui::Id::new("koolade_selected_planning")))
            .as_deref(),
        Some("CLR-036"),
        "a detached prompt cannot establish that the new CLR identity was already handled"
    );
}
