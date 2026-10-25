use super::*;

#[test]
// Production boundary: ensures Task Details distinguishes undiscovered
// applicability from a discovered empty target set. Model tests cannot prove
// users see the distinction. In-process egui only; no Docker, SSH, network,
// or credentials; two rendered frames.
fn empty_target_state_renders_truthful_applicability_and_next_step() {
    let undiscovered = render_frame(empty_state::paint_not_assessed);
    assert!(contains_text(
        &undiscovered,
        "Runtime QA applicability: not assessed yet"
    ));
    assert!(!contains_text(
        &undiscovered,
        "Runtime QA applicability: static review only"
    ));
    assert!(
        undiscovered
            .iter()
            .any(|text| text.contains("discover targets to determine runtime QA applicability"))
    );

    let mut rendered_empty_state = false;
    let discovered = render_frame(|ui| {
        rendered_empty_state = empty_state::paint_static_review_only_if_empty(ui, &[]);
    });
    assert!(rendered_empty_state);
    assert!(contains_text(
        &discovered,
        "Runtime QA applicability: static review only"
    ));
    assert!(discovered.iter().any(|text| {
        text.contains("Planning, implementation, and static code review can continue")
            && text.contains("add an approved custom target")
    }));
}

fn render_frame(paint: impl FnOnce(&mut egui::Ui)) -> Vec<String> {
    let context = egui::Context::default();
    context.memory_mut(|memory| memory.set_everything_is_visible(true));
    let mut paint = Some(paint);
    let mut output = context.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(900.0, 500.0),
            )),
            ..Default::default()
        },
        |context| {
            egui::CentralPanel::default().show(context, |ui| {
                if let Some(paint) = paint.take() {
                    paint(ui);
                }
            });
        },
    );
    output.textures_delta.clear();
    output
        .shapes
        .iter()
        .filter_map(|shape| match &shape.shape {
            egui::Shape::Text(text) => Some(text.galley.text().to_owned()),
            _ => None,
        })
        .collect::<Vec<_>>()
}

fn contains_text(rendered: &[String], expected: &str) -> bool {
    rendered.iter().any(|text| text.as_str() == expected)
}
