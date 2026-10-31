//! Exercises production task-modal painting; no GPU, provider, or Docker needed.
use super::*;

#[test]
fn decision_controls_stay_left_of_questions_and_context_starts_closed() {
    let mut app = fixture();
    let key = "planning/tasks/fixture/001-task.md";
    app.attention_fixture.insert(
        key.into(),
        crate::core::attention::Brief {
            problem: "Should I update the plan?".into(),
            recommendation: None,
            options: vec![crate::core::attention::OptionBrief {
                id: "update".into(),
                label: "Yes, update it".into(),
                meaning: "Use the new default.".into(),
                consequence: "The plan changes.".into(),
                clarification: false,
                source_evidence: None,
            }],
            steps: vec![],
            after: "A related issue was resolved.".into(),
        },
    );
    if let Screen::Connected(project) = &mut app.screen {
        project.queue.blocked.insert(
            key.into(),
            crate::core::implementation::Failure::other(
                "A related issue was resolved; a plan update needs your answer.",
            ),
        );
    }
    let ctx = super::super::mockup_layout::styled_context();
    ctx.data_mut(|data| data.insert_temp(egui::Id::new("koolade_selected_task"), key.to_owned()));
    let size = egui::vec2(1280.0, 820.0);
    for _ in 0..3 {
        frame_at(&mut app, &ctx, vec![], size);
    }
    let output = frame_at(&mut app, &ctx, vec![], size);
    let answer = text_position(&output, "Yes, update it").expect("answer card");
    let question = text_position(&output, "Ask a question").expect("conversation");
    assert!(answer.x < question.x);
    assert!(text_position(&output, "I'm not sure").is_some());
    assert!(text_position(&output, "Why am I seeing this?").is_some());
    assert!(text_position(&output, "Supporting details").is_none());
}

// Production command -> scoped worker -> adoption boundary. Mocked model output
// deliberately offers edits; local Git fixture only, no provider or Docker.
// A painter or validator test alone cannot prove chat does not adopt changes.
#[test]
fn clarification_command_keeps_the_planning_decision_open() {
    struct Explanation;
    impl crate::harness::AiHarness for Explanation {
        fn label(&self) -> String {
            "synthetic explanation".into()
        }
        fn check_available(&self) -> Result<String, crate::AppError> {
            Ok("ready".into())
        }
        fn execute(
            &self,
            _: &crate::harness::PlanningRequest,
        ) -> Result<crate::harness::HarnessOutcome, crate::AppError> {
            Ok(crate::harness::HarnessOutcome {
                final_text: r#"{"schema_version":1,"assistant_message":"This choice controls access.","open_items_resolved":[{"id":"CLR-100","resolution":"This must not be adopted."}]}"#.into(),
                envelope: None,
                stderr_tail: String::new(),
            })
        }
    }
    let mut app = fixture();
    let key = "CLR-100";
    if let Screen::Connected(project) = &mut app.screen {
        project.state.items.push(OpenItem::new(
            key.into(),
            crate::domain::Priority::High,
            crate::domain::ItemKind::Question,
            "General".into(),
            None,
            "Who should have access?".into(),
            "Controls project access.".into(),
        ));
    }
    app.task_harness = Some(Box::new(Explanation));
    app.dispatch_ui_command(crate::ui::ApplicationCommand::TaskDetail(
        crate::ui::task_detail::Command::AskQuestion {
            ticket: key.into(),
            draft: "Please explain the alternatives".into(),
        },
    ));
    let ctx = egui::Context::default();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        app.tick(0.016, &ctx);
        let Screen::Connected(project) = &app.screen else {
            panic!("connected fixture")
        };
        if project.task_turns.is_empty() {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "mock explanation did not finish"
        );
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    let Screen::Connected(project) = &app.screen else {
        unreachable!()
    };
    assert_eq!(
        project
            .state
            .items
            .iter()
            .find(|item| item.id == key)
            .unwrap()
            .status,
        crate::domain::ItemStatus::Open
    );
    let history = &project.task_chats.messages[key];
    assert!(
        history
            .iter()
            .any(|message| message.role == ChatRole::User && message.clarification)
    );
    assert!(
        history
            .iter()
            .any(|message| message.role == ChatRole::Agent
                && message.text.contains("controls access"))
    );
}

// Production blocker read-model and modal response boundary. Local Git fixture;
// setup is one fixture and several frames, with no provider or Docker worker.
// Structured human steps must expose an answer even without a prose question.
#[test]
fn human_steps_without_choices_expose_an_explicit_answer() {
    let mut app = fixture();
    let key = "planning/tasks/fixture/001-task.md";
    app.attention_fixture.insert(
        key.into(),
        crate::core::attention::Brief {
            problem: "A project setting is missing.".into(),
            recommendation: None,
            options: vec![],
            steps: vec![crate::core::attention::HumanStep {
                owner: "Project owner".into(),
                action: "Provide the required setting.".into(),
            }],
            after: "Provide the setting so work can continue.".into(),
        },
    );
    if let Screen::Connected(project) = &mut app.screen {
        project.queue.blocked.insert(
            key.into(),
            crate::core::implementation::Failure::other("A required setting is missing."),
        );
        project.task_chats.messages.insert(
            key.into(),
            vec![ChatMessage::new(
                ChatRole::Agent,
                "I need your help before I continue. Ask me if anything is unclear.",
                None,
            )],
        );
    }
    let ctx = egui::Context::default();
    frame(&mut app, &ctx, vec![]);
    click_text(&mut app, &ctx, "First task");
    click_text(&mut app, &ctx, "Enter your answer…");
    frame(
        &mut app,
        &ctx,
        vec![egui::Event::Text("Use the project default.".into())],
    );
    click_text(&mut app, &ctx, "Submit answer");
    let Screen::Connected(project) = &app.screen else {
        unreachable!()
    };
    assert!(project.task_chats.messages[key].iter().any(|message| {
        message.role == ChatRole::User
            && message.text == "My answer: Use the project default."
            && message.answer_context.is_some()
            && !message.clarification
    }));
}
