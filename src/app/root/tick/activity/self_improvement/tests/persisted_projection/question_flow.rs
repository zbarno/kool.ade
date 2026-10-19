use crate::ui::Surface;
use std::sync::{Arc, Mutex};

struct CaptureAnswer(Arc<Mutex<Vec<String>>>);

impl crate::harness::AiHarness for CaptureAnswer {
    fn label(&self) -> String {
        "self-improvement Q&A fixture".into()
    }

    fn check_available(&self) -> Result<String, crate::error::AppError> {
        Ok(self.label())
    }

    fn execute(
        &self,
        request: &crate::harness::PlanningRequest,
    ) -> Result<crate::harness::HarnessOutcome, crate::error::AppError> {
        self.0.lock().unwrap().push(request.prompt_body.clone());
        Err(crate::error::AppError::Other(
            "fixture captures the answer handoff".into(),
        ))
    }
}

/// Production boundary: the real answer command must persist the user's
/// response and hand it to the Manager. A QuestionV1 unit test cannot prove the
/// app checks the current feature snapshot, recomputes readiness, records the
/// answer, and dispatches the interview turn. The caller applies a separate
/// synthetic lifecycle AnswersResolved event afterward; this does not prove the
/// lifecycle event is causally gated on the QuestionV1 answer.
/// Dependencies: headless app fixture, temporary planning/chat stores, and a
/// scripted in-process harness; no Docker or network. Setup cost is low.
pub(super) fn answer_product_question(
    app: &mut crate::app::root::KooladeApp,
    feature_uid: &str,
) -> String {
    use crate::core::planning::{
        AcceptanceTestTrace, MATERIAL_AXES, QuestionCategory, QuestionOption, QuestionPriority,
        QuestionProposal, QuestionStore, ReadinessChecklistV1, propose_questions,
    };

    let feature_markdown = {
        let crate::app::root::Screen::Connected(project) = &mut app.screen else {
            panic!("fixture should be connected")
        };
        project
            .state
            .active_features
            .iter()
            .find_map(|(_, markdown)| {
                crate::domain::ArtifactIdentity::from_markdown(markdown)
                    .ok()
                    .flatten()
                    .filter(|identity| identity.uid == feature_uid)
                    .map(|_| markdown.clone())
            })
            .expect("pursuit should create the self-improvement feature")
    };
    assert!(!feature_markdown.is_empty());
    let chat_slug = format!("self-improvement-question-{}", uuid::Uuid::new_v4());
    let chat_root = crate::persistence::project_dir(&chat_slug);
    let (spec_revision, planning) = {
        let crate::app::root::Screen::Connected(project) = &mut app.screen else {
            panic!("fixture should remain connected")
        };
        project.chat_slug = chat_slug;
        project.activity.last_update = Some(std::time::Instant::now());
        let spec_revision = crate::app::root::planner_pass::spec_revision(project, feature_uid)
            .expect("feature snapshot should have a stable revision");
        (spec_revision, project.state.planning_store.clone())
    };
    let proposal = QuestionProposal {
        feature_uid: feature_uid.to_owned(),
        plan_revision: 1,
        decision_subject: "approval boundary".into(),
        decision_domain: "authorization".into(),
        origin_requirement_ids: vec!["REQ-1".into()],
        category: QuestionCategory::Security,
        priority: QuestionPriority::Blocking,
        question: "Who owns approval for this product change?".into(),
        reason: "Approval authority must be explicit before implementation.".into(),
        options: vec![QuestionOption {
            id: "operator".into(),
            label: "Workspace operator".into(),
            tradeoffs: "The operator confirms the current plan.".into(),
        }],
        recommended_option_id: Some("operator".into()),
        no_answer_impact: "Implementation remains blocked.".into(),
        evidence_refs: vec![format!("feature:{feature_uid}")],
        assumption_if_deferred: None,
        depends_on_question_ids: Vec::new(),
    };
    let question = propose_questions(vec![proposal], &[]).unwrap().remove(0);
    let questions = QuestionStore::new(&planning);
    questions.save(&question, 0).unwrap();
    questions
        .save_readiness(
            &ReadinessChecklistV1 {
                feature_uid: feature_uid.to_owned(),
                plan_revision: question.plan_revision,
                spec_revision: Some(spec_revision),
                required_requirement_ids: Some(vec!["REQ-1".into()]),
                assessed_axes: MATERIAL_AXES.to_vec(),
                acceptance_tests: vec![AcceptanceTestTrace {
                    requirement_id: "REQ-1".into(),
                    test_ref: "tests/approval-boundary.rs".into(),
                }],
                evidence_refs: vec![format!("feature:{feature_uid}")],
                unresolved_blocker_ids: vec![question.uid.clone()],
                blocker_notes: Vec::new(),
                ready: false,
                stalled: false,
                no_progress_count: 0,
                progress_fingerprint: None,
                revision: 0,
            },
            0,
        )
        .unwrap();

    let prompts = Arc::new(Mutex::new(Vec::new()));
    app.task_harness = Some(Box::new(CaptureAnswer(prompts.clone())));
    app.dispatch_ui_command(crate::ui::ApplicationCommand::AnswerClarification {
        uid: question.uid.clone(),
        expected_revision: question.revision,
        answer: "The workspace operator owns approval.".into(),
        selected_option: Some("operator".into()),
    });
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while prompts.lock().unwrap().is_empty() {
        app.tick(0.016, &egui::Context::default());
        assert!(
            std::time::Instant::now() < deadline,
            "answer did not reach the Manager"
        );
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    let turn_deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while app.conversation_busy() {
        app.tick(0.016, &egui::Context::default());
        assert!(
            std::time::Instant::now() < turn_deadline,
            "answer handoff did not settle"
        );
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert!(prompts.lock().unwrap()[0].contains("The workspace operator owns approval."));
    let _ = std::fs::remove_dir_all(chat_root);
    question.uid
}
