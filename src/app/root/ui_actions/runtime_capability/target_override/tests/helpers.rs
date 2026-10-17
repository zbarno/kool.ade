use super::super::super::KooladeApp;

pub(super) fn frame(
    context: &egui::Context,
    app: &mut KooladeApp,
    document: &crate::artifacts::task_docs::TaskDocument,
    events: Vec<egui::Event>,
) -> egui::FullOutput {
    context.memory_mut(|memory| memory.set_everything_is_visible(true));
    let mut output = context.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(900.0, 6000.0),
            )),
            events,
            ..Default::default()
        },
        |context| {
            egui::CentralPanel::default().show(context, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    crate::ui::layout::paint_custom_runtime_target_action(ui, app, document);
                });
            });
        },
    );
    output.textures_delta.clear();
    output
}

pub(super) fn text_position(output: &egui::FullOutput, text: &str) -> Option<egui::Pos2> {
    output.shapes.iter().find_map(|shape| {
        let egui::Shape::Text(text_shape) = &shape.shape else {
            return None;
        };
        (text_shape.galley.text() == text)
            .then(|| text_shape.pos + text_shape.galley.mesh_bounds.center().to_vec2())
    })
}

pub(super) fn pointer_button(position: egui::Pos2, pressed: bool) -> egui::Event {
    egui::Event::PointerButton {
        pos: position,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: Default::default(),
    }
}
