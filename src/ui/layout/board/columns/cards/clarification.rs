use super::super::super::super::task_cards;
use super::super::super::super::*;

pub(in crate::ui::layout::board::columns) fn clarification(
    ui: &mut egui::Ui,
    question: &crate::core::planning::QuestionV1,
    selected: &mut Option<String>,
) {
    let key = format!("clarification:{}", question.uid);
    let opened = task_cards::board_card(
        ui,
        &key,
        Some(crate::domain::ItemKind::Question),
        false,
        true,
        false,
        |ui| {
            ui.horizontal(|ui| {
                theme::badge(ui, "Question", theme::PANEL, theme::TEXT_DIM);
                let (label, color) = match question.priority {
                    crate::core::planning::QuestionPriority::Blocking => {
                        ("Blocking", theme::WARNING)
                    }
                    crate::core::planning::QuestionPriority::Important => {
                        ("Important", theme::ACTION)
                    }
                    crate::core::planning::QuestionPriority::Optional => {
                        ("Optional", theme::TEXT_DIM)
                    }
                };
                theme::badge(ui, label, color.gamma_multiply(0.14), color);
            });
            if ui
                .button(egui::RichText::new(&question.question).strong())
                .clicked()
            {
                *selected = Some(key.clone());
            }
            ui.label(theme::helper_text(&question.reason));
            ui.label(theme::helper_text(format!(
                "{} · plan revision {}",
                question.category.label(),
                question.plan_revision
            )));
            if ui
                .add_sized(
                    [ui.available_width(), 28.0],
                    egui::Button::new("Answer").fill(theme::WARNING.gamma_multiply(0.18)),
                )
                .clicked()
            {
                *selected = Some(key.clone());
            }
        },
    );
    if opened {
        *selected = Some(key);
    }
}
