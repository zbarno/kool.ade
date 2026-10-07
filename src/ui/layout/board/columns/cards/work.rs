use super::super::super::super::task_cards;
use super::super::super::super::*;

pub(in crate::ui::layout::board::columns) fn work(
    ui: &mut egui::Ui,
    s: &mut dyn Surface,
    work: &crate::core::planning_work::Work,
    planning: &[&crate::core::planning_work::Work],
    column: usize,
    selected: &mut Option<String>,
) {
    let kind = (work.kind == crate::core::planning_work::WorkKind::Question)
        .then_some(crate::domain::ItemKind::Question);
    let active = s.active_planning_work() == Some(work.key.as_str());
    let open_details = task_cards::board_card(
        ui,
        &work.key,
        kind,
        active || work.status == crate::core::planning_work::WorkStatus::InProgress,
        column == 3,
        column == 4,
        |ui| {
            ui.horizontal(|ui| {
                theme::badge(ui, work.kind.label(), theme::PANEL, theme::TEXT_DIM);
                if column == 3 {
                    super::super::attention::badge(
                        ui,
                        super::super::attention::Kind::WaitingOnUser,
                    );
                }
            });
            if ui
                .add(
                    egui::Button::new(
                        RichText::new(task_cards::card_summary(&work.title))
                            .size(15.0)
                            .strong(),
                    )
                    .frame(false)
                    .wrap(),
                )
                .clicked()
            {
                *selected = Some(work.key.clone());
            }
            let summary = if active {
                "Kool.ad/e is working on this task"
            } else if crate::ui::layout::planning_parent_label(work, planning).is_some() {
                "Waiting for prerequisite"
            } else {
                match work.status {
                    crate::core::planning_work::WorkStatus::Todo => "Ready for the next step",
                    crate::core::planning_work::WorkStatus::InProgress => "In progress",
                    crate::core::planning_work::WorkStatus::InReview => "Ready for review",
                    crate::core::planning_work::WorkStatus::NeedsAttention => {
                        "Waiting for your response"
                    }
                    crate::core::planning_work::WorkStatus::Done => "Completed",
                }
            };
            ui.label(theme::helper_text(summary));
            super::super::super::super::activity::paint_card(ui, s, &work.key, false);
            if column == 3
                && ui
                    .add_sized(
                        [ui.available_width(), 28.0],
                        egui::Button::new("Respond").fill(theme::WARNING.gamma_multiply(0.18)),
                    )
                    .clicked()
            {
                *selected = Some(work.key.clone());
            }
        },
    );
    if open_details {
        *selected = Some(work.key.clone());
    }
}
