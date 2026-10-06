use super::super::*;
fn fixture() -> DlgSettings {
    DlgSettings {
        user_name: "Zach".into(),
        user_groups: "Platform, QA".into(),
        identity_note: "Current project identity".into(),
        rows: vec![
            Row {
                category: "Product".into(),
                members: String::new(),
            },
            Row {
                category: "Engineering".into(),
                members: "Morgan, platform, (owner TBD)".into(),
            },
        ],
        repositories: Vec::new(),
        feedback: None,
        open_harness_setup: false,
        probe_rx: None,
        probe_view: ProbeView::Pending,
    }
}
#[test]
fn suggestions_deduplicate_and_selection_preserves_custom_owners() {
    let dlg = fixture();
    assert_eq!(
        owner_choices(&dlg),
        vec!["Morgan", "Platform", "QA", "Zach"]
    );
    let mut members = "Custom team, Morgan".to_string();
    set_owner_selected(&mut members, "morgan", true);
    assert_eq!(members, "Custom team, Morgan");
    set_owner_selected(&mut members, "QA", true);
    set_owner_selected(&mut members, "MORGAN", false);
    assert_eq!(members, "Custom team, QA");
}
#[test]
fn existing_owner_can_be_selected_in_the_modal() {
    let mut dlg = fixture();
    let ctx = egui::Context::default();
    fn frame(
        ctx: &egui::Context,
        dlg: &mut DlgSettings,
        events: Vec<egui::Event>,
    ) -> egui::FullOutput {
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1280.0, 900.0),
                )),
                events,
                ..Default::default()
            },
            |ui| {
                crate::ui::overlays::show_modal(
                    ui,
                    true,
                    "Stakeholders & ownership",
                    660.0,
                    |ui| {
                        paint_settings_card(ui, dlg);
                    },
                );
            },
        );
        // Egui paints duplicate-ID diagnostics into the frame when IDs collide.
        assert!(!output.shapes.iter().any(|s| matches!(&s.shape, egui::Shape::Text(t) if t.galley.text().contains("use of ScrollArea ID") || t.galley.text().contains("use of widget ID"))));
        output.textures_delta.clear();
        output
    }
    fn position(output: &egui::FullOutput, text: &str) -> egui::Pos2 {
        output
            .shapes
            .iter()
            .find_map(|s| match &s.shape {
                egui::Shape::Text(t) if t.galley.text() == text => {
                    Some(t.pos + t.galley.mesh_bounds.center().to_vec2())
                }
                _ => None,
            })
            .expect(text)
    }
    fn click(ctx: &egui::Context, dlg: &mut DlgSettings, pos: egui::Pos2) {
        for pressed in [true, false] {
            frame(
                ctx,
                dlg,
                vec![
                    egui::Event::PointerMoved(pos),
                    egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: Default::default(),
                    },
                ],
            );
        }
    }
    frame(&ctx, &mut dlg, vec![]);
    let output = frame(&ctx, &mut dlg, vec![]);
    click(&ctx, &mut dlg, position(&output, "Select existing owners…"));
    frame(&ctx, &mut dlg, vec![]);
    let output = frame(&ctx, &mut dlg, vec![]);
    click(&ctx, &mut dlg, position(&output, "Morgan"));
    assert_eq!(dlg.rows[0].members, "Morgan");
    assert_eq!(dlg.rows[1].members, "Morgan, platform, (owner TBD)");
}
