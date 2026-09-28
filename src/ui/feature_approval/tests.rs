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
        prepare_tasks: false,
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
    let position = text_center(&output, "Adopt Plan B")
        .expect("Plan B action should remain visible in a 360x480 comparison view");
    assert!((0.0..360.0).contains(&position.x));
    assert!((0.0..480.0).contains(&position.y));

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
