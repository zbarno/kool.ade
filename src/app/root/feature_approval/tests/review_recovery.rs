use super::*;
use std::sync::{Arc, Mutex};

struct ManagerHarness {
    prompt: Arc<Mutex<Option<String>>>,
    response: String,
}

impl crate::harness::AiHarness for ManagerHarness {
    fn label(&self) -> String {
        "manager review fixture".into()
    }

    fn check_available(&self) -> Result<String, crate::error::AppError> {
        Ok(self.label())
    }

    fn execute(
        &self,
        request: &crate::harness::PlanningRequest,
    ) -> Result<crate::harness::HarnessOutcome, crate::error::AppError> {
        *self.prompt.lock().unwrap() = Some(request.prompt_body.clone());
        Ok(crate::harness::HarnessOutcome {
            final_text: self.response.clone(),
            envelope: None,
            stderr_tail: String::new(),
        })
    }
}

#[path = "review_recovery/arbitration_worker.rs"]
mod arbitration_worker;
#[path = "review_recovery/manager_repair.rs"]
mod manager_repair;
#[path = "review_recovery/receipts.rs"]
mod receipts;

#[test]
fn independent_review_evidence_includes_plan_readiness_and_relevant_product_specs() {
    // Production boundary: the persisted plan, readiness assessment, and referenced product module are assembled into the independent-review prompt evidence.
    // Invariant: reviewer evidence is current and citable; serializer unit tests cannot prove the planning-store selection and workspace scoping.
    // External dependencies: none. Setup cost: one temporary Git repository and planning-store fixture.
    let _shield = crate::core::gitops::test_support::shield("plan-review-evidence-manifest");
    let (app, root, _) = setup();
    let snapshot = app.feature_actions(None)[0]
        .plan_snapshot
        .clone()
        .expect("ready plan has a snapshot");
    let Screen::Connected(project) = &app.screen else {
        panic!("project remains connected")
    };
    let evidence = crate::app::root::plan_review::evidence::collect(project, &snapshot).unwrap();
    assert_eq!(
        evidence.decision_refs,
        vec![format!("readiness:{}", snapshot.feature_uid)]
    );
    assert!(
        evidence
            .items
            .iter()
            .any(|item| item.id == format!("plan:{}", snapshot.uid))
    );
    assert!(
        evidence
            .items
            .iter()
            .any(|item| item.id == format!("readiness:{}", snapshot.feature_uid))
    );
    assert!(
        evidence
            .items
            .iter()
            .any(|item| item.id == "product-spec:current-capabilities")
    );
    assert!(
        evidence
            .items
            .iter()
            .all(|item| item.workspace_uid == project.state.planning_store.project_id.to_string())
    );
    drop(app);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn manager_receives_persisted_plan_findings_and_returns_a_typed_disposition() {
    // Production boundary: PlanningStore review record flows through Manager prompt rendering and strict output validation.
    // Invariant: every persisted finding is available as cited evidence and the Manager must return a typed disposition; a serializer unit test cannot prove catalog wiring.
    // External dependencies: none. Setup cost: one temporary Git repository and PlanningStore fixture.
    let _shield = crate::core::gitops::test_support::shield("manager-plan-review-findings");
    let (mut app, root, _) = setup();
    record_test_plan_author(&mut app, "CHG-004");
    app.approve_feature_from_board("CHG-004");
    record_test_plan_review(
        &mut app,
        "CHG-004",
        crate::core::lifecycle::GateDecision::ChangesRequested,
    );
    let Screen::Connected(project) = &app.screen else {
        panic!("project remains connected")
    };
    let (snapshot, requirement) = {
        let (_, markdown) = project
            .state
            .active_features
            .iter()
            .find(|(id, _)| id == "CHG-004")
            .unwrap();
        let identity = crate::domain::ArtifactIdentity::from_markdown(markdown)
            .unwrap()
            .unwrap();
        let contract = workflow::feature_contract(markdown);
        let snapshot = crate::core::lifecycle::plan_snapshot::from_feature_with_author(
            &project.state.planning_store,
            "CHG-004",
            &identity,
            markdown,
            &contract,
        )
        .unwrap();
        (snapshot.clone(), snapshot.requirements[0].stable_id.clone())
    };
    let evidence_id = format!("plan-review:{}", snapshot.uid);
    let prompt = Arc::new(Mutex::new(None));
    let response = serde_json::json!({
        "summary": "Repair the technical wording while preserving approved intent.",
        "proposed_actions": [{
            "kind": "repair_plan",
            "target": "CHG-004#TEST-FINDING",
            "reason": "The reviewer found a missing requirement detail.",
            "evidence": [evidence_id],
            "expected_revision": snapshot.content_hash
        }],
        "opportunities": []
    })
    .to_string();
    let manager = crate::app::manager::Manager::start(
        project,
        &["independent plan review requested changes".into()],
        Box::new(ManagerHarness {
            prompt: prompt.clone(),
            response,
        }),
    );
    let deadline = Instant::now() + Duration::from_secs(10);
    let outcome = loop {
        if let Some(result) = manager.result() {
            break result.expect("manager validates a finding disposition");
        }
        assert!(Instant::now() < deadline, "manager review triage timed out");
        std::thread::sleep(Duration::from_millis(5));
    };
    let manager_prompt = prompt.lock().unwrap().clone().unwrap();
    assert!(manager_prompt.contains(&evidence_id));
    assert!(manager_prompt.contains("Clarify the missing requirement detail."));
    assert!(manager_prompt.contains(&requirement));
    assert!(outcome.summary.contains("RepairPlan"));
    assert_eq!(outcome.proposed_actions.len(), 1);
    let repair_key = {
        let Screen::Connected(project) = &mut app.screen else {
            panic!("project remains connected")
        };
        let stale_revision = "f".repeat(64);
        let stale_messages = crate::app::manager::persist_plan_dispositions(
            project,
            &outcome.proposed_actions,
            &outcome.evidence_hashes,
            &stale_revision,
        )
        .unwrap();
        assert_eq!(stale_messages.len(), 1);
        assert!(stale_messages[0].contains("planning state changed"));
        assert!(
            project
                .planning_work
                .iter()
                .all(|work| { !work.key.starts_with("task:manager-plan-repair:") })
        );
        let mut stale_hashes = outcome.evidence_hashes.clone();
        stale_hashes.insert(evidence_id.clone(), "f".repeat(64));
        let stale_messages = crate::app::manager::persist_plan_dispositions(
            project,
            &outcome.proposed_actions,
            &stale_hashes,
            &outcome.planning_revision,
        )
        .unwrap();
        assert_eq!(stale_messages.len(), 1);
        assert!(stale_messages[0].contains("cited evidence changed"));
        assert!(
            !project
                .planning_work
                .iter()
                .any(|work| work.key.starts_with("task:manager-plan-repair:")),
            "a disposition with a stale evidence hash cannot create repair work"
        );
        let messages = crate::app::manager::persist_plan_dispositions(
            project,
            &outcome.proposed_actions,
            &outcome.evidence_hashes,
            &outcome.planning_revision,
        )
        .unwrap();
        assert!(messages.is_empty());
        let repair = project
            .planning_work
            .iter()
            .find(|work| work.key.starts_with("task:manager-plan-repair:"))
            .expect("Manager repair is visible as persisted planning work");
        assert!(
            repair
                .detail
                .contains("Clarify the missing requirement detail.")
        );
        assert!(repair.detail.contains(&snapshot.content_hash));
        let repair_key = repair.key.clone();
        let cards = crate::core::planning_work::cards(&project.state, &project.planning_work);
        assert!(cards.iter().any(|work| work.key == repair_key));
        assert!(cards.iter().any(|work| {
            work.feature_uid.as_deref() == Some(snapshot.feature_uid.as_str())
                && work.kind == crate::core::planning_work::WorkKind::Feature
        }));
        let thread_context = crate::core::planning_work::context(&project.state, &repair_key)
            .expect("Manager disposition has a standard task conversation context");
        assert!(thread_context.contains("Plan review"));
        assert!(thread_context.contains("CHG-004"));
        let card_count = project.planning_work.len();
        crate::app::manager::persist_plan_dispositions(
            project,
            &outcome.proposed_actions,
            &outcome.evidence_hashes,
            &outcome.planning_revision,
        )
        .unwrap();
        assert_eq!(
            project.planning_work.len(),
            card_count,
            "replay is idempotent"
        );
        let mut stale_action = outcome.proposed_actions[0].clone();
        stale_action.expected_revision = "f".repeat(64);
        let messages = crate::app::manager::persist_plan_dispositions(
            project,
            &[stale_action],
            &outcome.evidence_hashes,
            &outcome.planning_revision,
        )
        .unwrap();
        assert_eq!(messages.len(), 1);
        assert_eq!(project.planning_work.len(), card_count);
        project.activity.pending.clear();
        project.activity.last_update = Some(Instant::now());
        repair_key
    };
    let (planner, _) = harness(vec!["malformed planner response".into()]);
    app.task_harness = Some(planner);
    app.tick(0.016, &egui::Context::default());
    assert!(matches!(&app.screen, Screen::Connected(project)
    if project.active_turn.is_some()
        && project.active_planning_work.as_deref() == Some(repair_key.as_str())
        && project.planning_work.iter().any(|work| {
            work.key == repair_key
                && work.status == crate::core::planning_work::WorkStatus::InProgress
        })));
    assert!(
        !finish(&mut app),
        "fixture stops before any unvalidated repair"
    );
    drop(app);
    std::fs::remove_dir_all(root).unwrap();
}
