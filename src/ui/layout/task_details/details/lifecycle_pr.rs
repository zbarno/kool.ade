use super::*;

pub(super) fn paint(
    ui: &mut egui::Ui,
    surface: &mut dyn Surface,
    ticket: &str,
    view: &crate::ui::task_detail::ViewModel,
    record: Option<&crate::core::implementation::Implementation>,
    status: crate::core::lifecycle::PullRequestPreparationStatus,
) {
    use crate::core::lifecycle::PullRequestPreparationStatus as Status;
    match status {
        Status::AwaitingAuthorization => {
            ui.label("Implementation, independent review, runtime QA, and specification sync are complete. Authorize the single parent pull request when ready.");
            publication_error(ui, view);
            if ui
                .add_enabled(
                    view.can_start
                        && record.is_some_and(|record| {
                            matches!(
                                record.status,
                                crate::core::implementation::ImplementationStatus::ReadyToPublish
                                    | crate::core::implementation::ImplementationStatus::AwaitingApproval
                            )
                        }),
                    egui::Button::new(
                        RichText::new("Authorize and create pull request")
                            .strong()
                            .color(theme::TEXT),
                    )
                    .fill(theme::ACTION)
                    .min_size(egui::vec2(0.0, 36.0)),
                )
                .clicked()
            {
                dispatch(
                    surface,
                    crate::ui::task_detail::Command::PublishLifecyclePr {
                        ticket: ticket.to_owned(),
                    },
                );
            }
        }
        Status::Publishing => {
            ui.label("The authorized parent pull request is being prepared and published.");
            publication_error(ui, view);
        }
        Status::NeedsAttention => {
            ui.label("PR preparation stopped. Review the saved task report below to identify the provider or repository failure. Implementation, review, QA, and specification receipts are preserved; publication will not retry automatically.");
            if let Some(record) = record {
                ui.colored_label(theme::WARNING, &record.detail);
            }
            publication_error(ui, view);
        }
        Status::Opened => {
            ui.label("The parent pull request is open.");
            publication_error(ui, view);
        }
        Status::ClosedUnmerged => {
            ui.label("The parent pull request closed without merging. Review its provider status before deciding whether to prepare a new pull request.");
            publication_error(ui, view);
        }
        Status::Merged => {
            ui.label("The parent pull request is merged.");
            publication_error(ui, view);
        }
    }
}

fn publication_error(ui: &mut egui::Ui, view: &crate::ui::task_detail::ViewModel) {
    if let Some(error) = view.lifecycle_pr_error.as_deref() {
        ui.colored_label(
            theme::DANGER,
            format!("PR preparation status unavailable: {error}"),
        );
    }
}
