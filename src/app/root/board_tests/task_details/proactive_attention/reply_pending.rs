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
    peer.bind_conversation_identities(&[(key.clone(), uid)])
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
/// Production boundary: an unmatched conversation identity cannot suppress
/// the current item's prompt. UI focus and persistence need local fixtures,
/// but no Docker or network.
fn unmatched_conversation_does_not_suppress_the_current_prompt() {
    let _environment = crate::test_support::lock_process_environment();
    let mut app = fixture();
    let item = crate::domain::OpenItem::new(
        "CLR-033".into(),
        crate::domain::Priority::Blocking,
        crate::domain::ItemKind::Question,
        "General".into(),
        "Which branch should receive this task?".into(),
        "The task needs a destination.".into(),
    );
    let unmatched_key = "unmatched-conversation-identity";
    let slug = if let Screen::Connected(project) = &mut app.screen {
        project.state.items.push(item);
        project.chat_slug.clone()
    } else {
        panic!("fixture remains connected")
    };
    let mut unmatched = crate::persistence::task_chats::TaskChats::default();
    unmatched.ensure_loaded(&slug);
    unmatched
        .append(
            &slug,
            unmatched_key,
            vec![crate::domain::chatlog::ChatMessage {
                id: "unmatched-agent-message".into(),
                role: crate::domain::chatlog::ChatRole::Agent,
                text: "An unrelated conversation message".into(),
                ref_item: Some(unmatched_key.into()),
                lease_renewed_at: None,
                answer_context: None,
                clarification: false,
                specification_context: None,
                baseline_review_id: None,
                verified_repository_evidence: Vec::new(),
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
        "an unmatched conversation must not claim the current item's identity"
    );
    if let Screen::Connected(project) = &app.screen {
        assert!(
            project.task_chats.messages["CLR-033"]
                .iter()
                .any(|message| message.id.starts_with("needs-attention:clarification:"))
        );
    }
}

#[test]
/// Production boundary: a conversation attached to an earlier item identity
/// cannot suppress a prompt for a new item that reuses the same display ID.
/// The UI integration test uses persisted history and attention focus with
/// local Git only.
fn prior_item_conversation_does_not_hide_question_for_reused_display_id() {
    let _environment = crate::test_support::lock_process_environment();
    let mut app = fixture();
    let old_item = crate::domain::OpenItem::new(
        "CLR-036".into(),
        crate::domain::Priority::Blocking,
        crate::domain::ItemKind::Question,
        "General".into(),
        "Which branch should receive this task?".into(),
        "The task needs a destination.".into(),
    );
    let old_key = old_item.conversation_key().to_owned();
    let current_item = crate::domain::OpenItem::new(
        "CLR-036".into(),
        crate::domain::Priority::Blocking,
        crate::domain::ItemKind::Question,
        "General".into(),
        "Which branch should receive this task?".into(),
        "The task needs a destination.".into(),
    );
    assert_ne!(old_item.uid, current_item.uid);
    let slug = if let Screen::Connected(project) = &mut app.screen {
        project.state.items.push(current_item);
        project.chat_slug.clone()
    } else {
        panic!("fixture remains connected")
    };
    let mut prior_conversation = crate::persistence::task_chats::TaskChats::default();
    prior_conversation.ensure_loaded(&slug);
    prior_conversation
        .append(
            &slug,
            &old_key,
            vec![crate::domain::chatlog::ChatMessage {
                id: "prior-item-agent-message".into(),
                role: crate::domain::chatlog::ChatRole::Agent,
                text: "Question previously presented for another item".into(),
                ref_item: Some(old_key.clone()),
                lease_renewed_at: None,
                answer_context: None,
                clarification: false,
                specification_context: None,
                baseline_review_id: None,
                verified_repository_evidence: Vec::new(),
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
        "a prior item's conversation cannot suppress the new item's prompt"
    );
    if let Screen::Connected(project) = &app.screen {
        assert!(
            project.task_chats.messages["CLR-036"]
                .iter()
                .any(|message| message.id.starts_with("needs-attention:clarification:"))
        );
    }
}
