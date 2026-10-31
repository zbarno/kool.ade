//! Accessible task answer cards.
use super::*;

pub(crate) fn card(ui: &mut egui::Ui, title: &str, description: &str, enabled: bool) -> bool {
    ui.add_enabled_ui(enabled, |ui| {
        let accessible_label = format!("{title}. {description}");
        let width = ui.available_width();
        let text_width = (width - 100.0).max(40.0);
        let galley = ui.painter().layout(
            title.to_owned(),
            theme::task_heading_font(ui.ctx(), 22.0),
            theme::TEXT,
            text_width,
        );
        let description = ui.painter().layout(
            description.to_owned(),
            egui::FontId::proportional(17.0),
            theme::TEXT_DIM,
            text_width,
        );
        let text_height = galley.size().y + description.size().y + 6.0;
        let height = (text_height + 28.0).max(88.0);
        let (rect, response) =
            ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::click());
        response.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, &accessible_label)
        });
        if ui.is_rect_visible(rect) {
            let emphasized =
                response.hovered() || response.has_focus() || response.is_pointer_button_down_on();
            ui.painter().rect(
                rect,
                12,
                if emphasized {
                    theme::ACCENT_SOFT
                } else {
                    theme::PANEL_ALT
                },
                egui::Stroke::new(
                    1.5,
                    if emphasized {
                        theme::BLUE
                    } else {
                        theme::BORDER
                    },
                ),
                egui::StrokeKind::Inside,
            );
            let center = egui::pos2(rect.left() + 38.0, rect.center().y);
            ui.painter()
                .circle_stroke(center, 17.0, egui::Stroke::new(2.5, theme::TEXT_DIM));
            if title == "I'm not sure" {
                ui.painter().text(
                    center,
                    egui::Align2::CENTER_CENTER,
                    "?",
                    egui::FontId::proportional(22.0),
                    theme::TEXT,
                );
            }
            let text_top = rect.center().y - text_height / 2.0;
            let description_top = text_top + galley.size().y + 6.0;
            ui.painter().galley(
                egui::pos2(rect.left() + 78.0, text_top),
                galley,
                theme::TEXT,
            );
            ui.painter().galley(
                egui::pos2(rect.left() + 78.0, description_top),
                description,
                theme::TEXT_DIM,
            );
        }
        response.clicked()
    })
    .inner
}

pub(super) fn paint_change(
    ui: &mut egui::Ui,
    surface: &mut dyn Surface,
    state: state::Presentation<'_>,
    brief: &crate::core::attention::Brief,
) {
    ui.collapsing("Change decision", |ui| {
        for choice in &brief.options {
            if card(
                ui,
                &choice.label,
                &choice.meaning,
                !state.view.conversation_active,
            ) {
                if choice.clarification {
                    ask(
                        surface,
                        state.ticket,
                        format!("Please explain {} before I decide.", choice.label),
                    );
                } else {
                    submit(
                        surface,
                        state.ticket,
                        format!("I choose option ({}): {}.", choice.id, choice.label),
                        true,
                    );
                }
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn answer_card_supports_keyboard_activation_without_a_default_answer() {
        for enabled in [true, false] {
            let ctx = egui::Context::default();
            let mut clicks = 0;
            let mut frame = |events| {
                let mut output = ctx.run_ui(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(600.0, 400.0),
                        )),
                        events,
                        ..Default::default()
                    },
                    |ui| {
                        if card(ui, "Use the new default", "Update the plan.", enabled) {
                            clicks += 1;
                        }
                    },
                );
                output.textures_delta.clear();
            };
            frame(vec![]);
            frame(vec![key(egui::Key::Tab, true)]);
            frame(vec![key(egui::Key::Tab, false)]);
            frame(vec![key(egui::Key::Enter, true)]);
            frame(vec![key(egui::Key::Enter, false)]);
            assert_eq!(
                clicks,
                usize::from(enabled),
                "disabled cards must never submit"
            );
        }
    }

    fn key(key: egui::Key, pressed: bool) -> egui::Event {
        egui::Event::Key {
            key,
            physical_key: None,
            pressed,
            repeat: false,
            modifiers: Default::default(),
        }
    }
}
