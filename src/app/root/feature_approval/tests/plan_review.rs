use super::*;
use crate::{
    error::AppError,
    harness::{AiHarness, ExecutionMode, HarnessOutcome, PlanningRequest},
};
use std::sync::{Arc, Mutex};

#[path = "plan_review/lifecycle.rs"]
mod lifecycle;

#[derive(Default)]
struct ReviewObservation {
    mode: Option<ExecutionMode>,
    task_id: Option<String>,
    execution_id: Option<String>,
    session_id: Option<String>,
    subject_hash: Option<String>,
}

struct PlanReviewerHarness(Arc<Mutex<ReviewObservation>>);

impl AiHarness for PlanReviewerHarness {
    fn label(&self) -> String {
        "isolated plan reviewer fixture".into()
    }

    fn check_available(&self) -> Result<String, AppError> {
        Ok(self.label())
    }

    fn execute(&self, request: &PlanningRequest) -> Result<HarnessOutcome, AppError> {
        let identity = request
            .review_identity
            .as_ref()
            .expect("app supplies a fresh reviewer identity");
        let typed_request = request
            .prompt_body
            .split_once("TYPED TASK REQUEST (JSON DATA)\n")
            .and_then(|(_, rest)| rest.split_once("\n\nQUOTED EVIDENCE").map(|(json, _)| json))
            .expect("review prompt contains the typed request");
        let typed_request: serde_json::Value = serde_json::from_str(typed_request).unwrap();
        let review = &typed_request["payload"]["request"];
        let subject_hash = review["subject_hash"].as_str().unwrap().to_owned();
        let evidence_id = typed_request["evidence_manifest"][0]["id"]
            .as_str()
            .expect("review manifest contains the approved plan evidence")
            .to_owned();
        let requirements = review["requirements_matrix"].as_array().unwrap();
        assert!(!requirements.is_empty(), "review has approved requirements");
        let coverage = requirements
            .iter()
            .map(|entry| {
                let requirement = entry
                    .as_str()
                    .unwrap()
                    .split_once(':')
                    .expect("requirement matrix starts with its stable ID")
                    .0;
                serde_json::json!({
                    "requirement": requirement,
                    "status": "pass",
                    "evidence": [evidence_id]
                })
            })
            .collect::<Vec<_>>();
        let final_text = serde_json::json!({
            "subject_hash": subject_hash,
            "pass": true,
            "outcome": "pass",
            "coverage": coverage,
            "findings": [],
            "untested": []
        })
        .to_string();
        *self.0.lock().unwrap() = ReviewObservation {
            mode: Some(request.mode),
            task_id: request.task_id.clone(),
            execution_id: Some(identity.execution_id.clone()),
            session_id: Some(identity.session_id.clone()),
            subject_hash: Some(subject_hash),
        };
        Ok(HarnessOutcome {
            final_text,
            envelope: None,
            stderr_tail: String::new(),
        })
    }
}

/// Production boundary: the app must persist an explicit plan confirmation before invoking an
/// independent reviewer, then bind its accepted verdict to that same snapshot. A receipt unit test
/// cannot prove this dispatch and result-polling path. Uses a local Git fixture and deterministic
/// in-process harness; no provider, network, Docker, or live project is involved. Setup cost is one
/// temporary approval fixture.
#[test]
fn user_confirmation_runs_independent_plan_reviewer_and_persists_its_exact_verdict() {
    let _shield = crate::core::gitops::test_support::shield("approval-plan-review-e2e");
    let (mut app, root, _) = setup();
    prepare_reviewable_plan(&mut app, &root);
    let observation = Arc::new(Mutex::new(ReviewObservation::default()));
    app.approve_feature_action_with_harness_for_test(
        "CHG-004",
        Box::new(PlanReviewerHarness(observation.clone())),
    );

    let snapshot = {
        let Screen::Connected(project) = &app.screen else {
            panic!("approval fixture remains connected")
        };
        let (_, markdown) = project
            .state
            .active_features
            .iter()
            .find(|(feature_id, _)| feature_id == "CHG-004")
            .unwrap();
        let identity = crate::domain::ArtifactIdentity::from_markdown(markdown)
            .unwrap()
            .unwrap();
        crate::core::lifecycle::plan_snapshot::from_feature_with_author(
            &project.state.planning_store,
            "CHG-004",
            &identity,
            markdown,
            &crate::core::workflow::feature_contract(markdown),
        )
        .unwrap()
    };
    let Screen::Connected(project) = &app.screen else {
        unreachable!()
    };
    assert!(
        crate::core::lifecycle::plan_snapshot::has_current_user_confirmation(
            &project.state.planning_store,
            &snapshot
        )
        .unwrap()
    );
    assert!(
        crate::core::lifecycle::plan_snapshot::plan_review_receipt(
            &project.state.planning_store,
            &snapshot
        )
        .unwrap()
        .is_none()
    );
    assert_eq!(
        project.activity.plan_review_feature.as_deref(),
        Some("CHG-004"),
        "review did not start; activity: {:?}",
        project.activity.pending
    );

    let context = egui::Context::default();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
    loop {
        app.tick(0.016, &context);
        let reviewing = matches!(
            &app.screen,
            Screen::Connected(project) if project.activity.plan_review.is_some()
        );
        if !reviewing {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "review worker completes in budget"
        );
        std::thread::sleep(std::time::Duration::from_millis(5));
    }

    let Screen::Connected(project) = &app.screen else {
        unreachable!()
    };
    let receipt = crate::core::lifecycle::plan_snapshot::plan_review_receipt(
        &project.state.planning_store,
        &snapshot,
    )
    .unwrap()
    .unwrap_or_else(|| {
        panic!(
            "review receipt persisted; activity: {:?}; last chat: {:?}; approved: {}",
            project.activity.pending,
            app.chat_messages().last(),
            crate::core::workflow::feature_approved(
                &project.state.planning_store,
                &project.state.workflow,
                "CHG-004"
            )
        )
    });
    let verdict = crate::core::lifecycle::plan_snapshot::plan_review_verdict(
        &project.state.planning_store,
        &snapshot,
    )
    .unwrap()
    .expect("review verdict persisted");
    let observation = observation.lock().unwrap();
    assert_eq!(observation.mode, Some(ExecutionMode::ReadOnlyAnalysis));
    assert!(observation.task_id.is_none());
    assert_ne!(receipt.reviewer_run_id, receipt.author_run_id);
    assert_ne!(receipt.reviewer_session_id, receipt.author_session_id);
    assert_eq!(
        receipt.reviewer_run_id.as_deref(),
        observation.execution_id.as_deref()
    );
    assert_eq!(
        receipt.reviewer_session_id.as_deref(),
        observation.session_id.as_deref()
    );
    assert_eq!(verdict.subject_hash, snapshot.content_hash);
    assert!(verdict.pass);
    assert_eq!(
        observation.subject_hash.as_deref(),
        Some(snapshot.content_hash.as_str())
    );
    let (task_key, status, blocked_detail) = project
        .planning_work
        .iter()
        .find(|work| work.kind == crate::core::planning_work::WorkKind::TaskGeneration)
        .map(|work| (work.key.clone(), work.status, work.detail.clone()))
        .expect("approved feature retains a task-generation card");
    assert_eq!(status, crate::core::planning_work::WorkStatus::Todo);
    assert_eq!(
        blocked_detail,
        "Approved: the independent PlanReviewer approved this exact plan snapshot. Task generation starts automatically once required repository Git intents are approved."
    );
    drop(observation);
    assert!(matches!(
        &app.screen,
        Screen::Connected(project) if project.active_turn.is_none()
    ));
    assert!(!task_key.is_empty());
    drop(app);
    let reloaded = crate::core::state::PlannerState::load(&root).unwrap();
    lifecycle::assert_review_reconciled(&reloaded.planning_store, &reloaded.workflow, &snapshot);
    std::fs::remove_dir_all(root).unwrap();
}

fn prepare_reviewable_plan(app: &mut KooladeApp, root: &std::path::Path) {
    let published_path = {
        let Screen::Connected(project) = &app.screen else {
            panic!("approval fixture remains connected")
        };
        let (_, markdown) = project
            .state
            .active_features
            .iter()
            .find(|(id, _)| id == "CHG-004")
            .unwrap();
        let identity = crate::domain::ArtifactIdentity::from_markdown(markdown)
            .unwrap()
            .unwrap();
        root.join("docs/koolade/features")
            .join(format!("{}.md", identity.uid))
    };
    let spec = std::fs::read_to_string(&published_path).unwrap();
    let spec = spec
        .replace(
            "## Requirements\n\nPersist saved searches and restore them after restarting.",
            "## Requirements\n\n- REQ-001: Persist saved searches across restarts. Rationale: users need their searches after reopening the app. ACs: AC-001",
        )
        .replace(
            "## Acceptance Criteria\n\nPersist saved searches and restore them after restarting.",
            "## Acceptance Criteria\n\n- AC-001: A saved search is present after restarting the app.",
        );
    std::fs::write(&published_path, &spec).unwrap();
    assert!(
        std::process::Command::new("git")
            .args(["add", "docs/koolade/features"])
            .current_dir(root)
            .status()
            .unwrap()
            .success()
    );
    assert!(
        std::process::Command::new("git")
            .args([
                "commit",
                "-q",
                "-m",
                "Publish structured approval test feature"
            ])
            .current_dir(root)
            .status()
            .unwrap()
            .success()
    );

    if let Screen::Connected(project) = &mut app.screen {
        project.state = crate::core::state::PlannerState::load(root).unwrap();
    } else {
        panic!("approval fixture remains connected")
    }
    ensure_test_plan_adopted(app, "CHG-004");
    record_test_plan_author(app, "CHG-004");
    let Screen::Connected(project) = &mut app.screen else {
        panic!("approval fixture remains connected")
    };
    let (_, adopted_markdown) = project
        .state
        .active_features
        .iter()
        .find(|(id, _)| id == "CHG-004")
        .unwrap();
    let identity = crate::domain::ArtifactIdentity::from_markdown(adopted_markdown)
        .unwrap()
        .unwrap();
    let mut work = crate::core::planning_work::Work::new(
        "feature-work:CHG-004".into(),
        "Plan saved searches".into(),
        "Keep saved searches available after restart.".into(),
        "Approved feature planning work.".into(),
    );
    work.kind = crate::core::planning_work::WorkKind::TaskGeneration;
    work.status = crate::core::planning_work::WorkStatus::Todo;
    work.key = format!("task-generation:{}", work.uid);
    work.feature_id = Some("CHG-004".into());
    work.feature_uid = Some(identity.uid);
    project.planning_work.push(work);
    project.state.baseline_planning_revision = project.state.planning_store.revision().unwrap();
    project.save_planning_work().unwrap();
}
