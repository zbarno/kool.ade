use super::*;

#[test]
fn task_details_show_full_state_and_inline_reply() {
    let _environment = crate::test_support::lock_process_environment();
    let mut app = fixture();
    let key = ".koolade-packet/planning/tasks/fixture/001-task.md";
    if let Screen::Connected(p) = &mut app.screen {
        p.queue.blocked.insert(p.task_documents[0].path.clone(),
            crate::core::implementation::Failure::new(
                crate::core::implementation::FailureKind::ExternalPrerequisite,
                crate::core::implementation::RecoveryDisposition::UserAction,
                "## Waiting for user action\n\nThe published history conflicts with the gate.\n\n### Next action(s)\n\n- Adjudicator: approve the corrected footprint.\n- Operator: record the display demonstration.\n\nFull report: saved-report.json",
            ));
    }
    app.attention_fixture.insert(
        key.into(),
        crate::core::attention::Brief {
            problem: "The published history conflicts with the required file list.".into(),
            recommendation: None,
            options: Vec::new(),
            steps: vec![crate::core::attention::HumanStep {
                owner: "Operator".into(),
                action: "Record the display demonstration.".into(),
            }],
            after: "Resume once the required review is complete.".into(),
        },
    );
    let ctx = egui::Context::default();
    frame(&mut app, &ctx, vec![]);
    let output = click_text(&mut app, &ctx, "First task");
    for label in [
        "Ask a question",
        "Why am I seeing this?",
        "Resume after action",
        "Send",
    ] {
        assert!(text_position(&output, label).is_some(), "missing {label}");
    }
    let conversation = text_position(&output, "Ask a question").unwrap();
    let context = text_position(&output, "Why am I seeing this?").unwrap();
    assert!(
        conversation.x > context.x,
        "conversation should stay on the right"
    );
    assert!(text_position(&output, "CURRENT STATE").is_none());
    let output = click_text(&mut app, &ctx, "Why am I seeing this?");
    assert!(text_position(&output, "CURRENT STATE").is_some());
    assert!(
        text_position(
            &output,
            "The published history conflicts with the required file list."
        )
        .is_some()
    );
    assert!(text_position(&output, "Operator: Record the display demonstration.").is_some());
    assert!(text_position(&output, "Full blocker report").is_some());
    let output = click_text(&mut app, &ctx, "Full blocker report");
    assert!(text_contains(&output, "Full report: saved-report.json"));
    frame(
        &mut app,
        &ctx,
        vec![
            egui::Event::PointerMoved(egui::pos2(400.0, 700.0)),
            egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: egui::vec2(0.0, -400.0),
                phase: egui::TouchPhase::Move,
                modifiers: Default::default(),
            },
        ],
    );
    for _ in 0..3 {
        frame(&mut app, &ctx, vec![]);
    }
    let output = frame(&mut app, &ctx, vec![]);
    assert!(text_position(&output, "Copy full report").is_some());
    assert!(
        output.shapes.iter().any(|shape| matches!(&shape.shape,
        egui::Shape::Text(text) if text.galley.text().contains("Full report: saved-report.json"))),
        "the complete failure text must be rendered, not shortened to a summary"
    );
}
