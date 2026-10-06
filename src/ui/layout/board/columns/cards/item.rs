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
    let active = s.activity_active(item.conversation_key());
    task_cards::board_card(
        ui,
        &item.id,
        Some(item.kind),
        active,
        column == 3,
        column == 4,
        |ui| {
            if column == 3 {
                if board.eligible_item_ids.contains(&item.id) {
                    super::super::attention::user_action(ui, &format!("Answer {}", item.id));
                } else {
                    super::super::attention::badge(ui, super::super::attention::Kind::Blocked);
                }
            }
            super::super::super::presentation::metadata(ui, Some(item.kind), &item.id);
            if ui
                .add(
                    egui::Button::new(
                        RichText::new(task_cards::card_summary(&item.question))
                            .size(15.5)
                            .strong()
                            .color(theme::TEXT),
                    )
                    .frame(false)
                    .wrap(),
                )
                .on_hover_text(&item.question)
                .clicked()
            {
                *planning_selection = Some(item.id.clone());
            }
            if task_cards::task_conversation(ui, s, board, item.conversation_key(), false) {
                *planning_selection = Some(item.id.clone());
            }
            ui.add_space(4.0);
            ui.horizontal_wrapped(|ui| {
                ui.label(
                    RichText::new(item.priority.to_string())
                        .size(12.5)
                        .color(theme::WARNING),
                );
                let color = match item.authority {
                    crate::domain::Authority::Agent => theme::TEXT_DIM,
                    crate::domain::Authority::Review => theme::PURPLE,
                    crate::domain::Authority::Human => theme::WARNING,
                };
                ui.label(
                    RichText::new(item.authority.to_string())
                        .size(12.5)
                        .color(color),
                );
                ui.label(
                    RichText::new(item.assigned_to.as_deref().unwrap_or("Unassigned"))
                        .size(12.5)
                        .weak(),
                );
            });
            ui.label(RichText::new(&item.category).size(12.5).weak());
            if active {
                ui.label(RichText::new("● Active").color(theme::BLUE));
            }
            if column == 4 && ui.small_button("Archive").clicked() {
                s.dispatch(ApplicationCommand::ArchiveTask {
                    ticket: item.conversation_key().to_owned(),
                });
            }
            if active {
                crate::ui::task_activity::graph(
                    ui,
                    &s.activity_samples(Some(item.conversation_key())),
                    true,
                    34.0,
                );
            }
        },
    );
}
