use crate::{
    artifacts::task_docs::TaskDocument,
    core::git_intent::PublicationModeV1,
    ui::{ApplicationCommand, RepositoryGitOptions, Surface, theme},
};

#[derive(Clone)]
struct Draft {
    repository_id: String,
    source_ref: String,
    destination_ref: String,
    publication_mode: PublicationModeV1,
}

pub(super) fn paint(
    ui: &mut egui::Ui,
    surface: &mut dyn Surface,
    document: &TaskDocument,
    state: Option<&crate::core::implementation::Implementation>,
) {
    let Some(metadata) = document.metadata.as_ref() else {
        return;
    };
    let id = egui::Id::new(("koolade_task_git_intent_override", &metadata.uid));
    let default = Some(draft_from_metadata(metadata));
    let mut draft = ui.ctx().data_mut(|data| data.get_temp::<Draft>(id));
    if draft.is_none() {
        draft = default;
    }
    let Some(mut draft) = draft else {
        return;
    };
    if state.is_some() {
        ui.label(theme::helper_text(
            "Task Git intent is locked because implementation has started.",
        ));
        return;
    }
    let repositories = surface.repository_git_options();
    ui.collapsing("Review task Git intent", |ui| {
        if metadata.git_intent.is_none() {
            ui.label(theme::helper_text(
                "This task has no approved Git intent. Choose and approve one before implementation can start.",
            ));
        }
        if repositories.is_empty() {
            ui.label(theme::helper_text(
                "No code repository checkout is available for Git intent review.",
            ));
            return;
        }
        repository_picker(ui, &repositories, &mut draft);
        let Some(repository) = repositories
            .iter()
            .find(|repository| repository.id == draft.repository_id)
        else {
            ui.label(theme::helper_text("Select a mapped code repository."));
            return;
        };
        if let Some(reason) = &repository.unavailable_reason {
            ui.label(theme::helper_text(format!("Unavailable: {reason}")));
        } else {
            branch_picker(
                ui,
                "Source branch",
                &mut draft.source_ref,
                &repository.source_branches,
            );
            branch_picker(
                ui,
                "Destination branch",
                &mut draft.destination_ref,
                &repository.destination_branches,
            );
            ui.label(theme::helper_text("Publication mode"));
            egui::ComboBox::from_id_salt(("task-publication-mode", &metadata.uid))
                .selected_text(publication_mode_label(draft.publication_mode))
                .show_ui(ui, |ui| {
                    for mode in [
                        PublicationModeV1::HoldForReview,
                        PublicationModeV1::CreatePullRequest,
                        PublicationModeV1::AutoPublish,
                    ] {
                        ui.selectable_value(
                            &mut draft.publication_mode,
                            mode,
                            publication_mode_label(mode),
                        );
                    }
                });
            ui.label(theme::helper_text(
                "Approval pins the selected source commit and applies only to this task.",
            ));
            let valid = repository.source_branches.contains(&draft.source_ref)
                && repository
                    .destination_branches
                    .contains(&draft.destination_ref);
            if ui
                .add_enabled(valid, egui::Button::new("Approve task Git intent"))
                .clicked()
            {
                surface.dispatch(ApplicationCommand::ApproveTaskGitIntent {
                    ticket: document.path.clone(),
                    repository_id: draft.repository_id.clone(),
                    selection: crate::core::git_intent::GitIntentSelectionV1 {
                        source_ref: draft.source_ref.clone(),
                        destination_ref: draft.destination_ref.clone(),
                        publication_mode: draft.publication_mode,
                    },
                });
            }
        }
    });
    ui.ctx().data_mut(|data| data.insert_temp(id, draft));
}

fn draft_from_metadata(metadata: &crate::artifacts::task_docs::TaskMetadata) -> Draft {
    metadata
        .git_intent
        .as_ref()
        .map(|intent| Draft {
            repository_id: intent.repository_id.clone(),
            source_ref: intent.source_ref.clone(),
            destination_ref: intent.destination_ref.clone(),
            publication_mode: intent.publication_mode,
        })
        .unwrap_or_else(|| Draft {
            repository_id: metadata.repository_id.clone(),
            source_ref: metadata.source_branch.clone().unwrap_or_default(),
            destination_ref: metadata.destination_branch.clone().unwrap_or_default(),
            publication_mode: PublicationModeV1::HoldForReview,
        })
}

fn repository_picker(ui: &mut egui::Ui, repositories: &[RepositoryGitOptions], draft: &mut Draft) {
    egui::ComboBox::from_id_salt("task-git-intent-repository")
        .selected_text(
            repositories
                .iter()
                .find(|repository| repository.id == draft.repository_id)
                .map_or("Select repository", |repository| &repository.label),
        )
        .show_ui(ui, |ui| {
            for repository in repositories {
                ui.selectable_value(
                    &mut draft.repository_id,
                    repository.id.clone(),
                    &repository.label,
                );
            }
        });
}

fn branch_picker(ui: &mut egui::Ui, label: &str, selected: &mut String, branches: &[String]) {
    ui.label(theme::helper_text(label));
    egui::ComboBox::from_id_salt((label, selected.as_str()))
        .selected_text(if selected.is_empty() {
            "No branch available"
        } else {
            selected.as_str()
        })
        .width(ui.available_width())
        .wrap_mode(egui::TextWrapMode::Truncate)
        .show_ui(ui, |ui| {
            for branch in branches {
                ui.selectable_value(selected, branch.clone(), branch);
            }
        });
}

fn publication_mode_label(mode: PublicationModeV1) -> &'static str {
    match mode {
        PublicationModeV1::HoldForReview => "Hold for review",
        PublicationModeV1::CreatePullRequest => "Create pull request",
        PublicationModeV1::AutoPublish => "Auto publish",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_git_intent_can_be_reapproved_for_the_task_repository() {
        let metadata = serde_json::from_value::<crate::artifacts::task_docs::TaskMetadata>(
            serde_json::json!({
                "schemaVersion": 8,
                "uid": "00000000-0000-4000-8000-000000000001",
                "batchUid": "00000000-0000-4000-8000-000000000002",
                "repositoryId": "secondary",
                "dependencyUids": [],
                "sourceBranch": "legacy-source",
                "destinationBranch": "legacy-target"
            }),
        )
        .unwrap();

        let draft = draft_from_metadata(&metadata);

        assert_eq!(draft.repository_id, "secondary");
        assert_eq!(draft.source_ref, "legacy-source");
        assert_eq!(draft.destination_ref, "legacy-target");
        assert_eq!(draft.publication_mode, PublicationModeV1::HoldForReview);
    }
}
