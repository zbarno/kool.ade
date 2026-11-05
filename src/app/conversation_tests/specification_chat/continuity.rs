use super::*;
use sha2::Digest;

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
    assert!(!follow_up_prompt.contains("docs/koolade/features/synthetic-other-feature.md"));
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

/// Production boundary: a feature chat request crosses UI command dispatch and
/// TurnController after the selected feature changes. A core turn test cannot
/// prove the app captured the feature UID before that UI change. The test uses
/// two committed synthetic docs in a temporary Git fixture; no network, Docker,
/// or SSH is required (low setup cost).
#[test]
fn in_flight_feature_question_keeps_its_captured_uid_after_selection_changes() {
    let mut app = fixture();
    let (first_display, first_uid, second_display, first_source_sha256) =
        publish_two_features(&mut app);
    let (request, first_pin) = {
        let Screen::Connected(project) = &app.screen else {
            panic!("fixture remains connected")
        };
        let target = crate::core::specification_chat::capture_target(
            &project.state,
            crate::core::specification_chat::Scope::Feature {
                display_id: first_display.clone(),
                uid: first_uid.clone(),
            },
        )
        .unwrap();
        let pin = target.published_pin.commit_sha.clone();
        (
            crate::core::specification_chat::Request {
                text: "  Why does this feature show the renewal date?  ".into(),
                intent: crate::core::specification_chat::Intent::Ask,
                target,
                omitted_history_messages: 0,
            },
            pin,
        )
    };
    let (entered_tx, entered_rx) = mpsc::channel();
    let release = Arc::new((Mutex::new(false), Condvar::new()));
    let prompt = Arc::new(Mutex::new(None));
    app.task_harness = Some(Box::new(GatedSpecificationHarness {
        entered: entered_tx,
        release: release.clone(),
        prompt: prompt.clone(),
        reply: "The feature requires displaying the renewal date.".into(),
    }));
    app.dispatch_ui_command(crate::ui::ApplicationCommand::SpecificationChat(
        request.clone(),
    ));
    entered_rx
        .recv_timeout(Duration::from_secs(20))
        .expect("the feature Specification command should reach its harness");

    let ctx = egui::Context::default();
    ctx.data_mut(|data| data.insert_temp(egui::Id::new("koolade_document_tab"), false));
    frame(&mut app, &ctx, vec![]);
    click_text(&mut app, &ctx, "Features  2");
    click_text(&mut app, &ctx, "F101");
    click_text(&mut app, &ctx, "F102: Billing address");
    let selected = ctx.data_mut(|data| {
        data.get_temp::<Option<String>>(egui::Id::new("koolade_selected_feature"))
            .flatten()
    });
    assert_eq!(selected.as_deref(), Some(second_display.as_str()));
    {
        let (lock, wake) = &*release;
        *lock.lock().unwrap() = true;
        wake.notify_all();
    }
    complete(&mut app);

    let captured = prompt.lock().unwrap().clone().unwrap();
    assert!(captured.contains(&request.text));
    assert!(captured.contains("Scope: Feature"));
    assert!(captured.contains(&first_pin));
    assert!(captured.contains(&format!("docs/koolade/features/{first_uid}.md")));
    assert!(captured.contains(&first_source_sha256));

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
        "The feature requires displaying the renewal date."
    );
    let context = answer.specification_context.as_ref().unwrap();
    assert_eq!(context.scope_key, format!("feature:{first_uid}"));
    assert_eq!(context.published_commit_sha, first_pin);
}

fn publish_two_features(app: &mut KooladeApp) -> (String, String, String, String) {
    let Screen::Connected(project) = &mut app.screen else {
        panic!("fixture remains connected")
    };
    let mut identities = Vec::new();
    for (display_id, title, requirement) in [
        ("F101", "Renewal date", "Display the renewal date."),
        (
            "F102",
            "Billing address",
            "Keep the billing address current.",
        ),
    ] {
        let source = format!(
            "# {display_id}: {title}\n\n**Status:** Ready\n\n## Intent\n\n{title}.\n\n## Current Behavior\n\nCurrent behavior is documented.\n\n## Desired Behavior\n\n{requirement}\n\n## Scope\n\nThis feature only.\n\n## Affected Product Areas\n\n`product:overview`\n\n## Requirements\n\n- {display_id}-R1 (MUST). {requirement}\n\n## Decisions and Assumptions\n\nNone.\n\n## Acceptance Criteria\n\n1. {requirement}\n"
        );
        let identified =
            crate::domain::ArtifactIdentity::preserve_markdown(&source, None, display_id, title)
                .unwrap();
        let identity = crate::domain::ArtifactIdentity::from_markdown(&identified)
            .unwrap()
            .unwrap();
        let markdown = crate::domain::ChangeMetadata::write_markdown(
            &identified,
            &identity,
            crate::domain::ChangeStatus::Ready,
        )
        .unwrap();
        let path = format!("docs/koolade/features/{}.md", identity.uid);
        let full_path = project.state.repo_root.join(&path);
        std::fs::create_dir_all(full_path.parent().unwrap()).unwrap();
        std::fs::write(full_path, &markdown).unwrap();
        let output = std::process::Command::new("git")
            .args(["add", &path])
            .current_dir(&project.state.repo_root)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let source_sha256 = format!("{:x}", sha2::Sha256::digest(markdown.as_bytes()));
        identities.push((identity.display_id, identity.uid, source_sha256));
    }
    let output = std::process::Command::new("git")
        .args([
            "-c",
            "user.name=Kool.ad/e Test",
            "-c",
            "user.email=koolade-test@example.invalid",
            "commit",
            "-m",
            "add synthetic specification features",
        ])
        .current_dir(&project.state.repo_root)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    project.state.resync().unwrap();
    let [
        (first_display, first_uid, first_source_sha256),
        (second_display, _, _),
    ] = identities.as_slice()
    else {
        unreachable!("two feature fixtures were created")
    };
    (
        first_display.clone(),
        first_uid.clone(),
        second_display.clone(),
        first_source_sha256.clone(),
    )
}
