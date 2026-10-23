use super::*;
use crate::core::planning::{
    MATERIAL_AXES, QuestionCategory, QuestionOption, QuestionPriority, QuestionProposal,
    QuestionStore, ReadinessChecklistV1, propose_questions,
};
use std::{
    sync::{Arc, Mutex},
    thread,
    time::Duration,
};

struct ScriptedEvolveHarness {
    output: String,
    requests: Arc<Mutex<Vec<String>>>,
}

impl crate::harness::AiHarness for ScriptedEvolveHarness {
    fn label(&self) -> String {
        "Evolve clarification fixture".into()
    }

    fn check_available(&self) -> Result<String, crate::error::AppError> {
        Ok(self.label())
    }

    fn execute(
        &self,
        request: &crate::harness::PlanningRequest,
    ) -> Result<crate::harness::HarnessOutcome, crate::error::AppError> {
        self.requests
            .lock()
            .unwrap()
            .push(request.prompt_body.clone());
        thread::sleep(Duration::from_millis(250));
        Ok(crate::harness::HarnessOutcome {
            final_text: self.output.clone(),
            envelope: None,
            stderr_tail: String::new(),
        })
    }
}

// Production boundary: Evolve's persisted clarification answer must traverse
// the real Manager and follow-up Planner workers, update the feature, and save
// readiness before ordinary plan confirmation. Unit tests of answer parsing or
// Planner persistence cannot prove this cross-worker app handoff or that it
// stays fenced from clone allocation. Dependencies: a headless app, temporary
// planning store, local fake `gh`, and deterministic worker harnesses; no
// external services or Docker. Setup cost: moderate because both async workers
// and their persisted state are exercised.
#[cfg(unix)]
#[test]
fn pursued_product_question_reaches_planner_readiness_before_any_clone_is_allocated() {
    use std::os::unix::fs::PermissionsExt;

    let mut app = crate::app::root::board_tests::fixture();
    let opportunity_uid = {
        let crate::app::root::Screen::Connected(project) = &mut app.screen else {
            panic!("fixture should be connected")
        };
        app_product_proposal(project)
    };
    let opportunity_revision = match &app.screen {
        crate::app::root::Screen::Connected(project) => {
            let (_, revision) =
                crate::core::opportunity::OpportunityStore::new(&project.state.planning_store)
                    .load(&opportunity_uid)
                    .unwrap();
            revision
        }
        crate::app::root::Screen::Welcome => panic!("fixture should remain connected"),
    };
    let temp = tempfile::tempdir().unwrap();
    let gh = temp.path().join("gh");
    std::fs::write(
        &gh,
        "#!/bin/sh\n[ \"$1\" = api ] && [ \"$2\" = user ] || exit 2\nprintf '987654321\\n'\n",
    )
    .unwrap();
    std::fs::set_permissions(&gh, std::fs::Permissions::from_mode(0o700)).unwrap();
    crate::app::root::self_improvement_test_support::decide_product_pursuit(
        &mut app,
        &opportunity_uid,
        opportunity_revision,
        &gh,
    )
    .expect("authenticated product Pursue creates an Evolve planning run");

    let (feature_id, feature_uid, requirement_id, run_uid, original_run_phase, markdown, question) = {
        let crate::app::root::Screen::Connected(project) = &mut app.screen else {
            panic!("fixture should remain connected")
        };
        let (feature_id, markdown) = project
            .state
            .active_feature
            .as_ref()
            .expect("Pursue activates the draft feature");
        let identity = crate::domain::ArtifactIdentity::from_markdown(markdown)
            .unwrap()
            .expect("Pursue gives the draft a stable identity");
        let feature_id = feature_id.clone();
        let feature_uid = identity.uid.clone();
        let markdown = markdown.clone();
        let snapshot = crate::core::lifecycle::PlanSnapshotV1::from_feature(
            &feature_id,
            &identity,
            &markdown,
            &crate::core::workflow::feature_contract(&markdown),
        )
        .unwrap();
        let requirement_id = snapshot.requirements[0].stable_id.clone();
        let spec_revision =
            crate::app::root::planner_pass::spec_revision(project, &feature_uid).unwrap();
        let question = propose_questions(
            vec![QuestionProposal {
                feature_uid: feature_uid.clone(),
                plan_revision: 1,
                decision_subject: "workspace retention".into(),
                decision_domain: "product behavior".into(),
                origin_requirement_ids: vec![requirement_id.clone()],
                category: QuestionCategory::Data,
                priority: QuestionPriority::Blocking,
                question: "Should each workspace choose its own retention period?".into(),
                reason: "The pursued outcome does not specify how retention is selected.".into(),
                options: vec![
                    QuestionOption {
                        id: "workspace".into(),
                        label: "Each workspace chooses".into(),
                        tradeoffs: "Provides local control and needs clear authorization.".into(),
                    },
                    QuestionOption {
                        id: "global".into(),
                        label: "One global period".into(),
                        tradeoffs: "Applies a single retention choice to all workspaces.".into(),
                    },
                ],
                recommended_option_id: None,
                no_answer_impact: "The retention behavior remains ambiguous.".into(),
                evidence_refs: vec![format!("feature:{feature_uid}")],
                assumption_if_deferred: None,
                depends_on_question_ids: Vec::new(),
            }],
            &[],
        )
        .unwrap()
        .remove(0);
        let questions = QuestionStore::new(&project.state.planning_store);
        questions.save(&question, 0).unwrap();
        questions
            .save_readiness(
                &ReadinessChecklistV1 {
                    feature_uid: feature_uid.clone(),
                    plan_revision: 1,
                    spec_revision: Some(spec_revision),
                    required_requirement_ids: Some(vec![requirement_id.clone()]),
                    assessed_axes: Vec::new(),
                    acceptance_tests: Vec::new(),
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
        let decision =
            crate::core::opportunity::OpportunityStore::new(&project.state.planning_store)
                .decision_for_opportunity(&opportunity_uid)
                .unwrap()
                .expect("Pursue decision is persisted");
        let run_uid = crate::core::self_improvement::SelfImprovementStore::run_uid_for_decision(
            &decision.uid,
        )
        .unwrap();
        let run =
            crate::core::self_improvement::SelfImprovementStore::new(&project.state.planning_store)
                .load(&run_uid)
                .unwrap();
        (
            feature_id,
            feature_uid,
            requirement_id,
            run_uid,
            run.phase,
            markdown,
            question,
        )
    };

    let updated_feature = markdown.replace(
        "## Decisions and Assumptions",
        "## Decisions and Assumptions\n\n- The user chose workspace-specific retention periods.",
    );
    assert_ne!(updated_feature, markdown);
    let manager_output = serde_json::json!({
        "schema_version": 2,
        "assistant_message": "Recorded the user's workspace retention choice.",
        "change_summary": "Record workspace-specific retention choice",
        "document_updates": [{
            "document_id": format!("feature:{feature_id}"),
            "content": updated_feature.clone(),
        }],
        "open_items_added": [],
        "open_items_updated": [],
        "open_items_resolved": [],
        "next_question_id": null,
    })
    .to_string();
    let planner_output = serde_json::json!({
        "proposedQuestions": [],
        "draftChanges": [],
        "readiness": {
            "ready": true,
            "blockers": [],
            "evidence": [format!("feature:{feature_uid}")],
            "assessedAxes": [
                "Who", "What", "When", "Where", "Why", "How", "OutOfScope",
                "CriticalPath", "SystemBoundaries", "FailureBehavior", "DataOwnership",
                "Authorization", "MeasurableSuccess", "TestCases",
            ],
            "acceptanceTests": [{
                "requirementId": requirement_id,
                "testRef": "tests/self-improvement-lifecycle.rs",
            }]
        }
    })
    .to_string();

    let chat_slug = format!("evolve-clarification-{}", uuid::Uuid::new_v4());
    let chat_root = crate::persistence::project_dir(&chat_slug);
    let manager_requests = Arc::new(Mutex::new(Vec::new()));
    let planner_requests = Arc::new(Mutex::new(Vec::new()));
    let crate::app::root::Screen::Connected(project) = &mut app.screen else {
        panic!("fixture should remain connected")
    };
    project.chat_slug = chat_slug;
    app.task_harness = Some(Box::new(ScriptedEvolveHarness {
        output: manager_output,
        requests: Arc::clone(&manager_requests),
    }));
    app.dispatch_ui_command(crate::ui::ApplicationCommand::AnswerClarification {
        uid: question.uid.clone(),
        expected_revision: question.revision,
        answer: "Each workspace should choose its own retention period.".into(),
        selected_option: Some("workspace".into()),
    });
    app.task_harness = Some(Box::new(ScriptedEvolveHarness {
        output: planner_output.clone(),
        requests: Arc::clone(&planner_requests),
    }));

    let context = egui::Context::default();
    let deadline = std::time::Instant::now() + Duration::from_secs(60);
    loop {
        app.task_harness = Some(Box::new(ScriptedEvolveHarness {
            output: planner_output.clone(),
            requests: Arc::clone(&planner_requests),
        }));
        app.tick(0.016, &context);
        let completed = match &app.screen {
            crate::app::root::Screen::Connected(project) => {
                let questions = QuestionStore::new(&project.state.planning_store);
                questions.load_readiness(&feature_uid).is_ok_and(|record| {
                    record.is_some_and(|(readiness, _)| {
                        readiness.plan_revision == 2 && readiness.ready
                    })
                }) && questions.load(&question.uid).is_ok_and(|(saved, _)| {
                    saved.status == crate::core::planning::QuestionStatus::Superseded
                })
            }
            crate::app::root::Screen::Welcome => false,
        };
        if completed {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "Evolve clarification did not persist Manager update and Planner readiness"
        );
        thread::sleep(Duration::from_millis(5));
    }

    let crate::app::root::Screen::Connected(project) = &app.screen else {
        panic!("fixture remains connected")
    };
    assert!(project.state.active_features.iter().any(|(id, body)| {
        id == &feature_id && body.contains("The user chose workspace-specific retention periods.")
    }));
    let questions = QuestionStore::new(&project.state.planning_store);
    let readiness = questions
        .load_readiness(&feature_uid)
        .unwrap()
        .expect("Planner readiness is persisted")
        .0;
    assert_eq!(readiness.plan_revision, 2);
    assert!(readiness.ready);
    assert_eq!(readiness.assessed_axes, MATERIAL_AXES.to_vec());
    let answered_question = questions.load(&question.uid).unwrap().0;
    assert_eq!(
        answered_question.answer_text.as_deref(),
        Some("Each workspace should choose its own retention period.")
    );
    assert_eq!(
        answered_question.selected_option.as_deref(),
        Some("workspace")
    );
    let manager_prompt = manager_requests
        .lock()
        .unwrap()
        .iter()
        .find(|prompt| prompt.contains("Each workspace should choose its own retention period."))
        .cloned()
        .expect("Manager receives the clarification request");
    assert!(manager_prompt.contains("Treat the answer as explicit product intent"));
    assert!(manager_prompt.contains("Each workspace should choose its own retention period."));
    assert!(manager_prompt.contains("Selected workspace: Each workspace chooses"));
    let captured_planner_prompts = planner_requests.lock().unwrap();
    let planner_prompt = captured_planner_prompts
        .iter()
        .find(|prompt| prompt.contains("Selected option: workspace"))
        .cloned()
        .unwrap_or_else(|| {
            panic!(
                "follow-up Planner receives the persisted answer; captured prompts: {captured_planner_prompts:#?}"
            )
        });
    assert!(planner_prompt.contains("The user chose workspace-specific retention periods."));
    assert!(planner_prompt.contains("Each workspace should choose its own retention period."));
    assert!(planner_prompt.contains("Selected option: workspace"));
    let run =
        crate::core::self_improvement::SelfImprovementStore::new(&project.state.planning_store)
            .load(&run_uid)
            .unwrap();
    assert_eq!(run.phase, original_run_phase);
    assert!(run.clone_uid.is_none());
    assert!(run.plan_approval_hash.is_none());
    assert!(run.independent_plan_review_uid.is_none());
    assert!(project.active_implementations.is_empty());
    let _ = std::fs::remove_dir_all(chat_root);
}
