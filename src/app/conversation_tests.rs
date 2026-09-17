use super::board_tests::{click_text, fixture, frame, text_position};
use super::*;

#[test]
fn task_story_discussion_updates_board_without_overriding_review_or_done() {
    let mut app = fixture();
    if let Screen::Connected(p) = &mut app.screen {
        for doc in &p.task_documents {
            p.task_chats.messages.insert(
                doc.path.clone(),
                vec![
                    ChatMessage::new(ChatRole::User, "Use SSO", None),
                    ChatMessage::new(ChatRole::Agent, "Recorded.", None),
                ],
            );
        }
    }
    let ctx = egui::Context::default();
    frame(&mut app, &ctx, vec![]);
    let output = frame(&mut app, &ctx, vec![]);
    for label in ["To do · 0", "In progress · 1", "In review · 1", "Done · 1"] {
        assert!(text_position(&output, label).is_some(), "missing {label}");
    }
    if let Screen::Connected(p) = &mut app.screen {
        p.task_chats
            .messages
            .get_mut(&p.task_documents[0].path)
            .unwrap()
            .push(ChatMessage::new(
                ChatRole::System,
                "Planning stopped: cancelled",
                None,
            ));
    }
    let output = frame(&mut app, &ctx, vec![]);
    assert!(text_position(&output, "In progress · 0").is_some());
    assert!(text_position(&output, "Needs attention · 1").is_some());
}

#[test]
fn next_step_is_prominent_and_older_messages_are_disclosed_on_request() {
    let mut app = fixture();
    if let Screen::Connected(p) = &mut app.screen {
        p.task_documents.clear();
        p.state.items = vec![OpenItem::new(
            "CLR-001".into(),
            crate::domain::Priority::High,
            crate::domain::ItemKind::Question,
            "General".into(),
            None,
            "Which provider?".into(),
            "Access".into(),
        )];
        p.task_chats.messages.insert(
            "CLR-001".into(),
            vec![
                ChatMessage::new(ChatRole::User, "Earlier context for the provider", None),
                ChatMessage::new(ChatRole::Agent, "Which authentication method?", None),
                ChatMessage::new(ChatRole::User, "Use corporate SSO", None),
                ChatMessage::new(
                    ChatRole::Agent,
                    "SSO is recorded.\nYour next step: Should guests use SSO too?",
                    None,
                ),
            ],
        );
    }
    let ctx = egui::Context::default();
    for theme in [egui::Theme::Dark, egui::Theme::Light] {
        ctx.style_mut_of(theme, |style| style.animation_time = 0.0);
    }
    frame(&mut app, &ctx, vec![]);
    let output = frame(&mut app, &ctx, vec![]);
    assert!(text_position(&output, "Your answer needed").is_some());
    assert!(text_position(&output, "Should guests use SSO too?").is_some());
    assert!(text_position(&output, "Send answer").is_some());
    assert!(
        text_position(&output, "Send answer").unwrap().y
            < text_position(&output, "General").unwrap().y,
        "The reply action belongs above metadata and activity"
    );
    assert!(text_position(&output, "Earlier context for the provider").is_none());
    click_text(&mut app, &ctx, "Which provider?");
    let output = frame(&mut app, &ctx, vec![]);
    assert!(text_position(&output, "Earlier context for the provider").is_none());
    assert!(text_position(&output, "Use corporate SSO").is_some());
    click_last(&mut app, &ctx, "Conversation history (4)");
    let output = frame(&mut app, &ctx, vec![]);
    assert!(text_position(&output, "Earlier context for the provider").is_some());
}

#[test]
fn saved_json_replies_render_as_readable_text_with_optional_diagnostics() {
    let mut app = fixture();
    let raw =
        r#"{"schema_version":1,"assistant_message":"Use corporate SSO.","open_items_updated":[]}"#;
    if let Screen::Connected(p) = &mut app.screen {
        p.task_documents.clear();
        p.state.items = vec![OpenItem::new(
            "CLR-001".into(),
            crate::domain::Priority::High,
            crate::domain::ItemKind::Question,
            "General".into(),
            None,
            "Which provider?".into(),
            "Access".into(),
        )];
        p.task_chats.messages.insert(
            "CLR-001".into(),
            vec![
                ChatMessage::new(
                    ChatRole::System,
                    "⚠ Turn rejected — nothing was written.\nInvalid update",
                    None,
                ),
                ChatMessage::new(ChatRole::Agent, raw, None),
            ],
        );
    }
    let ctx = egui::Context::default();
    for theme in [egui::Theme::Dark, egui::Theme::Light] {
        ctx.style_mut_of(theme, |style| style.animation_time = 0.0);
    }
    frame(&mut app, &ctx, vec![]);
    let output = frame(&mut app, &ctx, vec![]);
    assert!(text_position(&output, "Use corporate SSO.").is_some());
    assert!(text_position(&output, "Update not saved").is_some());
    click_text(&mut app, &ctx, "Which provider?");
    let output = frame(&mut app, &ctx, vec![]);
    assert!(text_position(&output, "Use corporate SSO.").is_some());
    assert!(
        text_position(
            &output,
            "Nothing changed. Retry your last reply or send a revised answer."
        )
        .is_some()
    );
    assert!(text_position(&output, raw).is_none());
    assert_eq!(app.task_messages("CLR-001")[1].text, raw);
    click_last(&mut app, &ctx, "Conversation history (2)");
    click_last(&mut app, &ctx, "Response details");
    let output = frame(&mut app, &ctx, vec![]);
    assert!(text_position(&output, raw).is_some());
}
use std::sync::{Arc, Mutex};

struct ReplyHarness {
    prompts: Arc<Mutex<Vec<String>>>,
    reply: String,
}

struct StoppedHarness {
    wait_for_cancel: bool,
}
impl crate::harness::AiHarness for StoppedHarness {
    fn label(&self) -> String {
        "stopped fixture".into()
    }
    fn check_available(&self) -> Result<String, crate::error::AppError> {
        Ok(self.label())
    }
    fn execute(
        &self,
        request: &crate::harness::PlanningRequest,
    ) -> Result<crate::harness::HarnessOutcome, crate::error::AppError> {
        if self.wait_for_cancel {
            while !request.cancel.load(std::sync::atomic::Ordering::SeqCst) {
                std::thread::sleep(Duration::from_millis(1));
            }
        }
        Err(crate::error::AppError::Other(
            if self.wait_for_cancel {
                "cancelled"
            } else {
                "provider unavailable"
            }
            .into(),
        ))
    }
}

#[test]
fn rejected_failed_and_cancelled_replies_stay_in_task_and_preserve_project_state() {
    for mode in ["rejected", "failed", "cancelled"] {
        let root = std::env::temp_dir().join(format!("packet_chat_{mode}_{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let mut app = fixture();
        if let Screen::Connected(p) = &mut app.screen {
            p.state = crate::core::state::PlannerState::load(&root).unwrap();
            p.state.bootstrap_missing().unwrap();
            p.state.items.push(OpenItem::new(
                "CLR-001".into(),
                crate::domain::Priority::High,
                crate::domain::ItemKind::Question,
                "General".into(),
                None,
                "Provider?".into(),
                "Access".into(),
            ));
            p.chat_slug = root.join("runtime").to_string_lossy().into_owned();
            p.chat = vec![ChatMessage::new(
                ChatRole::User,
                "Main chat unchanged",
                None,
            )];
        }
        let before = std::fs::read(root.join("planning/specification.md")).unwrap();
        app.task_harness = Some(if mode == "rejected" {
            Box::new(ReplyHarness {
                prompts: Default::default(),
                reply: serde_json::json!({
                    "schema_version":1, "assistant_message":"Starting an unrelated interview",
                    "interview":crate::core::workflow::InterviewBrief::default()
                })
                .to_string(),
            }) as Box<dyn crate::harness::AiHarness>
        } else {
            Box::new(StoppedHarness {
                wait_for_cancel: mode == "cancelled",
            })
        });
        *app.task_draft("CLR-001").unwrap() = "Use corporate SSO".into();
        app.submit_task_reply("CLR-001");
        if mode == "cancelled" {
            app.cancel_task_reply("CLR-002");
            let Screen::Connected(p) = &app.screen else {
                panic!()
            };
            assert!(!p.active_turn.as_ref().unwrap().cancel_requested());
            app.cancel_task_reply("CLR-001");
        }
        complete(&mut app);
        assert_eq!(app.chat_messages().len(), 1, "{mode}");
        assert_eq!(app.task_messages("CLR-001")[0].text, "Use corporate SSO");
        assert!(
            app.task_messages("CLR-001")
                .iter()
                .any(|m| m.role == ChatRole::System),
            "{mode}"
        );
        assert!(!app.task_chat_active("CLR-001"));
        assert_eq!(
            std::fs::read(root.join("planning/specification.md")).unwrap(),
            before
        );
        let Screen::Connected(p) = &mut app.screen else {
            panic!()
        };
        let expected = p.task_chats.messages["CLR-001"].clone();
        p.task_chats = Default::default();
        p.task_chats.ensure_loaded(&p.chat_slug);
        assert_eq!(p.task_chats.messages["CLR-001"], expected);
        std::fs::remove_dir_all(root).unwrap();
    }
}

impl crate::harness::AiHarness for ReplyHarness {
    fn label(&self) -> String {
        "task conversation fixture".into()
    }
    fn check_available(&self) -> Result<String, crate::error::AppError> {
        Ok(self.label())
    }
    fn execute(
        &self,
        request: &crate::harness::PlanningRequest,
    ) -> Result<crate::harness::HarnessOutcome, crate::error::AppError> {
        self.prompts
            .lock()
            .unwrap()
            .push(request.prompt_body.clone());
        Ok(crate::harness::HarnessOutcome {
            final_text: self.reply.clone(),
            envelope: None,
            stderr_tail: String::new(),
        })
    }
}

fn complete(app: &mut PacketApp) {
    let mut project = match std::mem::replace(&mut app.screen, Screen::Welcome) {
        Screen::Connected(project) => project,
        _ => panic!("not connected"),
    };
    let started = Instant::now();
    let outcome = loop {
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "focused turn did not finish"
        );
        match project
            .active_turn
            .as_ref()
            .expect("reply must start a turn")
            .poll(Duration::from_millis(20))
        {
            Some(TurnEvt::Done(outcome)) => break outcome,
            _ => {}
        }
    };
    app.adopt_turn(&mut project, outcome);
    app.screen = Screen::Connected(project);
}

fn click_last(app: &mut PacketApp, ctx: &egui::Context, label: &str) {
    let output = frame(app, ctx, vec![]);
    let pos = output
        .shapes
        .iter()
        .rev()
        .find_map(|shape| {
            if let egui::Shape::Text(text) = &shape.shape {
                if text.galley.text() == label {
                    return Some(text.pos + text.galley.mesh_bounds.center().to_vec2());
                }
            }
            None
        })
        .unwrap_or_else(|| panic!("missing {label}"));
    for pressed in [true, false] {
        frame(
            app,
            ctx,
            vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: Default::default(),
                },
            ],
        );
    }
}

#[test]
fn inline_and_modal_replies_share_history_and_keep_other_chats_out_of_prompts() {
    let root = std::env::temp_dir().join(format!("packet_conversation_ui_{}", std::process::id()));
    let repo = root.join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    for args in [
        vec!["init", "-q"],
        vec!["config", "user.name", "Fixture"],
        vec!["config", "user.email", "fixture@example.test"],
    ] {
        assert!(
            std::process::Command::new("git")
                .current_dir(&repo)
                .args(args)
                .status()
                .unwrap()
                .success()
        );
    }
    let mut app = fixture();
    let question = "Which authentication provider?";
    if let Screen::Connected(p) = &mut app.screen {
        p.state = crate::core::state::PlannerState::load(&repo).unwrap();
        p.state.bootstrap_missing().unwrap();
        p.state.items = vec![OpenItem::new(
            "CLR-001".into(),
            crate::domain::Priority::High,
            crate::domain::ItemKind::Question,
            "General".into(),
            Some("All".into()),
            question.into(),
            "Controls access".into(),
        )];
        p.task_documents.clear();
        p.chat_slug = root.join("runtime").to_string_lossy().into_owned();
        p.chat = vec![ChatMessage::new(
            ChatRole::User,
            "MAIN CHAT PRIVATE SENTINEL",
            None,
        )];
        p.task_chats
            .append(
                &p.chat_slug,
                "CLR-002",
                vec![ChatMessage::new(
                    ChatRole::User,
                    "OTHER TASK PRIVATE SENTINEL",
                    None,
                )],
            )
            .unwrap();
    }
    let prompts = Arc::new(Mutex::new(Vec::new()));
    app.task_harness = Some(Box::new(ReplyHarness { prompts: prompts.clone(), reply: serde_json::json!({
        "schema_version":1, "assistant_message":"Corporate SSO recorded. Any additional requirement?",
        "open_items_updated":[{"id":"CLR-001", "evidence":"Use corporate SSO"}]
    }).to_string() }));
    let ctx = egui::Context::default();
    frame(&mut app, &ctx, vec![]);
    click_text(&mut app, &ctx, "Your answer…");
    frame(
        &mut app,
        &ctx,
        vec![egui::Event::Text("Use corporate SSO".into())],
    );
    assert_eq!(app.task_draft("CLR-001").unwrap(), "Use corporate SSO");
    click_text(&mut app, &ctx, question);
    assert!(text_position(&frame(&mut app, &ctx, vec![]), "Use corporate SSO").is_some());
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
    frame(&mut app, &ctx, vec![]);
    click_text(&mut app, &ctx, "Send answer");
    assert_eq!(app.task_messages("CLR-001").len(), 1);
    complete(&mut app);
    assert_eq!(app.task_messages("CLR-001").len(), 2);
    assert_eq!(app.chat_messages().len(), 1);
    let output = frame(&mut app, &ctx, vec![]);
    assert!(text_position(&output, "In progress · 1").is_some());
    click_text(&mut app, &ctx, question);
    let output = frame(&mut app, &ctx, vec![]);
    assert!(text_position(&output, "Your answer needed").is_some());
    assert!(text_position(&output, "Use corporate SSO").is_some());
    click_last(&mut app, &ctx, "Your answer…");
    frame(
        &mut app,
        &ctx,
        vec![egui::Event::Text("Require MFA as well".into())],
    );
    assert_eq!(app.task_draft("CLR-001").unwrap(), "Require MFA as well");
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
    assert!(text_position(&frame(&mut app, &ctx, vec![]), "Require MFA as well").is_some());
    click_text(&mut app, &ctx, question);
    app.task_harness = Some(Box::new(ReplyHarness { prompts: prompts.clone(), reply: serde_json::json!({
        "schema_version":1, "assistant_message":"Corporate SSO with MFA is confirmed.",
        "updated_specification":crate::core::specification::fixture("Use corporate SSO with MFA."),
        "open_items_resolved":["CLR-001"]
    }).to_string() }));
    click_last(&mut app, &ctx, "Send answer");
    assert_eq!(app.task_messages("CLR-001").len(), 3);
    complete(&mut app);
    assert_eq!(app.task_messages("CLR-001").len(), 4);
    assert_eq!(app.chat_messages().len(), 1);
    let captured = prompts.lock().unwrap();
    assert_eq!(captured.len(), 2);
    for prompt in captured.iter() {
        assert!(!prompt.contains("MAIN CHAT PRIVATE SENTINEL"));
        assert!(!prompt.contains("OTHER TASK PRIVATE SENTINEL"));
    }
    assert!(captured[1].contains("Corporate SSO recorded. Any additional requirement?"));
    let Screen::Connected(p) = &mut app.screen else {
        panic!()
    };
    let persisted = crate::core::state::PlannerState::load(&repo).unwrap();
    assert!(persisted.spec_text.unwrap().contains("SSO with MFA"));
    assert_eq!(persisted.resolved_items[0].conversation_key(), "CLR-001");
    p.task_chats = Default::default();
    p.task_chats.ensure_loaded(&p.chat_slug);
    assert_eq!(p.task_chats.messages["CLR-001"].len(), 4);
    assert_eq!(p.task_chats.messages["CLR-002"].len(), 1);
    let output = frame(&mut app, &ctx, vec![]);
    assert!(text_position(&output, "Done · 1").is_some());
    assert!(text_position(&output, "Corporate SSO with MFA is confirmed.").is_some());
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn tabs_load_persisted_item_histories_without_main_or_other_task_messages() {
    let mut app = fixture();
    let ctx = egui::Context::default();
    let first = "planning/tasks/fixture/001-task.md";
    let second = "planning/tasks/fixture/002-task.md";
    let dir = std::env::temp_dir().join(format!("packet_tab_isolation_{}", std::process::id()));
    let slug = dir.to_str().unwrap();
    if let Screen::Connected(p) = &mut app.screen {
        p.chat = vec![ChatMessage::new(
            ChatRole::Agent,
            "MAIN HISTORY MARKER",
            None,
        )];
        let mut saved = crate::persistence::task_chats::TaskChats::default();
        saved
            .append(
                slug,
                first,
                vec![ChatMessage::new(
                    ChatRole::Agent,
                    "FIRST TASK HISTORY",
                    None,
                )],
            )
            .unwrap();
        saved
            .append(
                slug,
                second,
                vec![ChatMessage::new(
                    ChatRole::Agent,
                    "SECOND TASK HISTORY",
                    None,
                )],
            )
            .unwrap();
        p.task_chats = Default::default();
        p.task_chats.ensure_loaded(slug);
    }
    frame(&mut app, &ctx, vec![]);
    click_text(&mut app, &ctx, "Open conversation");
    // Add the second loaded conversation to the same tab strip, then exercise
    // actual pointer-driven tab selection and closing below.
    ctx.data_mut(|d| {
        let id = egui::Id::new("packet_chat_tabs");
        let mut tabs = d.get_temp::<crate::ui::layout::ChatTabs>(id).unwrap();
        tabs.keys.push(second.into());
        d.insert_temp(id, tabs);
    });
    let assert_history = |output: &egui::FullOutput, expected: &str| {
        let board_x = text_position(output, "Board  3").unwrap().x;
        let texts = output
            .shapes
            .iter()
            .filter_map(|shape| match &shape.shape {
                egui::Shape::Text(text) if text.pos.x < board_x - 100.0 => Some(text.galley.text()),
                _ => None,
            })
            .collect::<Vec<_>>();
        for marker in [
            "MAIN HISTORY MARKER",
            "FIRST TASK HISTORY",
            "SECOND TASK HISTORY",
        ] {
            assert_eq!(
                texts.contains(&marker),
                marker == expected,
                "{marker}: {texts:?}"
            );
        }
    };
    let output = frame(&mut app, &ctx, vec![]);
    assert_history(&output, "FIRST TASK HISTORY");
    let label = text_position(&output, "TASK-001").unwrap();
    let close = text_position(&output, "×").unwrap();
    assert!(
        output.shapes.iter().any(|shape| matches!(&shape.shape,
        egui::Shape::Rect(rect) if rect.fill == crate::ui::theme::ACCENT_SOFT
            && rect.rect.contains(label) && rect.rect.contains(close))),
        "ID and close button must share one tab background"
    );
    let output = click_text(&mut app, &ctx, "TASK-002");
    assert_history(&output, "SECOND TASK HISTORY");
    let output = click_text(&mut app, &ctx, "Main Chat");
    assert_history(&output, "MAIN HISTORY MARKER");
    let output = click_text(&mut app, &ctx, "TASK-001");
    assert_history(&output, "FIRST TASK HISTORY");
    let output = click_text(&mut app, &ctx, "×");
    assert_history(&output, "MAIN HISTORY MARKER");
    std::fs::remove_dir_all(dir).unwrap();
}
