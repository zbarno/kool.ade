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
        if state.legacy_linkage {
            let status = legacy_status_label(state.execution_status.as_deref());
            ui.label(format!("Publication: Previously tracked as {status}"));
        } else {
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
}

fn legacy_status_label(status: Option<&str>) -> &'static str {
    match status {
        Some("preparing") => "preparing",
        Some("implementing") => "implementing",
        Some("verifying") => "verifying",
        Some("ready_to_publish") => "ready to share",
        Some("awaiting_approval") => "awaiting approval",
        Some("changes_requested") => "changes requested",
        Some("publishing") => "publishing",
        Some("waiting_for_independent_checks") => "waiting for project checks",
        Some("awaiting_review") => "awaiting review",
        Some("waiting_to_merge") => "waiting to merge",
        Some("blocked") => "needs attention",
        Some("completed") => "completed",
        Some("pull_request_closed") => "pull request closed",
        Some("interrupted") => "interrupted",
        _ => "unknown status",
    }
}

fn short_sha(sha: &str) -> &str {
    sha.get(..12).unwrap_or(sha)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_execution_status_is_labeled_as_historical_publication_state() {
        let mut identity = crate::domain::ArtifactIdentity::new("TASK-1", "Legacy task");
        identity.parent_uid = Some(uuid::Uuid::new_v4().to_string());
        let metadata =
            crate::artifacts::task_docs::TaskMetadata::new(&identity, "repo", vec![]).unwrap();
        let document = TaskDocument {
            path: ".koolade-packet/planning/tasks/legacy/001-task.md".into(),
            title: "Legacy task".into(),
            text: "# Legacy task".into(),
            identity: Some(identity),
            metadata: Some(metadata.clone()),
            task_state: Some(crate::artifacts::task_docs::TaskState {
                batch_uid: metadata.batch_uid,
                repository_id: "repo".into(),
                dependency_uids: Vec::new(),
                status: crate::core::planning_work::WorkStatus::InReview,
                execution_status: Some("awaiting_review".into()),
                source_branch: None,
                source_sha: None,
                destination_branch: None,
                implementation_branch: None,
                publication_status:
                    crate::artifacts::task_docs::ExecutionPublicationStatus::Planned,
                pull_requests: Vec::new(),
                legacy_linkage: true,
                revision: 4,
            }),
            metadata_error: None,
        };
        let context = egui::Context::default();
        let mut output = context.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(800.0, 600.0),
                )),
                ..Default::default()
            },
            |context| {
                egui::CentralPanel::default().show(context, |ui| paint(ui, &document));
            },
        );
        output.textures_delta.clear();
        let rendered = output
            .shapes
            .iter()
            .filter_map(|shape| match &shape.shape {
                egui::Shape::Text(text) => Some(text.galley.text().to_owned()),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert!(
            rendered
                .iter()
                .any(|text| text == "Publication: Previously tracked as awaiting review")
        );
    }
}
