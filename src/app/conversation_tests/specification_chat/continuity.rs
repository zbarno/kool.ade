use super::*;

/// Production boundary: command dispatch persists a Specification turn, then
/// the next turn retrieves bounded history into its model prompt. This proves the
/// stored scope metadata actually controls prompt retrieval, which the history
/// helper's unit test cannot establish. The fixture uses temporary Git and
/// local chat storage only; no provider, Docker, SSH, or network is needed
/// (low setup cost).
#[test]
fn specification_follow_up_uses_same_scope_history_and_excludes_other_streams() {
    let mut app = fixture();
    let first_text = "Which product requirement describes renewal timing?";
    let first_request = product_request(&app, first_text);
    let first_pin = first_request.target.published_pin.commit_sha.clone();
    let prompts = Arc::new(Mutex::new(Vec::new()));
    app.task_harness = Some(Box::new(ReplyHarness {
        prompts: prompts.clone(),
        reply: "The published requirement says renewal timing is explicit.".into(),
    }));
    app.dispatch_ui_command(crate::ui::ApplicationCommand::SpecificationChat(
        first_request,
    ));
    complete(&mut app);

    let Screen::Connected(project) = &mut app.screen else {
        panic!("fixture remains connected")
    };
    let mut other_feature = crate::domain::ChatMessage::new(
        crate::domain::ChatRole::Agent,
        "OTHER_FEATURE_CHAT_MARKER",
        None,
    );
    other_feature.specification_context = Some(crate::domain::chatlog::SpecificationChatContext {
        scope_key: "feature:synthetic-other-feature".into(),
        intent: "ask".into(),
        published_reference: "refs/heads/main".into(),
        published_commit_sha: "a".repeat(40),
        source_documents: vec![crate::domain::chatlog::SpecificationSourceDocument {
            path: "docs/koolade/features/synthetic-other-feature.md".into(),
            sha256: "b".repeat(64),
        }],
        effective_document_sha256: "c".repeat(64),
    });
    project.remember_chat(vec![
        crate::domain::ChatMessage::new(
            crate::domain::ChatRole::System,
            "MANAGER_PATROL_CHAT_MARKER",
            None,
        ),
        other_feature,
        crate::domain::ChatMessage::new(
            crate::domain::ChatRole::Agent,
            "UNATTRIBUTED_MAIN_CHAT_MARKER",
            None,
        ),
    ]);
    project.task_chats.messages.insert(
        "task:synthetic-task".into(),
        vec![crate::domain::ChatMessage::new(
            crate::domain::ChatRole::Agent,
            "PRIVATE_TASK_CHAT_MARKER",
            None,
        )],
    );

    let follow_up = "Why is that the chosen behavior?";
    let request = product_request(&app, follow_up);
    assert_eq!(request.target.published_pin.commit_sha, first_pin);
    app.task_harness = Some(Box::new(ReplyHarness {
        prompts: prompts.clone(),
        reply: "The source ties the timing to renewal notices.".into(),
    }));
    app.dispatch_ui_command(crate::ui::ApplicationCommand::SpecificationChat(
        request.clone(),
    ));
    complete(&mut app);

    let prompts = prompts.lock().unwrap();
    assert_eq!(prompts.len(), 2);
    let follow_up_prompt = &prompts[1];
    assert!(follow_up_prompt.contains(first_text));
    assert!(
        follow_up_prompt.contains("The published requirement says renewal timing is explicit.")
    );
    assert!(follow_up_prompt.contains("Assistant [scope product; intent ask;"));
    assert!(follow_up_prompt.contains(&first_pin));
    assert!(follow_up_prompt.contains(follow_up));
    assert!(!follow_up_prompt.contains("OTHER_FEATURE_CHAT_MARKER"));
    assert!(!follow_up_prompt.contains(&"a".repeat(40)));
    assert!(!follow_up_prompt.contains(&"b".repeat(64)));
    assert!(
        !follow_up_prompt.contains("docs/koolade/features/synthetic-other-feature.md")
    );
    assert!(!follow_up_prompt.contains("MANAGER_PATROL_CHAT_MARKER"));
    assert!(!follow_up_prompt.contains("UNATTRIBUTED_MAIN_CHAT_MARKER"));
    assert!(!follow_up_prompt.contains("PRIVATE_TASK_CHAT_MARKER"));
    let Screen::Connected(project) = &app.screen else {
        panic!("fixture remains connected")
    };
    let answer = project
        .chat
        .iter()
        .rev()
        .find(|message| message.role == crate::domain::ChatRole::Agent)
        .unwrap();
    assert_eq!(
        answer.text,
        "The source ties the timing to renewal notices."
    );
    assert_eq!(
        answer
            .specification_context
            .as_ref()
            .unwrap()
            .published_commit_sha,
        request.target.published_pin.commit_sha
    );
}
