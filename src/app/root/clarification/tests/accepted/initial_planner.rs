use super::*;
use crate::core::planning::{QuestionStatus, QuestionStore};

#[test]
// Production boundary: a successful main-chat feature update must schedule the
// initial ambiguity Planner and persist its independent questions/readiness. Unit
// tests of turn parsing or Planner persistence cannot prove the adoption-to-Planner
// scheduling handoff. Dependencies: in-process app and temporary planning store;
// no external services. Setup cost: moderate because it drives both worker turns.
fn main_chat_spec_update_runs_initial_clarification_planner() {
    let (mut app, original, feature) = super::super::setup();
    let chat_slug = format!("clarification-initial-{}", uuid::Uuid::new_v4());
    let chat_root = crate::persistence::project_dir(&chat_slug);
    let updated_feature = feature.replace(
        "- No retention choice recorded yet.",
        "- Retention is configurable for each workspace.",
    );
    let manager_output = serde_json::json!({
        "schema_version": 2,
        "assistant_message": "Updated the active feature specification.",
        "change_summary": "Clarify workspace retention behavior",
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
        "proposedQuestions": [
            {
                "decisionSubject": "retention policy owner",
                "decisionDomain": "data ownership",
                "originRequirementIds": ["REQ-1"],
                "category": "Data",
                "priority": "Blocking",
                "question": "Who may change the workspace retention policy?",
                "reason": "Authorization affects data ownership and auditability.",
                "options": [
                    {"id": "owner", "label": "Workspace owner", "tradeoffs": "One accountable role."},
                    {"id": "admin", "label": "Any administrator", "tradeoffs": "More flexible, broader access."}
                ],
                "recommendedOptionId": "owner",
                "noAnswerImpact": "The policy change authority remains unclear.",
                "evidenceRefs": [format!("feature:{}", original.feature_uid)],
                "assumptionIfDeferred": null,
                "dependsOnQuestionIds": []
            },
            {
                "decisionSubject": "retention enforcement failure",
                "decisionDomain": "failure behavior",
                "originRequirementIds": ["REQ-1"],
                "category": "FailureMode",
                "priority": "Important",
                "question": "What should happen when an updated retention policy cannot be enforced?",
                "reason": "A partial update could leave data under an unknown policy.",
                "options": [
                    {"id": "rollback", "label": "Keep prior policy", "tradeoffs": "Existing behavior remains in force."},
                    {"id": "block", "label": "Block writes", "tradeoffs": "Safer, but interrupts workspace use."}
                ],
                "recommendedOptionId": "rollback",
                "noAnswerImpact": "The system may behave inconsistently during policy updates.",
                "evidenceRefs": [format!("feature:{}", original.feature_uid)],
                "assumptionIfDeferred": null,
                "dependsOnQuestionIds": []
            }
        ],
        "draftChanges": [],
        "readiness": {
            "ready": false,
            "blockers": [],
            "evidence": [format!("feature:{}", original.feature_uid)],
            "assessedAxes": [
                "Who", "What", "When", "Where", "Why", "How", "OutOfScope",
                "CriticalPath", "SystemBoundaries", "FailureBehavior", "DataOwnership",
                "Authorization", "MeasurableSuccess", "TestCases"
            ],
            "acceptanceTests": [{"requirementId": "REQ-1", "testRef": "tests/retention.rs"}]
        }
    })
    .to_string();

    let Screen::Connected(project) = &mut app.screen else {
        panic!("fixture must be connected")
    };
    assert!(
        project.task_chats.active.is_none(),
        "initial interview uses the main chat"
    );
    assert!(
        project.active_planning_work.is_none(),
        "initial interview is not a task work turn"
    );
    project.chat_slug = chat_slug;
    app.task_harness = Some(Box::new(FixedClarificationHarness(manager_output)));
    app.start_turn_with_purpose(
        "Clarify the retention behavior in the active feature.",
        crate::core::workflow::TurnPurpose::Interview,
    );
    assert!(matches!(&app.screen, Screen::Connected(project) if project.active_turn.is_some()));

    let context = egui::Context::default();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(45);
    loop {
        // Keep the deterministic Planner harness available through the Manager
        // completion tick, when the application starts the initial Planner pass.
        app.task_harness = Some(Box::new(FixedClarificationHarness(planner_output.clone())));
        app.tick(0.016, &context);
        let completed = match &app.screen {
            Screen::Connected(project) => {
                let store = QuestionStore::new(&project.state.planning_store);
                store.list().is_ok_and(|questions| {
                    questions.len() == 3
                        && questions
                            .iter()
                            .filter(|question| question.status == QuestionStatus::Open)
                            .count()
                            == 2
                }) && store
                    .load_readiness(&original.feature_uid)
                    .is_ok_and(|readiness| {
                        readiness.is_some_and(|(checklist, _)| checklist.plan_revision == 5)
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
                    "active_turn={}, planner={}, pending_planner={:?}, work={:?}, schedule={:?}, active_feature={:?}, questions={:?}, chat={:?}, feature_updated={}",
                    project.active_turn.is_some(),
                    project.activity.clarification_planner.is_some(),
                    project.activity.pending_clarification_planner,
                    project.active_planning_work,
                    super::super::super::planner_for_changed_active_feature(project),
                    project.state.active_feature.as_ref().map(|(id, markdown)| (
                        id,
                        markdown.contains("Retention is configurable for each workspace."),
                    )),
                    store.list().map(|questions| questions
                        .into_iter()
                        .map(|question| (
                            question.question,
                            question.status,
                            question.plan_revision
                        ))
                        .collect::<Vec<_>>()),
                    project
                        .chat
                        .iter()
                        .rev()
                        .take(4)
                        .map(|message| &message.text)
                        .collect::<Vec<_>>(),
                    project.state.active_features.iter().any(|(_, markdown)| {
                        markdown.contains("Retention is configurable for each workspace.")
                    }),
                )
            }
            Screen::Welcome => "screen disconnected".into(),
        };
        assert!(
            std::time::Instant::now() < deadline,
            "new feature snapshot did not trigger a persisted clarification Planner pass: {diagnostics}"
        );
        std::thread::sleep(std::time::Duration::from_millis(5));
    }

    let Screen::Connected(project) = &app.screen else {
        panic!("feature stays connected")
    };
    let store = QuestionStore::new(&project.state.planning_store);
    let questions = store.list().unwrap();
    assert!(questions.iter().any(|question| {
        question.uid == original.uid && question.status == QuestionStatus::Superseded
    }));
    assert!(questions.iter().any(|question| {
        question.question == "Who may change the workspace retention policy?"
            && question.status == QuestionStatus::Open
    }));
    assert!(questions.iter().any(|question| {
        question.question
            == "What should happen when an updated retention policy cannot be enforced?"
            && question.status == QuestionStatus::Open
    }));
    let _ = std::fs::remove_dir_all(chat_root);
}
