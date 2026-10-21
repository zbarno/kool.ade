use super::*;

#[test]
// Production boundary: a successful Manager answer-reconciliation turn must
// trigger the configured follow-up Planner, whose result is durably saved as a
// new question and readiness record. Separate unit tests of answer handling and
// persistence cannot prove the turn-completion handoff between these workers.
// Dependencies: in-process app fixture and temporary project/planning stores; no
// external services. Setup cost: moderate because it drives both worker turns.
fn successful_manager_update_runs_planner_and_persists_follow_up_readiness() {
    use crate::core::planning::{QuestionStatus, QuestionStore};

    let (mut app, original, feature) = setup();
    let chat_slug = format!("clarification-follow-up-{}", uuid::Uuid::new_v4());
    let chat_root = crate::persistence::project_dir(&chat_slug);
    let updated_feature = feature.replace(
        "- No retention choice recorded yet.",
        "- Retention is configurable for each workspace.",
    );
    let manager_output = serde_json::json!({
        "schema_version": 2,
        "assistant_message": "Recorded the user's retention decision.",
        "change_summary": "Record the retention decision",
        "document_updates": [{
            "document_id": "feature:CHG-997",
            "content": updated_feature,
        }],
        "open_items_added": [],
        "open_items_updated": [],
        "open_items_resolved": [],
        "next_question_id": null,
    })
    .to_string();
    let planner_output = serde_json::json!({
        "proposedQuestions": [{
            "decisionSubject": "retention test boundary",
            "decisionDomain": "acceptance testing",
            "originRequirementIds": ["REQ-1"],
            "category": "Testing",
            "priority": "Blocking",
            "question": "Should the acceptance test cover workspace-specific retention values?",
            "reason": "The recorded decision makes this behavior configurable.",
            "options": [
                {"id": "workspace", "label": "Yes", "tradeoffs": "Tests each workspace's policy."},
                {"id": "default", "label": "No", "tradeoffs": "Tests only the default policy."},
            ],
            "recommendedOptionId": "workspace",
            "noAnswerImpact": "The configurable behavior has no explicit acceptance test.",
            "evidenceRefs": [format!("feature:{}", original.feature_uid)],
            "assumptionIfDeferred": null,
            "dependsOnQuestionIds": [],
        }],
        "draftChanges": [],
        "readiness": {
            "ready": false,
            "blockers": [],
            "evidence": [format!("feature:{}", original.feature_uid)],
            "assessedAxes": [
                "Who", "What", "When", "Where", "Why", "How", "OutOfScope",
                "CriticalPath", "SystemBoundaries", "FailureBehavior", "DataOwnership",
                "Authorization", "MeasurableSuccess", "TestCases",
            ],
            "acceptanceTests": [{
                "requirementId": "REQ-1",
                "testRef": "tests/active-spec.rs",
            }],
        },
    })
    .to_string();

    let Screen::Connected(project) = &mut app.screen else {
        panic!("fixture must be connected")
    };
    project.chat_slug = chat_slug;
    assert!(
        project.active_turn.is_none(),
        "fixture starts without a Manager turn"
    );
    let question_store = QuestionStore::new(&project.state.planning_store);
    assert_eq!(question_store.load(&original.uid).unwrap().0, original);
    super::super::fence::current_question_planning_revision(project, &question_store, &original)
        .unwrap();
    app.task_harness = Some(Box::new(FixedClarificationHarness(manager_output)));
    app.dispatch_ui_command(crate::ui::ApplicationCommand::AnswerClarification {
        uid: original.uid.clone(),
        expected_revision: original.revision,
        answer: "Keep retention configurable for each workspace.".into(),
        selected_option: Some("yes".into()),
    });
    let Screen::Connected(project) = &app.screen else {
        panic!("answer keeps the fixture connected")
    };
    assert_eq!(
        QuestionStore::new(&project.state.planning_store)
            .load(&original.uid)
            .unwrap()
            .0
            .status,
        QuestionStatus::Answered,
        "answer command did not persist the user choice"
    );
    assert!(
        project.active_turn.is_some(),
        "Manager reconciliation turn did not start"
    );
    // The Manager override is consumed by the answer turn; install a distinct
    // override before its completion dispatches the follow-up Planner.
    let context = egui::Context::default();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    loop {
        // Background activity may request a harness during the Manager turn.
        // Reinstall the scripted Planner route immediately before each tick so
        // the completion handoff always receives it.
        app.task_harness = Some(Box::new(FixedClarificationHarness(planner_output.clone())));
        app.tick(0.016, &context);
        let completed = match &app.screen {
            Screen::Connected(project) => {
                let store = QuestionStore::new(&project.state.planning_store);
                store.list().is_ok_and(|questions| questions.len() == 2)
                    && store
                        .load_readiness(&original.feature_uid)
                        .is_ok_and(|readiness| {
                            readiness.is_some_and(|(checklist, _)| checklist.revision > 0)
                        })
            }
            Screen::Welcome => false,
        };
        if completed {
            break;
        }
        let diagnostics = match &app.screen {
            Screen::Connected(project) => {
                let store = QuestionStore::new(&project.state.planning_store);
                format!(
                    "active_turn={}, planner={}, pending_planner={:?}, active_work={:?}, pending={:?}, questions={:?}, chat={:?}, task_chat={:?}, feature_updated={}",
                    project.active_turn.is_some(),
                    project.activity.clarification_planner.is_some(),
                    project.activity.pending_clarification_planner,
                    project.active_planning_work,
                    project.activity.pending,
                    store.list().map(|questions| questions
                        .into_iter()
                        .map(|question| (question.uid, question.status, question.revision))
                        .collect::<Vec<_>>()),
                    project
                        .chat
                        .iter()
                        .rev()
                        .take(4)
                        .map(|message| &message.text)
                        .collect::<Vec<_>>(),
                    project.task_chats.active,
                    project.state.active_features[0]
                        .1
                        .contains("Retention is configurable for each workspace."),
                )
            }
            Screen::Welcome => "screen disconnected".into(),
        };
        assert!(
            std::time::Instant::now() < deadline,
            "Manager-to-Planner follow-up did not persist its question and readiness: {diagnostics}"
        );
        std::thread::sleep(std::time::Duration::from_millis(5));
    }

    let Screen::Connected(project) = &app.screen else {
        panic!("fixture remains connected")
    };
    assert!(
        project.state.active_features[0]
            .1
            .contains("Retention is configurable for each workspace.")
    );
    let store = QuestionStore::new(&project.state.planning_store);
    let (saved_answer, _) = store.load(&original.uid).unwrap();
    assert_eq!(saved_answer.status, QuestionStatus::Superseded);
    assert_eq!(
        saved_answer.answer_text.as_deref(),
        Some("Keep retention configurable for each workspace.")
    );
    let questions = store.list().unwrap();
    let follow_up = questions
        .into_iter()
        .find(|question| question.uid != original.uid)
        .unwrap();
    assert_eq!(follow_up.status, QuestionStatus::Open);
    assert_eq!(
        follow_up.question,
        "Should the acceptance test cover workspace-specific retention values?"
    );
    assert!(matches!(
        store.reassess_readiness(&original.feature_uid, follow_up.plan_revision),
        Ok(Some(crate::core::planning::ReadinessResult::NeedsWork { blockers, .. }))
            if blockers.contains(&follow_up.uid)
    ));
    let _ = std::fs::remove_dir_all(chat_root);
}
