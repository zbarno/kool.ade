use super::*;
use crate::core::lifecycle::{SpecDeltaClassificationV1, SpecDeltaRecordV1};

fn delta() -> SpecDeltaRecordV1 {
    SpecDeltaRecordV1 {
        requirement_id: "REQ-901".into(),
        event_id: "00000000-0000-4000-8000-000000000901".into(),
        old_target: Some("Old target".into()),
        proposed_target: Some("New target".into()),
        observed_current: None,
        classification: SpecDeltaClassificationV1::UserDecision,
        evidence_refs: vec!["doc:feature-901".into()],
        decision_refs: vec!["00000000-0000-4000-8000-000000000902".into()],
        test_refs: vec!["tests/feature_901.rs".into()],
        affected_task_ids: Vec::new(),
        needs_user_reapproval: true,
        followup_ids: Vec::new(),
        operator_decision: None,
    }
}

fn click(label: &str) -> (Option<ApplicationCommand>, Vec<String>) {
    let context = egui::Context::default();
    let app = crate::app::root::board_tests::trace_navigation_test_app();
    let screen = Some(egui::Rect::from_min_size(
        egui::Pos2::ZERO,
        egui::vec2(500.0, 300.0),
    ));
    let mut command = None;
    let mut initial = context.run_ui(
        egui::RawInput {
            screen_rect: screen,
            ..Default::default()
        },
        |ui| command = paint(ui, &app, Some("feature-901"), Some(4), delta()),
    );
    let center = initial
        .shapes
        .iter()
        .find_map(|shape| {
            let egui::Shape::Text(shape) = &shape.shape else {
                return None;
            };
            (shape.galley.text() == label)
                .then(|| shape.pos + shape.galley.mesh_bounds.center().to_vec2())
        })
        .expect("control is rendered");
    initial.textures_delta.clear();
    let pointer_input = |pressed| egui::RawInput {
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
    let mut pressed = context.run_ui(pointer_input(true), |ui| {
        let _ = paint(ui, &app, Some("feature-901"), Some(4), delta());
    });
    pressed.textures_delta.clear();
    let mut released = context.run_ui(pointer_input(false), |ui| {
        command = paint(ui, &app, Some("feature-901"), Some(4), delta());
    });
    released.textures_delta.clear();
    let text = released
        .shapes
        .iter()
        .filter_map(|shape| {
            let egui::Shape::Text(shape) = &shape.shape else {
                return None;
            };
            Some(shape.galley.text().to_string())
        })
        .collect();
    (command, text)
}

#[test]
// Production UI boundary: egui pointer events exercise the visible proposal
// buttons and typed command payload; no external service is required.
fn rendered_adopt_and_decline_buttons_emit_expected_commands() {
    let matches = |command, adopt| {
        matches!(
            command,
            Some(ApplicationCommand::DecideFeatureSpecDelta {
                feature_uid,
                requirement_id,
                expected_sequence,
                adopt: actual,
            }) if feature_uid == "feature-901"
                && requirement_id == "REQ-901"
                && expected_sequence == 4
                && actual == adopt
        )
    };
    assert!(matches(click("Adopt target").0, true));
    assert!(matches(click("Decline").0, false));
}

#[test]
// The visible reference emits the typed navigation command consumed by the
// production application dispatcher.
fn clicking_evidence_reference_emits_typed_navigation() {
    let (command, _) = click("Evidence or PR state trace: doc:feature-901");
    assert!(matches!(
        command,
        Some(ApplicationCommand::OpenSpecificationTrace {
            feature_uid,
            kind: crate::core::lifecycle::SpecTraceReferenceKindV1::Evidence,
            reference,
        }) if feature_uid == "feature-901" && reference == "doc:feature-901"
    ));
}

#[test]
fn clicking_persisted_task_reference_opens_its_record_view() {
    let mut app = crate::app::root::board_tests::trace_navigation_test_app();
    let (feature_uid, delta, task_uid, _, _, _, _, _, _, _, _, _) =
        app.seed_trace_navigation_fixture();
    let context = egui::Context::default();
    let screen = Some(egui::Rect::from_min_size(
        egui::Pos2::ZERO,
        egui::vec2(800.0, 500.0),
    ));
    let mut output = context.run_ui(
        egui::RawInput {
            screen_rect: screen,
            ..Default::default()
        },
        |ui| {
            let _ = paint(ui, &app, Some(&feature_uid), Some(2), delta.clone());
        },
    );
    output.textures_delta.clear();
    let label = format!("Task trace: {task_uid}");
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
        .expect("persisted task link is rendered");
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
        let _ = paint(ui, &app, Some(&feature_uid), Some(2), delta.clone());
    });
    pressed.textures_delta.clear();
    let mut command = None;
    let mut released = context.run_ui(frame(false), |ui| {
        command = paint(ui, &app, Some(&feature_uid), Some(2), delta.clone());
    });
    released.textures_delta.clear();
    crate::ui::Surface::dispatch(
        &mut app,
        command.expect("task click dispatches typed navigation"),
    );
    let selected = crate::ui::Surface::specification_trace_navigation(&app)
        .expect("app dispatcher selects persisted task");
    assert_eq!(selected.target_uid, task_uid);
    assert_eq!(
        selected.target_kind,
        crate::core::lifecycle::SpecTraceTargetKindV1::Task
    );
    assert!(selected.detail.contains("Phase Ready"));
}
