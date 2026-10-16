use super::*;

#[test]
fn clicking_operator_receipt_from_trace_view_selects_operator_decision() {
    let mut app = crate::app::root::board_tests::trace_navigation_test_app();
    let (feature_uid, _, task_uid, _, _, _, _, _, _, _, _, operator_uid) =
        app.seed_trace_navigation_fixture();
    crate::ui::Surface::dispatch(
        &mut app,
        crate::ui::ApplicationCommand::OpenSpecificationTrace {
            feature_uid: feature_uid.clone(),
            kind: crate::core::lifecycle::SpecTraceReferenceKindV1::Task,
            reference: task_uid.clone(),
        },
    );
    let navigation = crate::ui::Surface::specification_trace_navigation(&app).unwrap();
    assert_eq!(navigation.target_uid, task_uid);
    assert_eq!(
        navigation.target_kind,
        crate::core::lifecycle::SpecTraceTargetKindV1::Task
    );
    let context = egui::Context::default();
    let screen = Some(egui::Rect::from_min_size(
        egui::Pos2::ZERO,
        egui::vec2(900.0, 600.0),
    ));
    let mut output = context.run_ui(
        egui::RawInput {
            screen_rect: screen,
            ..Default::default()
        },
        |ui| paint(ui, &mut app, navigation.clone()),
    );
    let label = format!("Decision or receipt: {operator_uid}");
    let center = output
        .shapes
        .iter()
        .find_map(|shape| {
            let egui::Shape::Text(shape) = &shape.shape else {
                return None;
            };
            shape
                .galley
                .text()
                .contains(&label)
                .then(|| shape.pos + shape.galley.mesh_bounds.center().to_vec2())
        })
        .expect("operator receipt link appears in selected trace view");
    output.textures_delta.clear();
    let frame = |pressed| egui::RawInput {
        screen_rect: screen,
        events: vec![
            egui::Event::PointerMoved(center),
            egui::Event::PointerButton {
                pos: center,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: Default::default(),
            },
        ],
        ..Default::default()
    };
    let mut pressed = context.run_ui(frame(true), |ui| {
        paint(ui, &mut app, navigation.clone());
    });
    pressed.textures_delta.clear();
    let mut released = context.run_ui(frame(false), |ui| {
        paint(ui, &mut app, navigation.clone());
    });
    released.textures_delta.clear();

    let selected = crate::ui::Surface::specification_trace_navigation(&app).unwrap();
    assert_eq!(selected.target_uid, operator_uid);
    assert_eq!(
        selected.target_kind,
        crate::core::lifecycle::SpecTraceTargetKindV1::OperatorDecision
    );
}
