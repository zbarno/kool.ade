use super::*;

struct PlannerResponseHarness {
    response: String,
    prompts: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
}

impl crate::harness::AiHarness for PlannerResponseHarness {
    fn label(&self) -> String {
        "evidence resolution fixture".into()
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
            final_text: format!("```json\n{}\n```", self.response),
            envelope: None,
            stderr_tail: String::new(),
        })
    }
}

#[test]
// Production boundary: a real async Planner pass must persist a repository-backed
// technical resolution and its specification decision atomically, without
// representing it as a user answer. Unit tests cannot prove prompt evidence,
// app scheduling, schema validation, and the durable specification transaction together.
// Dependencies: standard in-process app fixture and temporary planning store;
// no external services. Setup cost: low; one deterministic in-process harness.
fn evidence_resolved_question_and_spec_decision_are_saved_together() {
    use crate::core::planning::{QuestionCategory, QuestionStatus, QuestionStore};
    use crate::harness::prompt_catalog::{
        AcceptanceTestRefV1, PlannerOutputV2, ReadinessV2, TechnicalResolutionV1,
    };

    let (mut app, question, _) = setup();
    let Screen::Connected(project) = &mut app.screen else {
        panic!("fixture must be connected")
    };
    std::fs::write(
        project.state.repo_root.join("README.md"),
        "API responses use the established JSON response envelope.\n",
    )
    .unwrap();
    let questions = QuestionStore::new(&project.state.planning_store);
    let (mut technical, record_revision) = questions.load(&question.uid).unwrap();
    technical.category = QuestionCategory::Api;
    technical.decision_subject = "API response envelope".into();
    technical.decision_domain = "API response format".into();
    technical.question = "Which JSON envelope should the API use?".into();
    technical.revision += 1;
    questions.save(&technical, record_revision).unwrap();

    let decision = "Keep the existing JSON response envelope";
    let rationale = "The repository README establishes the existing JSON response envelope.";
    let output = PlannerOutputV2 {
        proposed_questions: vec![],
        resolved_questions: vec![TechnicalResolutionV1 {
            question_uid: question.uid.clone(),
            decision: decision.into(),
            rationale: rationale.into(),
            selected_option_id: None,
            evidence_refs: vec![format!("repository-overview:{}", question.feature_uid)],
        }],
        draft_changes: vec![],
        readiness: ReadinessV2 {
            ready: true,
            blockers: vec![],
            evidence: vec![format!("feature:{}", question.feature_uid)],
            assessed_axes: crate::core::planning::MATERIAL_AXES
                .iter()
                .map(|axis| format!("{axis:?}"))
                .collect(),
            acceptance_tests: vec![AcceptanceTestRefV1 {
                requirement_id: "REQ-1".into(),
                test_ref: "tests/active-spec.rs".into(),
            }],
        },
    };
    let prompts = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    app.task_harness = Some(Box::new(PlannerResponseHarness {
        response: serde_json::to_string(&output).unwrap(),
        prompts: prompts.clone(),
    }));
    app.start_clarification_planner(&question.feature_uid, question.plan_revision);
    if let Screen::Connected(project) = &mut app.screen {
        // Keep unrelated background patrol from claiming the test harness.
        project.activity.last_update = Some(std::time::Instant::now());
    }

    let context = egui::Context::default();
    context.data_mut(|data| {
        data.insert_temp(
            egui::Id::new("koolade_selected_planning"),
            "clarification-evidence-fixture".to_owned(),
        );
    });
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        app.tick(0.016, &context);
        let complete = match &app.screen {
            Screen::Connected(project) => {
                let questions = QuestionStore::new(&project.state.planning_store);
                questions
                    .load(&question.uid)
                    .is_ok_and(|(saved, _)| saved.status == QuestionStatus::EvidenceResolved)
                    && project.activity.clarification_planner.is_none()
                    && project
                        .state
                        .active_feature
                        .as_ref()
                        .is_some_and(|(_, markdown)| markdown.contains(decision))
            }
            _ => false,
        };
        if complete {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "async Planner did not persist the technical resolution and feature update"
        );
        std::thread::sleep(std::time::Duration::from_millis(5));
    }

    let Screen::Connected(project) = &app.screen else {
        panic!("fixture remains connected")
    };
    let (resolved, _) = QuestionStore::new(&project.state.planning_store)
        .load(&question.uid)
        .unwrap();
    let prompts = prompts.lock().unwrap();
    assert_eq!(prompts.len(), 1, "the clarification Planner ran once");
    assert!(prompts[0].contains(&format!("repository-overview:{}", question.feature_uid)));
    assert!(prompts[0].contains("API responses use the established JSON response envelope."));
    assert!(prompts[0].contains("Which JSON envelope should the API use?"));
    assert_eq!(resolved.status, QuestionStatus::EvidenceResolved);
    assert_eq!(
        resolved.answered_by.as_deref(),
        Some("Man.ager (repository evidence)")
    );
    assert_eq!(resolved.selected_option, None);
    assert!(resolved.answer_text.as_deref().unwrap().contains(rationale));
    assert!(
        resolved
            .evidence_refs
            .contains(&format!("repository-overview:{}", question.feature_uid))
    );
    let feature = project
        .state
        .active_feature
        .as_ref()
        .map(|(_, markdown)| markdown)
        .expect("resolved feature remains active");
    assert!(feature.contains(decision));
    assert!(feature.contains(rationale));
    assert!(feature.contains(&format!("repository-overview:{}", question.feature_uid)));
    let (readiness, _) = QuestionStore::new(&project.state.planning_store)
        .load_readiness(&question.feature_uid)
        .unwrap()
        .expect("updated readiness is durable");
    assert_eq!(
        readiness.spec_revision.as_deref(),
        Some(
            crate::app::root::planner_pass::spec_revision(project, &question.feature_uid)
                .unwrap()
                .as_str()
        )
    );
}
