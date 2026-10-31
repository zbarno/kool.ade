use super::*;

#[path = "support/fixture.rs"]
mod fixture;
pub(in crate::app::root) use fixture::fixture;

#[path = "support/restart.rs"]
mod restart;
pub(crate) use restart::restart_fixture;

pub(in crate::app::root) fn frame(
    app: &mut KooladeApp,
    ctx: &egui::Context,
    events: Vec<egui::Event>,
) -> egui::FullOutput {
    frame_at(app, ctx, events, egui::vec2(1800.0, 900.0))
}

pub(in crate::app::root) fn frame_at(
    app: &mut KooladeApp,
    ctx: &egui::Context,
    events: Vec<egui::Event>,
    size: egui::Vec2,
) -> egui::FullOutput {
    for theme in [egui::Theme::Dark, egui::Theme::Light] {
        ctx.style_mut_of(theme, |style| style.animation_time = 0.0);
    }
    let mut output = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
            events,
            ..Default::default()
        },
        |ui| {
            crate::ui::layout::paint(ui, app);
            app.render_queued_settings_dialog(ui);
        },
    );
    output.textures_delta.clear();
    output
}
pub(in crate::app::root) fn text_position(
    output: &egui::FullOutput,
    needle: &str,
) -> Option<egui::Pos2> {
    output
        .shapes
        .iter()
        .find_map(|shape| {
            if let egui::Shape::Text(text) = &shape.shape
                && text.galley.text() == needle
            {
                return Some(text.pos + text.galley.mesh_bounds.center().to_vec2());
            }
            None
        })
        .or_else(|| super::mockup_layout::lane_position(output, needle))
}
pub(in crate::app::root) fn text_contains(output: &egui::FullOutput, needle: &str) -> bool {
    output.shapes.iter().any(|shape| {
        matches!(
            &shape.shape,
            egui::Shape::Text(text) if text.galley.text().contains(needle)
        )
    })
}

pub(in crate::app::root) fn click_text(
    app: &mut KooladeApp,
    ctx: &egui::Context,
    label: &str,
) -> egui::FullOutput {
    click_text_at(app, ctx, label, egui::vec2(1800.0, 900.0))
}

pub(in crate::app::root) fn click_text_at(
    app: &mut KooladeApp,
    ctx: &egui::Context,
    label: &str,
    size: egui::Vec2,
) -> egui::FullOutput {
    let output = frame_at(app, ctx, vec![], size);
    let click = text_position(&output, label)
        .or_else(|| {
            output.shapes.iter().find_map(|shape| match &shape.shape {
                egui::Shape::Text(text) if text.galley.text().contains(label) => {
                    Some(text.pos + text.galley.mesh_bounds.center().to_vec2())
                }
                _ => None,
            })
        })
        .unwrap_or_else(|| {
            let visible = output
                .shapes
                .iter()
                .filter_map(|shape| match &shape.shape {
                    egui::Shape::Text(text) => Some(text.galley.text().to_owned()),
                    _ => None,
                })
                .collect::<Vec<_>>();
            panic!("missing clickable {label}; visible: {visible:?}")
        });
    frame_at(
        app,
        ctx,
        vec![
            egui::Event::PointerMoved(click),
            egui::Event::PointerButton {
                pos: click,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: Default::default(),
            },
        ],
        size,
    );
    frame_at(
        app,
        ctx,
        vec![egui::Event::PointerButton {
            pos: click,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: Default::default(),
        }],
        size,
    );
    frame_at(app, ctx, vec![], size)
}

pub(in crate::app::root) struct HangingTurnHarness;
impl crate::harness::AiHarness for HangingTurnHarness {
    fn label(&self) -> String {
        "hanging-fixture 0".into()
    }
    fn check_available(&self) -> Result<String, crate::AppError> {
        Ok("present".into())
    }
    fn execute(
        &self,
        request: &crate::harness::PlanningRequest,
    ) -> Result<crate::harness::HarnessOutcome, crate::AppError> {
        while !request.cancel.load(std::sync::atomic::Ordering::SeqCst) {
            std::thread::sleep(Duration::from_millis(10));
        }
        Err(crate::AppError::Other(String::from(
            "fixture turn cancelled",
        )))
    }
}

/// All on-screen text, galley-concatenated without separators so a
/// phrase spanning a toast's soft line wrap still matches as a whole.
pub(in crate::app::root) fn canvas_text(output: &egui::FullOutput) -> String {
    output
        .shapes
        .iter()
        .filter_map(|shape| match &shape.shape {
            egui::Shape::Text(text) => Some(text.galley.text().to_string()),
            _ => None,
        })
        .collect()
}

/// Paint ONE SETTLED frame with the app's toast layer included — the
/// test helper paints `layout::paint` directly, bypassing
/// `KooladeApp::ui`, so the same `toasts.show(..)` call is replayed
/// INSIDE the layout pass, in the same order the native app uses it.
/// A freshly introduced toast area needs one settle pass before its
/// content reaches the paint output, so one pass is discarded and the
/// settled second pass is returned.
pub(in crate::app::root) fn frame_toasting(
    app: &mut KooladeApp,
    ctx: &egui::Context,
) -> egui::FullOutput {
    let paint_one_pass = |app: &mut KooladeApp, ctx: &egui::Context| -> egui::FullOutput {
        for theme in [egui::Theme::Dark, egui::Theme::Light] {
            ctx.style_mut_of(theme, |style| style.animation_time = 0.0);
        }
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1800.0, 900.0),
                )),
                events: vec![],
                ..Default::default()
            },
            |ui| {
                crate::ui::layout::paint(ui, app);
                app.toasts().show(ui.ctx());
            },
        );
        output.textures_delta.clear();
        output
    };
    let _ = paint_one_pass(app, ctx);
    paint_one_pass(app, ctx)
}

// Mirrors the production pattern deliberately: the UI owns the live turn
// handle locally, and the assertion needs exactly that Rc's identity.
pub(in crate::app::root) fn park_fixture_with_unapproved_feature(lapsed: bool) -> KooladeApp {
    let mut app = fixture();
    if let Screen::Connected(p) = &mut app.screen {
        p.queue.running = true;
        if lapsed {
            let draft = "# CHG-999: Fixture feature\n\n**Status:** Ready\n\n## Intent\n\nKeep the fixture behavior.\n\n## Desired Behavior\n\nKeep the fixture behavior approved.\n";
            let identified = crate::domain::ArtifactIdentity::preserve_markdown(
                draft,
                None,
                "CHG-999",
                "Fixture feature",
            )
            .unwrap();
            let identity = crate::domain::ArtifactIdentity::from_markdown(&identified)
                .unwrap()
                .unwrap();
            let specification = crate::domain::ChangeMetadata::write_markdown(
                &identified,
                &identity,
                crate::domain::ChangeStatus::Ready,
            )
            .unwrap();
            std::fs::create_dir_all(
                p.state
                    .planning_store
                    .root
                    .join("planning/changes/CHG-999-fixture"),
            )
            .unwrap();
            let feature_path = crate::artifacts::product_docs::document_path(
                &p.state.planning_store,
                "feature:CHG-999",
            )
            .unwrap();
            std::fs::create_dir_all(feature_path.parent().unwrap()).unwrap();
            std::fs::write(feature_path, specification).unwrap();
            p.state
                .workflow
                .approved_features
                .insert("CHG-999".into(), "fixture stale contract".into());
            crate::persistence::feature_approvals::record(
                &p.state.planning_store.root,
                "CHG-999",
                "fixture stale contract",
            )
            .unwrap();
        }
        for doc in &mut p.task_documents {
            doc.text.push_str("\nFeature ID: CHG-999\n");
        }
    }
    app
}
