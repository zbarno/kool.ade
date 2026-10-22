use super::*;

pub(super) fn paint(
    ui: &mut egui::Ui,
    s: &mut dyn Surface,
    ticket: &str,
    view: &crate::ui::task_detail::ViewModel,
    record: Option<&crate::core::implementation::Implementation>,
) {
    let retry = view.lifecycle_pr_status
        == Some(crate::core::lifecycle::PullRequestPreparationStatus::NeedsAttention);
    ui.label(if retry {
        "Implementation remains Done. Address the PR blocker, then retry publication; the existing PR will be checked before creating one."
    } else {
        "Implementation, independent review, runtime QA, and specification sync are complete. Authorize the single parent pull request when ready."
    });
    if let Some(error) = view.lifecycle_pr_error.as_deref() {
        ui.colored_label(theme::DANGER, format!("PR preparation: {error}"));
    }
    if ui
        .add_enabled(
            view.can_start
                && record.is_some_and(|record| {
                    crate::ui::task_detail::lifecycle_pr_publication_enabled(
                        view.lifecycle_pr_status,
                        record.status,
                        record.pr_url.is_some(),
                    )
                }),
            egui::Button::new(
                RichText::new(if retry {
                    "Retry PR publication"
                } else {
                    "Authorize and create pull request"
                })
                .strong()
                .color(theme::TEXT),
            )
            .fill(theme::ACTION)
            .min_size(egui::vec2(0.0, 36.0)),
        )
        .clicked()
    {
        dispatch(
            s,
            crate::ui::task_detail::Command::PublishLifecyclePr {
                ticket: ticket.to_owned(),
            },
        );
    }
}
