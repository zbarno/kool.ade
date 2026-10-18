use super::*;

struct ReadinessMutatingHarness {
    response: String,
    planning_store: crate::artifacts::planning_store::PlanningStore,
    question: crate::core::planning::QuestionV1,
    calls: Arc<Mutex<usize>>,
}

impl crate::harness::AiHarness for ReadinessMutatingHarness {
    fn label(&self) -> String {
        "readiness-mutating approval fixture".into()
    }

    fn check_available(&self) -> Result<String, crate::error::AppError> {
        Ok(self.label())
    }

    fn execute(
        &self,
        _request: &crate::harness::PlanningRequest,
    ) -> Result<crate::harness::HarnessOutcome, crate::error::AppError> {
        *self.calls.lock().unwrap() += 1;
        crate::core::planning::QuestionStore::new(&self.planning_store)
            .save(&self.question, 0)
            .map_err(|error| crate::error::AppError::Other(error.to_string()))?;
        Ok(crate::harness::HarnessOutcome {
            final_text: self.response.clone(),
            envelope: None,
            stderr_tail: String::new(),
        })
    }
}

fn newer_blocking_question(app: &KooladeApp) -> crate::core::planning::QuestionV1 {
    use crate::core::planning::{
        QuestionCategory, QuestionOption, QuestionPriority, QuestionStatus, QuestionStore,
        QuestionV1,
    };
    let Screen::Connected(project) = &app.screen else {
        panic!("project stays connected")
    };
    let (_, markdown) = project.state.active_feature.as_ref().unwrap();
    let identity = crate::domain::ArtifactIdentity::from_markdown(markdown)
        .unwrap()
        .unwrap();
    let snapshot = crate::core::lifecycle::plan_snapshot::from_feature_with_author(
        &project.state.planning_store,
        "CHG-004",
        &identity,
        markdown,
        &crate::core::workflow::feature_contract(markdown),
    )
    .unwrap();
    let questions = QuestionStore::new(&project.state.planning_store);
    let (readiness, _) = questions.load_readiness(&identity.uid).unwrap().unwrap();
    QuestionV1 {
        uid: uuid::Uuid::new_v4().hyphenated().to_string(),
        feature_uid: identity.uid,
        plan_revision: readiness.plan_revision + 1,
        decision_subject: "new access policy".into(),
        decision_domain: "authorization".into(),
        origin_requirement_ids: snapshot
            .requirements
            .first()
            .map(|requirement| vec![requirement.stable_id.clone()])
            .unwrap_or_default(),
        category: QuestionCategory::Security,
        priority: QuestionPriority::Blocking,
        question: "Who may access the saved searches?".into(),
        reason: "Access policy affects the approved data boundary.".into(),
        options: vec![QuestionOption {
            id: "workspace-members".into(),
            label: "Workspace members".into(),
            tradeoffs: "Requires membership enforcement.".into(),
        }],
        recommended_option_id: Some("workspace-members".into()),
        no_answer_impact: "Task generation must wait for an access decision.".into(),
        evidence_refs: Vec::new(),
        assumption_if_deferred: None,
        deferred: None,
        depends_on_question_ids: Vec::new(),
        status: QuestionStatus::Open,
        answer_text: None,
        selected_option: None,
        answered_by: None,
        answered_at_ms: None,
        deferred_by: None,
        deferred_at_ms: None,
        revision: 0,
    }
}

fn save_newer_blocking_question(app: &KooladeApp) {
    let Screen::Connected(project) = &app.screen else {
        panic!("project stays connected")
    };
    let question = newer_blocking_question(app);
    crate::core::planning::QuestionStore::new(&project.state.planning_store)
        .save(&question, 0)
        .unwrap();
}

#[test]
// Production boundary: task-generation must recheck the persisted clarification
// ledger before opening a run, calling the decomposer, or writing task artifacts.
// A validator unit test cannot prove the generation entrypoint enforces the gate.
// Dependencies: temporary Git repository and managed-local planning store; no
// external services. Setup cost: low, using the existing approval fixture.
fn newer_blocking_clarification_stops_task_generation_before_model_or_artifacts() {
    let _shield = crate::core::gitops::test_support::shield("stale-readiness-task-generation");
    let (mut app, root, _) = setup();
    approve(&mut app);
    save_newer_blocking_question(&app);
    let baseline = task_files(&root);
    let (harness, calls) = harness(Vec::new());
    let request = crate::harness::PlanningRequest {
        mode: crate::harness::ExecutionMode::TaskGeneration,
        task_id: None,
        execution_workspace: None,
        destination_sha: None,
        authorization_verifier: None,
        prompt_manifest: None,
        reasoning_level: "medium".into(),
        telemetry_phase: None,
        repo_root: root.clone(),
        runtime_config_source: None,
        prompt_body: "task generation fixture".into(),
        system_instructions: String::new(),
        timeout: Duration::from_secs(2),
        progress_tx: std::sync::mpsc::channel().0,
        cancel: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        review_identity: None,
    };
    let Screen::Connected(project) = &app.screen else {
        panic!("project stays connected")
    };
    let error = crate::core::task_generation::generate(
        harness.as_ref(),
        &request,
        &project.state,
        Instant::now(),
    )
    .err()
    .expect("stale readiness blocks task generation");
    assert!(error.to_string().contains("readiness is stale"));
    assert!(calls.lock().unwrap().is_empty());
    assert_eq!(task_files(&root), baseline);
    drop(app);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
// Production boundary: task publication must recheck readiness after the
// decomposer returns, since a blocking clarification can arrive mid-run. A
// validator unit test cannot prove the generation transaction refuses output.
// Dependencies: temporary Git/planning-store fixture; no external services.
// Setup cost: low, with one synthetic decomposer response.
fn newer_blocking_clarification_during_decomposition_prevents_publication() {
    let _shield = crate::core::gitops::test_support::shield("readiness-change-during-generation");
    let (mut app, root, _) = setup();
    approve(&mut app);
    let key = generation_key(&app);
    let response = graph_response(&app);
    let question = newer_blocking_question(&app);
    let Screen::Connected(project) = &app.screen else {
        panic!("project stays connected")
    };
    let planning_store = project.state.planning_store.clone();
    let calls = Arc::new(Mutex::new(0));
    app.task_harness = Some(Box::new(ReadinessMutatingHarness {
        response,
        planning_store,
        question,
        calls: calls.clone(),
    }));
    let baseline = task_files(&root);

    app.start_task_generation(&key, "CHG-004");
    assert!(!finish(&mut app));
    assert_eq!(*calls.lock().unwrap(), 1);
    assert_eq!(task_files(&root), baseline);
    drop(app);
    std::fs::remove_dir_all(root).unwrap();
}
