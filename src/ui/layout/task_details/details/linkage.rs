use crate::artifacts::task_docs::TaskDocument;

pub(super) fn paint(ui: &mut egui::Ui, document: &TaskDocument) {
    let Some(metadata) = document.metadata.as_ref() else {
        return;
    };
    ui.separator();
    ui.label(crate::ui::theme::section_heading("Code target"));
    ui.label(format!("Repository: {}", metadata.repository_id));

    let state = document.task_state.as_ref();
    if let Some(source) = state
        .and_then(|state| state.source_branch.as_deref())
        .or(metadata.source_branch.as_deref())
    {
        let sha = state.and_then(|state| state.source_sha.as_deref());
        ui.label(match sha {
            Some(sha) => format!("Source: {source} at {}", short_sha(sha)),
            None => format!("Source: {source}"),
        });
    }
    if let Some(destination) = state
        .and_then(|state| state.destination_branch.as_deref())
        .or(metadata.destination_branch.as_deref())
    {
        ui.label(format!("Destination: {destination}"));
    }
    if let Some(branch) = state.and_then(|state| state.implementation_branch.as_deref()) {
        ui.label(format!("Implementation branch: {branch}"));
    }
    if let Some(state) = state {
        ui.label(format!("Publication: {}", state.publication_status.label()));
        for pull_request in &state.pull_requests {
            ui.horizontal_wrapped(|ui| {
                ui.label(format!(
                    "PR #{} · {} · {}",
                    pull_request.number,
                    pull_request.status.label(),
                    pull_request.base_branch
                ));
                ui.hyperlink_to("Open", &pull_request.url);
            });
        }
    }
}

fn short_sha(sha: &str) -> &str {
    sha.get(..12).unwrap_or(sha)
}
