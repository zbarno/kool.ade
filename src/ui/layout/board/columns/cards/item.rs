use super::super::super::super::task_cards;
use super::super::super::super::*;

pub(in crate::ui::layout::board::columns) fn item(
    ui: &mut egui::Ui,
    s: &mut dyn Surface,
    board: &crate::ui::planning_board::ViewModel,
    item: &crate::domain::item::OpenItem,
    column: usize,
    planning_selection: &mut Option<String>,
) {
    let key = item.conversation_key();
    let active = s.activity_active(key);
    let open_details = task_cards::board_card(
        ui,
        &item.id,
        Some(item.kind),
        active,
        column == 3,
        column == 4,
        |ui| {
            ui.horizontal(|ui| {
                theme::badge(
                    ui,
                    item.kind.to_string().as_str(),
                    item.kind.badge_colors().0,
                    item.kind.badge_colors().1,
                );
                if column == 3 {
                    super::super::attention::badge(
                        ui,
                        super::super::attention::Kind::WaitingOnUser,
                    );
                } else if column == 4 {
                    theme::badge(ui, "Resolved", theme::PANEL, theme::TEXT_DIM);
                }
            });
            if ui
                .add(
                    egui::Button::new(
                        RichText::new(task_cards::card_summary(&item.question)).strong(),
                    )
                    .frame(false)
                    .wrap(),
                )
                .on_hover_text(&item.question)
                .clicked()
            {
                *planning_selection = Some(item.id.clone());
            }
            let summary = if item.status == crate::domain::ItemStatus::Resolved {
                "Decision recorded"
            } else if crate::core::routing::has_open_prerequisite(&board.planning_items, item) {
                "Waiting for a prerequisite"
            } else if item.authority == crate::domain::Authority::Review {
                "Ready for review"
            } else if item.authority == crate::domain::Authority::Human {
                "Waiting for your response"
            } else {
                "Ready for the next step"
            };
            ui.label(theme::helper_text(summary));
            if column == 3 {
                let action = if item.authority == crate::domain::Authority::Review {
                    "Review"
                } else {
                    "Respond"
                };
                if ui
                    .add_sized(
                        [ui.available_width(), 28.0],
                        egui::Button::new(action).fill(theme::WARNING.gamma_multiply(0.18)),
                    )
                    .clicked()
                {
                    *planning_selection = Some(item.id.clone());
                }
            }
        },
    );
    if open_details {
        *planning_selection = Some(item.id.clone());
    }
}
