use super::super::super::super::task_cards;
use super::super::super::super::*;

pub(in crate::ui::layout::board::columns) fn work(
    ui: &mut egui::Ui,
    s: &mut dyn Surface,
    work: &crate::core::planning_work::Work,
    board: &crate::ui::planning_board::ViewModel,
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
                if super::ownership::work_is_agent(work)
                    || crate::ui::saved_reply::label(s.task_messages(&work.key)).is_some()
                {
                    super::ownership::agent_pill(ui);
                }
                if column == 3 {
                    super::super::attention::badge(ui, super::super::attention::Kind::Blocked);
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
            let summary =
                if column == 4 && work.status != crate::core::planning_work::WorkStatus::Done {
                    "Cancelled"
                } else if active {
                    "Kool.ad/e is working on this task"
                } else {
                    match work.status {
                        crate::core::planning_work::WorkStatus::Todo => "Ready for the next step",
                        crate::core::planning_work::WorkStatus::InProgress => "In progress",
                        crate::core::planning_work::WorkStatus::InReview => "Ready for review",
                        crate::core::planning_work::WorkStatus::NeedsAttention => {
                            crate::ui::saved_reply::label(s.task_messages(&work.key))
                                .unwrap_or("Planning needs review or internal recovery")
                        }
                        crate::core::planning_work::WorkStatus::Done => "Completed",
                    }
                };
            ui.label(theme::helper_text(summary));
            if work.task_intent_root_uid().is_some_and(|uid| {
                uid != work.uid
                    && board
                        .planning_work
                        .iter()
                        .filter(|root| root.uid == uid && root.task_intent_root_uid() == Some(uid))
                        .count()
                        != 1
            }) {
                ui.label(theme::helper_text("Request link needs agent recovery"));
            }
            if work.operational_recovery.is_some()
                || work.status == crate::core::planning_work::WorkStatus::NeedsAttention
            {
                let blocker = (work.status
                    == crate::core::planning_work::WorkStatus::NeedsAttention)
                    .then(|| {
                        work.operational_recovery
                            .as_ref()
                            .and_then(|recovery| recovery.repair_error.as_deref())
                    })
                    .flatten()
                    .unwrap_or(&work.detail);
                ui.label(theme::helper_text(
                    blocker.chars().take(220).collect::<String>(),
                ));
            }
            for intent in work.git_intents.values() {
                ui.label(theme::helper_text(format!(
                    "{} · {} → {} · {}{}",
                    intent.repository_id,
                    intent.source_ref,
                    intent.destination_ref,
                    match intent.publication_mode {
                        crate::core::git_intent::PublicationModeV1::HoldForReview => "Review",
                        crate::core::git_intent::PublicationModeV1::CreatePullRequest => "PR",
                        crate::core::git_intent::PublicationModeV1::AutoPublish => "Auto publish",
                    },
                    if intent.inherited_from_task_uid.is_some() {
                        " · inherited"
                    } else {
                        ""
                    }
                )));
            }
            super::super::super::super::activity::paint_card(ui, s, &work.key, false);
            if column == 3
                && ui
                    .add_sized(
                        [ui.available_width(), 28.0],
                        egui::Button::new("View details").fill(theme::WARNING.gamma_multiply(0.18)),
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
