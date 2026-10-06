use super::*;

fn is_approval_status(status: crate::core::implementation::ImplementationStatus) -> bool {
    matches!(
        status,
        crate::core::implementation::ImplementationStatus::ReadyToPublish
            | crate::core::implementation::ImplementationStatus::AwaitingApproval
    )
}

pub(super) fn approval_required(
    record: Option<&crate::core::implementation::Implementation>,
) -> bool {
    record.is_some_and(|record| is_approval_status(record.status) && record.pr_url.is_none())
}

pub(super) fn changes_requested(
    record: Option<&crate::core::implementation::Implementation>,
) -> bool {
    record.is_some_and(|record| {
        record.status == crate::core::implementation::ImplementationStatus::ChangesRequested
    })
}

pub(super) fn paint_approval(ui: &mut egui::Ui, s: &mut dyn Surface, ticket: &str) {
    ui.label("Implementation is complete and verified. Approve to create a pull request, or request changes.");
    if ui.button("Approve and create pull request").clicked() {
        s.dispatch(crate::ui::ApplicationCommand::ApprovePublication {
            ticket: ticket.to_owned(),
        });
    }
    if ui.button("Request changes").clicked() {
        s.dispatch(crate::ui::ApplicationCommand::RequestPublicationChanges {
            ticket: ticket.to_owned(),
        });
    }
}

pub(super) fn paint_changes_requested(ui: &mut egui::Ui) {
    ui.label("Changes were requested before PR approval. Describe the requested changes below to resume implementation; another approval will be required before a PR is created.");
}
