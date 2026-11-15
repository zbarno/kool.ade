use super::{Action, ComparisonIntent, paint_comparison};

fn action() -> Action {
    let alternative = |id: &str| crate::domain::PlanAlternative {
        id: id.into(),
        objective: format!("Plan {id} keeps the approved feature behavior."),
        phases: vec![
            crate::domain::PlanPhase {
                name: "Persist".into(),
                subtasks: vec!["Write validated state".into()],
            },
            crate::domain::PlanPhase {
                name: "Adopt".into(),
                subtasks: vec!["Freeze the selected plan".into()],
            },
            crate::domain::PlanPhase {
                name: "Verify".into(),
                subtasks: vec!["Reload and check the record".into()],
            },
        ],
        files_touched: vec![format!("src/plan_{id}.rs")],
        state_changes: vec![format!("Record {id}")],
        failure_modes: vec![format!("Plan {id} write fails")],
        effort_band: "Medium — multiple modules".into(),
        known_risks: vec![format!("Plan {id} migration risk")],
        reversibility: format!("Remove Plan {id} record"),
    };
    Action {
        id: "F7".into(),
        specification: String::new(),
        approved: false,
        user_confirmed: false,
        ready: true,
        plan_snapshot: None,
        review_verdict: None,
        prepare_tasks: false,
        retry_review: false,
        compare_plans: false,
        plan_comparison: Some(crate::domain::PlanComparison {
            alternatives: vec![alternative("A"), alternative("B")],
            recommendation: crate::domain::PlanRecommendation {
                plan_id: "A".into(),
                rationale: "Plan A has fewer migration steps.".into(),
                evidence: vec!["src/core/workflow.rs".into()],
            },
            selected_plan: None,
        }),
    }
}

/// Production boundary: approval ownership is resolved by immutable feature
/// identity, not a shared display ID. No external dependencies or setup cost.
#[test]
fn feature_approval_is_owned_only_by_the_matching_task_intent_root() {
    use crate::core::planning_work::{Work, WorkKind};
    let mut root = Work::new(
        "intent:one".into(),
        "Feature root".into(),
        "Request".into(),
        String::new(),
    );
    root.feature_id = Some("CHG-004".into());
    root.feature_uid = Some("stable-feature-uid".into());
    assert!(super::belongs_to_root(&root, "stable-feature-uid"));
    assert!(!super::belongs_to_root(&root, "different-feature-uid"));

    let mut child = Work::new(
        "intent:child".into(),
        "Planning child".into(),
        "Request".into(),
        String::new(),
    );
    child.feature_id = root.feature_id.clone();
    child.feature_uid = root.feature_uid.clone();
    child.parent_uid = Some(root.uid.clone());
    child.task_intent_uid = Some(root.uid.clone());
    assert!(!super::belongs_to_root(&child, "stable-feature-uid"));

    let mut generation = root.clone();
    generation.kind = WorkKind::TaskGeneration;
    assert!(!super::belongs_to_root(&generation, "stable-feature-uid"));
}

#[test]
fn unavailable_review_retry_action_has_a_plain_language_label() {
    let mut action = action();
    action.retry_review = true;
    action.review_verdict = Some(review_unable_verdict());
    assert_eq!(action.label(), "Retry independent review");
}

fn review_unable_verdict() -> crate::harness::prompt_catalog::ReviewVerdictV1 {
    crate::harness::prompt_catalog::ReviewVerdictV1 {
        subject_hash: "a".repeat(64),
        examined_files: vec![],
        tested_evidence_ids: vec![],
        unexamined_areas: vec!["One review area".into()],
        pass: false,
        outcome: Some(crate::harness::prompt_catalog::ReviewOutcomeV1::ReviewUnable),
        coverage: vec![],
        findings: vec![],
        untested: vec!["REQ-001".into()],
    }
}

/// Production UI boundary: paint the persisted unable-to-review verdict and
/// verify that the person sees an incomplete review rather than findings.
/// Model tests cannot catch confusing rendered copy. In-memory egui only.
#[test]
fn unable_review_copy_says_the_plan_is_unchanged() {
    let context = egui::Context::default();
    let mut output = context.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(400.0, 500.0),
            )),
            ..Default::default()
        },
        |context| {
            egui::CentralPanel::default().show(context, |ui| {
                super::confirmation::paint_review(ui, &review_unable_verdict());
            });
        },
    );
    let labels: Vec<_> = output
        .shapes
        .iter()
        .filter_map(|shape| match &shape.shape {
            egui::Shape::Text(shape) => Some(shape.galley.text().to_owned()),
            _ => None,
        })
        .collect();
    output.textures_delta.clear();
    assert!(
        labels
            .iter()
            .any(|text| text == "Independent review could not finish")
    );
    assert!(labels.iter().any(|text| text.contains("left unchecked")));
    assert!(
        !labels
            .iter()
            .any(|text| text == "Independent review findings")
    );
}

fn frame(
    context: &egui::Context,
    action: &Action,
    events: Vec<egui::Event>,
) -> (egui::FullOutput, Option<ComparisonIntent>) {
    let mut intent = None;
    let mut output = context.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(360.0, 480.0),
            )),
            events,
            ..Default::default()
        },
        |context| {
            egui::CentralPanel::default().show(context, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    intent = paint_comparison(ui, action);
                });
            });
        },
    );
    output.textures_delta.clear();
    (output, intent)
}

fn text_center(output: &egui::FullOutput, text: &str) -> Option<egui::Pos2> {
    output.shapes.iter().find_map(|shape| {
        let egui::Shape::Text(shape) = &shape.shape else {
            return None;
        };
        (shape.galley.text() == text)
            .then(|| shape.pos + shape.galley.mesh_bounds.center().to_vec2())
    })
}

#[test]
fn plan_adoption_remains_clickable_at_narrow_viewport_width() {
    let context = egui::Context::default();
    let action = action();
    let (output, intent) = frame(&context, &action, vec![]);
    assert!(intent.is_none());
    assert!(text_center(&output, "Adopt Plan A").is_some());
    let mut latest = output;
    let mut position = None;
    for _ in 0..15 {
        position = text_center(&latest, "Adopt Plan B")
            .filter(|point| (0.0..360.0).contains(&point.x) && (30.0..450.0).contains(&point.y));
        if position.is_some() {
            break;
        }
        latest = frame(
            &context,
            &action,
            vec![
                egui::Event::PointerMoved(egui::pos2(180.0, 350.0)),
                egui::Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    delta: egui::vec2(0.0, -90.0),
                    phase: egui::TouchPhase::Move,
                    modifiers: Default::default(),
                },
            ],
        )
        .0;
    }
    let position = position.expect("Plan B remains reachable by scrolling at 360x480");

    let (_, intent) = frame(
        &context,
        &action,
        vec![
            egui::Event::PointerMoved(position),
            egui::Event::PointerButton {
                pos: position,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: Default::default(),
            },
        ],
    );
    assert!(intent.is_none());
    let (_, intent) = frame(
        &context,
        &action,
        vec![egui::Event::PointerButton {
            pos: position,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: Default::default(),
        }],
    );
    assert!(matches!(intent, Some(ComparisonIntent::Adopt(id)) if id == "B"));
}
