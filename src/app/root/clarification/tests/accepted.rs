use super::*;
use crate::ui::Surface;

#[test]
// Production boundary: an accepted answer must persist on its own durable thread
// and start the Manager reconciliation turn with the exact user decision. A unit
// test of answer mutation cannot prove app dispatch, thread storage, and turn wiring.
// Dependencies: in-process app fixture, temporary planning store, and temp chat path.
// Setup cost: low; the scripted harness exits after capturing the request.
fn accepted_answer_persists_its_thread_and_starts_spec_reconciliation() {
    use crate::core::planning::{QuestionStatus, QuestionStore};
    use std::sync::{Arc, Mutex};

    let (mut app, question, _) = setup();
    let chat_slug = format!("clarification-test-{}", uuid::Uuid::new_v4());
    let chat_root = crate::persistence::project_dir(&chat_slug);
    let calls = Arc::new(Mutex::new(Vec::new()));
    app.task_harness = Some(Box::new(CaptureClarificationHarness(calls.clone())));
    let Screen::Connected(project) = &mut app.screen else {
        panic!("fixture must be connected")
    };
    project.chat_slug = chat_slug;

    app.dispatch_ui_command(crate::ui::ApplicationCommand::AnswerClarification {
        uid: question.uid.clone(),
        expected_revision: question.revision,
        answer: "Keep retention configurable for each workspace.".into(),
        selected_option: Some("yes".into()),
    });

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while calls.lock().unwrap().is_empty() {
        assert!(
            std::time::Instant::now() < deadline,
            "Manager turn did not start"
        );
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert!(calls.lock().unwrap()[0].contains("Treat the answer as explicit product intent"));
    assert!(calls.lock().unwrap()[0].contains("Keep retention configurable for each workspace."));

    let context = egui::Context::default();
    let turn_deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while app.conversation_busy() {
        app.tick(0.016, &context);
        assert!(
            std::time::Instant::now() < turn_deadline,
            "captured Manager turn did not finish"
        );
        std::thread::sleep(std::time::Duration::from_millis(5));
    }

    let Screen::Connected(project) = &app.screen else {
        panic!("answer keeps the fixture connected")
    };
    let (saved, _) = QuestionStore::new(&project.state.planning_store)
        .load(&question.uid)
        .unwrap();
    assert_eq!(saved.status, QuestionStatus::Answered);
    assert_eq!(
        saved.answer_text.as_deref(),
        Some("Keep retention configurable for each workspace.")
    );
    let thread = project
        .task_chats
        .messages
        .get(&format!("clarification:{}", question.uid))
        .unwrap();
    assert!(thread.iter().any(|message| {
        message
            .text
            .contains("Keep retention configurable for each workspace.")
    }));
    assert!(chat_root.join("task-conversations.json").is_file());
    let _ = std::fs::remove_dir_all(chat_root);
}

#[test]
// Production boundary: declining a substantive question must leave its blocker
// unresolved and must not dispatch an answer-as-intent Manager turn. A core state
// transition test cannot prove the app's persisted state and dispatch behavior.
// Dependencies: in-process app fixture, temporary planning store, and temp chat path.
// Setup cost: low; no external services.
fn declined_choice_stays_unresolved_without_manager_reconciliation() {
    use crate::core::planning::{QuestionStatus, QuestionStore};

    let (mut app, question, _) = setup();
    let chat_slug = format!("clarification-decline-{}", uuid::Uuid::new_v4());
    let chat_root = crate::persistence::project_dir(&chat_slug);
    let Screen::Connected(project) = &mut app.screen else {
        panic!("fixture must be connected")
    };
    project.chat_slug = chat_slug;

    app.dispatch_ui_command(crate::ui::ApplicationCommand::AnswerClarification {
        uid: question.uid.clone(),
        expected_revision: question.revision,
        answer: String::new(),
        selected_option: None,
    });

    let Screen::Connected(project) = &app.screen else {
        panic!("declining keeps the fixture connected")
    };
    let store = QuestionStore::new(&project.state.planning_store);
    let (saved, _) = store.load(&question.uid).unwrap();
    assert_eq!(saved.status, QuestionStatus::Declined);
    assert!(project.active_turn.is_none());
    assert!(project.activity.pending_clarification_planner.is_none());
    assert!(matches!(
        store.reassess_readiness(&question.feature_uid, question.plan_revision),
        Ok(Some(crate::core::planning::ReadinessResult::NeedsWork { blockers, .. }))
            if blockers.iter().any(|blocker| blocker == &question.uid)
    ));
    let thread = project
        .task_chats
        .messages
        .get(&format!("clarification:{}", question.uid))
        .unwrap();
    assert!(
        thread
            .iter()
            .any(|message| { message.text.contains("I declined to choose an option.") })
    );
    let _ = std::fs::remove_dir_all(chat_root);
}
