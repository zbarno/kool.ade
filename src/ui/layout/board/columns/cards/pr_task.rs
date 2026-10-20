use super::super::super::super::task_cards;
use super::super::super::super::*;

pub(in crate::ui::layout::board::columns) fn pr_task_column(
    status: crate::core::lifecycle::PullRequestPreparationStatus,
) -> usize {
    use crate::core::lifecycle::PullRequestPreparationStatus as Status;
    match status {
        Status::AwaitingAuthorization => 3,
        Status::Publishing => 1,
        Status::NeedsAttention | Status::ClosedUnmerged => 3,
        Status::Opened | Status::Merged => 4,
    }
}

pub(in crate::ui::layout::board::columns) fn pr_task(
    ui: &mut egui::Ui,
    task: &crate::ui::planning_board::LifecyclePrBoardTask,
    column: usize,
    selected_path: &mut Option<String>,
) {
    use crate::core::lifecycle::PullRequestPreparationStatus as Status;
    let work = &task.preparation;
    let status = match work.status {
        Status::AwaitingAuthorization => "Awaiting your publication approval",
        Status::Publishing => "Opening the pull request",
        Status::NeedsAttention => "Publication needs your attention",
        Status::Opened => "Pull request is open",
        Status::ClosedUnmerged => "Pull request closed without merging",
        Status::Merged => "Pull request merged",
    };
    let key = format!("pr-preparation:{}", work.work_item_uid);
    let complete = matches!(work.status, Status::Opened | Status::Merged);
    let open_details = task_cards::board_card(
        ui,
        &key,
        None,
        work.status == Status::Publishing,
        column == 3,
        complete,
        |ui| {
            ui.horizontal_wrapped(|ui| {
                theme::badge(ui, "PR preparation", theme::PANEL, theme::TEXT_DIM);
                theme::badge(ui, status, theme::PANEL, theme::TEXT_DIM);
            });
            if ui
                .add(
                    egui::Button::new(
                        RichText::new(task_cards::card_summary(&task.parent_title))
                            .size(15.0)
                            .strong(),
                    )
                    .frame(false)
                    .wrap(),
                )
                .clicked()
            {
                *selected_path = Some(task.parent_path.clone());
            }
            ui.label(theme::helper_text(format!(
                "Separate publication work for {} · revision {}",
                task.parent_title, work.revision
            )));
            let action = if work.status == Status::AwaitingAuthorization {
                "Review PR authorization"
            } else if work.status == Status::NeedsAttention {
                "Review publication issue"
            } else {
                "Open PR details"
            };
            if ui
                .add_sized(
                    [ui.available_width(), 28.0],
                    egui::Button::new(action).fill(theme::ACCENT_SOFT),
                )
                .clicked()
            {
                *selected_path = Some(task.parent_path.clone());
            }
        },
    );
    if open_details {
        *selected_path = Some(task.parent_path.clone());
    }
}

#[cfg(test)]
mod tests {
    use super::pr_task_column;
    use crate::core::lifecycle::PullRequestPreparationStatus as Status;

    #[test]
    fn publication_work_has_a_separate_column_for_each_lifecycle_state() {
        assert_eq!(pr_task_column(Status::AwaitingAuthorization), 3);
        assert_eq!(pr_task_column(Status::Publishing), 1);
        assert_eq!(pr_task_column(Status::NeedsAttention), 3);
        assert_eq!(pr_task_column(Status::ClosedUnmerged), 3);
        assert_eq!(pr_task_column(Status::Opened), 4);
        assert_eq!(pr_task_column(Status::Merged), 4);
    }
}
