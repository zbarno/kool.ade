use super::*;

pub(super) fn paint(
    ui: &mut egui::Ui,
    s: &mut dyn Surface,
    ticket: &str,
    view: &crate::ui::task_detail::ViewModel,
    record: Option<&crate::core::implementation::Implementation>,
) {
    if view.lifecycle.as_ref().is_some_and(|lifecycle| {
        lifecycle.task_phase == crate::core::lifecycle::TaskPhase::ReReview
    }) {
        ui.label("The destination branch changed after review. Previous review, QA, and specification receipts were invalidated; refresh the gates before authorizing PR publication.");
        if ui
            .add_enabled(
                view.can_start
                    && record.is_some_and(|record| {
                        crate::core::lifecycle::pull_request_gate_refresh_action_enabled(
                            view.lifecycle
                                .as_ref()
                                .map(|lifecycle| lifecycle.task_phase),
                            record.status,
                            record.pr_url.is_some(),
                        )
                    }),
                egui::Button::new(
                    RichText::new("Refresh review, QA, and specification checks")
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
        return;
    }
    if let Some(lifecycle) = view.lifecycle.as_ref()
        && lifecycle.task_phase != crate::core::lifecycle::TaskPhase::Done
    {
        ui.label(match lifecycle.task_phase {
            crate::core::lifecycle::TaskPhase::AwaitingRuntimeQa => {
                "The updated head has passed independent code review. Complete runtime QA before PR authorization."
            }
            crate::core::lifecycle::TaskPhase::AwaitingSpecReconciliation => {
                "The updated head has passed code review and runtime QA. Complete specification reconciliation before PR authorization."
            }
            _ => "The updated head is being refreshed. PR authorization is unavailable until review, runtime QA, and specification reconciliation finish.",
        });
        if let Some(error) = view.lifecycle_pr_error.as_deref() {
            ui.colored_label(theme::DANGER, format!("PR preparation: {error}"));
        }
        return;
    }
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
                    crate::core::lifecycle::pull_request_publication_action_enabled(
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
