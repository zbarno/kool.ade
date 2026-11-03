use super::paint;
use crate::{
    core::{
        lifecycle::{
            CurrentImplementationEvidenceV1, DeploymentEvidenceV1, FeatureGoal, FeaturePhase,
            TaskPhase, TaskRecord,
        },
        planning_work::{Work, WorkKind, WorkStatus},
    },
    ui::planning_board::ViewModel,
};

fn root_and_board() -> (Work, ViewModel) {
    let feature_uid = uuid::Uuid::new_v4().hyphenated().to_string();
    let mut root = Work::new(
        "task:root-intent".into(),
        "Ship the request".into(),
        "Refine the request during planning".into(),
        "Planning complete".into(),
    );
    root.kind = WorkKind::Feature;
    root.status = WorkStatus::Done;
    root.original_goal = Some("Preserve this original goal".into());
    root.feature_uid = Some(feature_uid.clone());
    root.task_intent_uid = Some(root.uid.clone());
    let mut child = Work::new(
        "task:child-work".into(),
        "Implement the request".into(),
        "Implement the approved behavior".into(),
        "Implementation finished".into(),
    );
    child.status = WorkStatus::InProgress;
    child.parent_uid = Some(root.uid.clone());
    child.task_intent_uid = Some(root.uid.clone());

    let mut board = ViewModel {
        planning_work: vec![root.clone(), child],
        lifecycle_features: vec![FeatureGoal {
            schema_version: 1,
            feature_uid: feature_uid.clone(),
            original_intent: "Preserve this original goal".into(),
            draft: "Approved specification".into(),
            scope: Vec::new(),
            non_goals: Vec::new(),
            revision: 2,
            phase: FeaturePhase::Complete,
            approved_plan_sha256: Some("a".repeat(64)),
            plan_snapshot_uid: None,
            plan_snapshot_sha256: None,
            unresolved_questions: Vec::new(),
            decision_refs: Vec::new(),
            review_refs: Vec::new(),
            readiness_evidence_refs: Vec::new(),
            task_uids: Vec::new(),
        }],
        lifecycle_feature_tasks: vec![TaskRecord {
            schema_version: 1,
            task_uid: uuid::Uuid::new_v4().hyphenated().to_string(),
            feature_uid: feature_uid.clone(),
            planning_item_uid: Some("child-work-uid".into()),
            task_spec_sha256: None,
            parent_task_uid: None,
            subtask_uids: Vec::new(),
            dependency_task_uids: Vec::new(),
            approved_requirement_ids: Vec::new(),
            phase: TaskPhase::PrMerged,
            revision: 2,
            approved_plan_sha256: "a".repeat(64),
            implementation_sha256: None,
            reviewed_code_head_sha: None,
            reviewed_code_subject_sha256: None,
            code_review: None,
            review_history: Vec::new(),
            repair_attempts: 0,
            runtime_qa: None,
            qa_execution: None,
            spec_reconciliation: None,
            spec_reconciliation_blocker: None,
            spec_reconciliation_proposal_sha256: None,
            spec_reconciliation_spec_sha256: None,
            spec_reconciliation_proposal_json: None,
            pr_work_item_uid: None,
        }],
        ..ViewModel::default()
    };
    let current = CurrentImplementationEvidenceV1 {
        schema_version: 1,
        uid: uuid::Uuid::new_v4().hyphenated().to_string(),
        feature_uid: feature_uid.clone(),
        task_uid: board.lifecycle_feature_tasks[0].task_uid.clone(),
        work_item_uid: uuid::Uuid::new_v4().hyphenated().to_string(),
        repository_id: "synthetic-repo".into(),
        base_commit_sha: "b".repeat(40),
        pull_request_head_sha: "c".repeat(40),
        merge_commit_sha: "d".repeat(40),
        checked_out_head_sha: "d".repeat(40),
        specification_revision: 2,
        specification_hash: "e".repeat(64),
        revision: 3,
        recorded_at_ms: 10,
    };
    let deployment = DeploymentEvidenceV1 {
        schema_version: 1,
        feature_uid: feature_uid.clone(),
        current_implementation_revision: current.revision,
        specification_revision: current.specification_revision,
        specification_hash: current.specification_hash.clone(),
        merge_commit_sha: current.merge_commit_sha.clone(),
        repository_id: current.repository_id.clone(),
        deployment_id: Some(1),
        status_id: Some(1),
        environment: Some("production".into()),
        production_environment: true,
        state: Some("success".into()),
        observed_at_ms: 11,
        revision: 1,
    };
    board
        .lifecycle_deployment_evidence
        .insert(feature_uid, (deployment, current));
    (root, board)
}

fn frame(
    context: &egui::Context,
    input: egui::RawInput,
    board: &ViewModel,
    root: &Work,
) -> egui::FullOutput {
    context.run_ui(input, |context| {
        egui::CentralPanel::default().show(context, |ui| paint(ui, None, board, root));
    })
}

fn viewport(events: Vec<egui::Event>) -> egui::RawInput {
    egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(760.0, 640.0),
        )),
        events,
        ..Default::default()
    }
}

fn button_center(output: &egui::FullOutput, label: &str) -> egui::Pos2 {
    output
        .shapes
        .iter()
        .find_map(|shape| match &shape.shape {
            egui::Shape::Text(text) if text.galley.text() == label => {
                Some(text.pos + text.galley.size() / 2.0)
            }
            _ => None,
        })
        .unwrap_or_else(|| panic!("button label {label:?} was not rendered"))
}

fn click(context: &egui::Context, board: &ViewModel, root: &Work, position: egui::Pos2) {
    for pressed in [true, false] {
        let mut output = frame(
            context,
            viewport(vec![
                egui::Event::PointerMoved(position),
                egui::Event::PointerButton {
                    pos: position,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: Default::default(),
                },
            ]),
            board,
            root,
        );
        output.textures_delta.clear();
    }
}

fn rendered_text(output: &egui::FullOutput) -> Vec<String> {
    output
        .shapes
        .iter()
        .filter_map(|shape| match &shape.shape {
            egui::Shape::Text(text) => Some(text.galley.text().to_owned()),
            _ => None,
        })
        .collect()
}

/// Production UI boundary: TaskIntent details must derive its badge from the
/// board's current deployment receipt and current-implementation evidence.
/// A core projection test cannot prove the actual details badge uses those
/// records. This renders synthetic in-memory state with no external services.
#[test]
fn details_badge_shows_deployed_only_for_current_production_evidence() {
    let (root, mut board) = root_and_board();
    board.planning_work[1].status = WorkStatus::Done;
    let context = egui::Context::default();
    let mut output = frame(&context, viewport(Vec::new()), &board, &root);
    let rendered = rendered_text(&output);
    assert!(rendered.iter().any(|text| text == "Deployed"));
    assert!(
        rendered
            .iter()
            .any(|text| text == "The requested outcome is deployed.")
    );
    output.textures_delta.clear();

    let (_, current) = board
        .lifecycle_deployment_evidence
        .values_mut()
        .next()
        .expect("fixture includes deployment evidence");
    current.revision += 1;
    let mut stale_output = frame(&context, viewport(Vec::new()), &board, &root);
    let rendered = rendered_text(&stale_output);
    assert!(rendered.iter().any(|text| text == "Merged"));
    assert!(!rendered.iter().any(|text| text == "Deployed"));
    stale_output.textures_delta.clear();
}

/// Production UI boundary: clicking a Task Map child must preserve the root
/// conversation key and route back to it from child details. Model-only tests
/// cannot exercise egui click handling or its temporary navigation state.
/// Uses synthetic Work records and in-memory egui input only.
#[test]
fn task_map_child_navigation_records_and_restores_root_conversation_key() {
    let (root, board) = root_and_board();
    let child_key = board.planning_work[1].key.clone();
    let root_key = root.key.clone();
    let context = egui::Context::default();
    let mut first = frame(&context, viewport(Vec::new()), &board, &root);
    let child_button = button_center(&first, "Implement the request · In progress");
    first.textures_delta.clear();
    click(&context, &board, &root, child_button);

    let return_key_id = egui::Id::new("koolade_task_relationship_intent_return");
    let child_navigation_id = egui::Id::new("koolade_task_relationship_planning_navigation");
    assert_eq!(
        context.data_mut(|data| data.get_temp::<String>(return_key_id)),
        Some(root_key.clone())
    );
    assert_eq!(
        context.data_mut(|data| data.get_temp::<String>(child_navigation_id)),
        Some(child_key)
    );

    let mut back_frame = context.run_ui(viewport(Vec::new()), |context| {
        egui::CentralPanel::default().show(
            context,
            crate::ui::layout::task_details::planning::paint_relationship_back,
        );
    });
    let back_button = button_center(&back_frame, "← Back to TaskIntent");
    back_frame.textures_delta.clear();
    let mut output = context.run_ui(
        viewport(vec![
            egui::Event::PointerMoved(back_button),
            egui::Event::PointerButton {
                pos: back_button,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: Default::default(),
            },
            egui::Event::PointerButton {
                pos: back_button,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: Default::default(),
            },
        ]),
        |context| {
            egui::CentralPanel::default().show(
                context,
                crate::ui::layout::task_details::planning::paint_relationship_back,
            );
        },
    );
    output.textures_delta.clear();
    assert_eq!(
        context.data_mut(|data| data.get_temp::<String>(return_key_id)),
        None
    );
    assert_eq!(
        context.data_mut(|data| {
            data.get_temp::<String>(egui::Id::new("koolade_task_relationship_work_navigation"))
        }),
        Some(root_key)
    );
}
