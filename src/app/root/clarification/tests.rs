use super::*;

#[path = "tests/accepted.rs"]
mod accepted;
#[path = "tests/evidence_resolution.rs"]
mod evidence_resolution;

struct CaptureClarificationHarness(std::sync::Arc<std::sync::Mutex<Vec<String>>>);

impl crate::harness::AiHarness for CaptureClarificationHarness {
    fn label(&self) -> String {
        "clarification fixture".into()
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
            "fixture stops after capture".into(),
        ))
    }
}

fn setup() -> (KooladeApp, crate::core::planning::QuestionV1, String) {
    use crate::core::planning::{
        AcceptanceTestTrace, MATERIAL_AXES, QuestionCategory, QuestionOption, QuestionPriority,
        QuestionProposal, QuestionStore, ReadinessChecklistV1, propose_questions,
    };

    let mut app = crate::app::root::board_tests::fixture();
    let feature_id = "CHG-997";
    let draft = "# CHG-997: Clarification fence\n\n**Status:** Ready\n\n## Intent\n\nTest stale clarification answers.\n\n## Current Behavior\n\nClarification replies can outlive the feature revision they reference.\n\n## Desired Behavior\n\nAnswers apply only to the specification they were written for.\n\n## Scope\n\n- Preserve each clarification against the active specification.\n\n## Affected Product Areas\n\n`product:current-capabilities`\n\n## Requirements\n\n- REQ-1: Answers are bound to the active specification.\n\n## Decisions and Assumptions\n\n- No retention choice recorded yet.\n\n## Acceptance Criteria\n\n- AC-001: Stale answers are rejected.\n";
    let identified = crate::domain::ArtifactIdentity::preserve_markdown(
        draft,
        None,
        feature_id,
        "Clarification fence",
    )
    .unwrap();
    let identity = crate::domain::ArtifactIdentity::from_markdown(&identified)
        .unwrap()
        .unwrap();
    let feature = crate::domain::ChangeMetadata::write_markdown(
        &identified,
        &identity,
        crate::domain::ChangeStatus::Ready,
    )
    .unwrap();
    let Screen::Connected(project) = &mut app.screen else {
        panic!("fixture must be connected")
    };
    crate::artifacts::migration::bootstrap_product_with_store(
        &project.state.repo_root,
        &project.state.planning_store,
        "Clarification fence",
    )
    .unwrap();
    project
        .state
        .planning_store
        .transaction(
            &[(
                "planning/changes/CHG-997-clarification-fence/specification.md".into(),
                feature.as_bytes().to_vec(),
            )],
            None,
        )
        .unwrap();
    project.state.active_features = vec![(feature_id.into(), feature.clone())];
    project.state.active_feature = Some((feature_id.into(), feature.clone()));
    let spec_revision =
        crate::app::root::planner_pass::spec_revision(project, &identity.uid).unwrap();
    let proposal = QuestionProposal {
        feature_uid: identity.uid.clone(),
        plan_revision: 4,
        decision_subject: "active specification".into(),
        decision_domain: "scope".into(),
        origin_requirement_ids: vec!["REQ-1".into()],
        category: QuestionCategory::Scope,
        priority: QuestionPriority::Blocking,
        question: "Does the current specification still cover this requirement?".into(),
        reason: "A stale answer must not be applied to a changed specification.".into(),
        options: vec![
            QuestionOption {
                id: "yes".into(),
                label: "Yes".into(),
                tradeoffs: "The current requirement remains in scope.".into(),
            },
            QuestionOption {
                id: "no".into(),
                label: "No".into(),
                tradeoffs: "The scope needs revision.".into(),
            },
        ],
        recommended_option_id: None,
        no_answer_impact: "Scope remains unresolved.".into(),
        evidence_refs: vec![format!("feature:{}", identity.uid)],
        assumption_if_deferred: None,
        depends_on_question_ids: Vec::new(),
    };
    let question = propose_questions(vec![proposal], &[]).unwrap().remove(0);
    let store = QuestionStore::new(&project.state.planning_store);
    store.save(&question, 0).unwrap();
    store
        .save_readiness(
            &ReadinessChecklistV1 {
                feature_uid: identity.uid.clone(),
                plan_revision: 4,
                spec_revision: Some(spec_revision),
                required_requirement_ids: Some(vec!["REQ-1".into()]),
                assessed_axes: MATERIAL_AXES.to_vec(),
                acceptance_tests: vec![AcceptanceTestTrace {
                    requirement_id: "REQ-1".into(),
                    test_ref: "tests/active-spec.rs".into(),
                }],
                evidence_refs: vec![format!("feature:{}", identity.uid)],
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
    (app, question, feature)
}

#[test]
fn changed_snapshot_replaces_a_stale_running_planner_without_restarting_the_same_snapshot() {
    let (mut app, question, feature) = setup();
    let Screen::Connected(project) = &mut app.screen else {
        panic!("fixture must be connected")
    };
    let changed = feature.replace(
        "REQ-1: Answers are bound to the active specification.",
        "REQ-1: Answers are bound to the active specification and decision history.",
    );
    project
        .state
        .planning_store
        .transaction(
            &[(
                "planning/changes/CHG-997-clarification-fence/specification.md".into(),
                changed.as_bytes().to_vec(),
            )],
            None,
        )
        .unwrap();
    project.state.active_features = vec![("CHG-997".into(), changed.clone())];
    project.state.active_feature = Some(("CHG-997".into(), changed.clone()));
    let revision = crate::app::root::planner_pass::spec_revision_for_markdown(
        project,
        &question.feature_uid,
        "CHG-997",
        &changed,
    )
    .unwrap();
    project.activity.clarification_planner_identity = Some((
        question.feature_uid.clone(),
        question.plan_revision,
        revision,
    ));

    assert_eq!(
        super::planner_for_changed_active_feature(project).unwrap(),
        None,
        "the already-running current-snapshot pass must not duplicate"
    );
    project.activity.clarification_planner_identity = Some((
        question.feature_uid.clone(),
        question.plan_revision,
        "stale-spec-revision".into(),
    ));
    assert_eq!(
        super::planner_for_changed_active_feature(project).unwrap(),
        Some((question.feature_uid, 5)),
        "a new snapshot must replace the stale in-flight pass"
    );
}

#[test]
// Production boundary: dispatching an answer through the connected app must fence
// stale specification revisions before writing question state or starting a turn.
// A unit test of the fence cannot prove the UI command preserves store and turn state.
// Dependencies: in-process app fixture and temporary planning store; no services.
// Setup cost: low, using the standard board fixture.
fn stale_spec_rejects_answer_without_mutating_question_or_starting_a_turn() {
    use crate::core::planning::QuestionStore;

    let (mut app, question, feature) = setup();
    let Screen::Connected(project) = &mut app.screen else {
        panic!("fixture must be connected")
    };
    let updated = format!("{feature}\n\n## Updated intent\n\nThe plan changed.\n");
    project
        .state
        .planning_store
        .transaction(
            &[(
                "planning/changes/CHG-997-clarification-fence/specification.md".into(),
                updated.as_bytes().to_vec(),
            )],
            None,
        )
        .unwrap();
    project.state.active_features[0].1 = updated.clone();
    project.state.active_feature = Some(("CHG-997".into(), updated));
    let before_store_revision = project.state.planning_store.revision().unwrap();
    let before_markdown = project
        .state
        .planning_store
        .read(format!("planning/items/{}.md", question.uid))
        .unwrap();

    app.dispatch_ui_command(crate::ui::ApplicationCommand::AnswerClarification {
        uid: question.uid.clone(),
        expected_revision: question.revision,
        answer: "Apply the previous scope decision".into(),
        selected_option: Some("yes".into()),
    });

    let Screen::Connected(project) = &app.screen else {
        panic!("fixture remains connected")
    };
    let store = &project.state.planning_store;
    let (saved, _) = QuestionStore::new(store).load(&question.uid).unwrap();
    assert_eq!(saved, question);
    assert_eq!(
        store
            .read(format!("planning/items/{}.md", question.uid))
            .unwrap(),
        before_markdown
    );
    assert_eq!(store.revision().unwrap(), before_store_revision);
    assert!(project.active_turn.is_none());
}

#[test]
// Production boundary: two app sessions racing on one durable question must reject
// the stale user's answer after the first answer commits. A unit test of the state
// transition cannot prove the command reloads and fences shared storage correctly.
// Dependencies: in-process app fixture and temporary planning store; no services.
// Setup cost: low, using the standard board fixture.
fn competing_session_answer_rejects_the_stale_question_revision() {
    use crate::core::planning::{QuestionStore, reconcile_answer};

    let (mut app, question, _) = setup();
    let Screen::Connected(project) = &mut app.screen else {
        panic!("fixture must be connected")
    };
    let questions = QuestionStore::new(&project.state.planning_store);
    let (mut remote_answer, record_revision) = questions.load(&question.uid).unwrap();
    let remote_revision = remote_answer.revision;
    let remote_plan_revision = remote_answer.plan_revision;
    reconcile_answer(
        &mut remote_answer,
        remote_revision,
        remote_plan_revision,
        Some("The other session chose yes."),
        Some("yes"),
        "user:other-session",
        1_700_000_000_001,
    )
    .unwrap();
    questions.save(&remote_answer, record_revision).unwrap();

    app.dispatch_ui_command(crate::ui::ApplicationCommand::AnswerClarification {
        uid: question.uid.clone(),
        expected_revision: question.revision,
        answer: "Overwrite the other session".into(),
        selected_option: Some("no".into()),
    });

    let Screen::Connected(project) = &app.screen else {
        panic!("fixture remains connected")
    };
    let (saved, _) = QuestionStore::new(&project.state.planning_store)
        .load(&question.uid)
        .unwrap();
    assert_eq!(saved, remote_answer);
    assert!(project.active_turn.is_none());
    assert!(
        !project
            .task_chats
            .messages
            .contains_key(&format!("clarification:{}", question.uid))
    );
}
