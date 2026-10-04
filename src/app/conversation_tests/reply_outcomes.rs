use super::*;

#[test]
fn rejected_failed_and_cancelled_replies_stay_in_task_and_preserve_project_state() {
    for mode in ["rejected", "failed", "cancelled"] {
        let root = std::env::temp_dir().join(format!("koolade_chat_{mode}_{}", std::process::id()));
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
        let product_vision = root.join(".koolade-packet/planning/product/overview.md");
        let before = std::fs::read(&product_vision).unwrap();
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
            assert!(!p.task_turns["CLR-001"].cancel_requested());
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
        assert_eq!(std::fs::read(&product_vision).unwrap(), before);
        let Screen::Connected(p) = &mut app.screen else {
            panic!()
        };
        let expected = p.task_chats.messages["CLR-001"].clone();
        let manager_prompt =
            crate::app::manager::Manager::prompt_body(p.as_ref(), &p.activity.pending);
        assert!(manager_prompt.contains("Use corporate SSO"));
        assert!(!manager_prompt.contains("Task reply applied to planning artifacts."));
        assert!(manager_prompt.contains(if mode == "rejected" {
            "Turn rejected"
        } else {
            "Planning stopped"
        }));
        p.task_chats = Default::default();
        p.task_chats.ensure_loaded(&p.chat_slug);
        assert_eq!(p.task_chats.messages["CLR-001"], expected);
        std::fs::remove_dir_all(root).unwrap();
    }
}
