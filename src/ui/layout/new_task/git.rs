use crate::ui::{RepositoryGitOptions, theme};
use egui::{RichText, Ui};
use std::collections::BTreeMap;

#[derive(Clone, Default)]
pub(crate) struct RepositoryDraft {
    pub(crate) enabled: bool,
    pub(crate) source_branch: String,
    pub(crate) destination_branch: String,
    pub(crate) publication_mode: crate::core::git_intent::PublicationModeV1,
}

pub(crate) fn paint_repository_options(
    ui: &mut Ui,
    repositories: &[RepositoryGitOptions],
    draft: &mut BTreeMap<String, RepositoryDraft>,
    inherited_repositories: &std::collections::BTreeSet<String>,
) {
    ui.label(RichText::new("Implementation repositories").strong());
    for repository in repositories {
        ui.group(|ui| {
            let selection = draft.get_mut(&repository.id).unwrap();
            if inherited_repositories.contains(&repository.id) {
                selection.enabled = true;
                ui.label(format!("Inherited · {}", repository.label));
            } else {
                ui.checkbox(&mut selection.enabled, &repository.label);
            }
            if let Some(reason) = &repository.unavailable_reason {
                ui.label(theme::helper_text(format!("Unavailable: {reason}")));
            } else if selection.enabled {
                ui.label(theme::helper_text(
                    "A matching local branch is selected first; remote-only branches must still exist on origin.",
                ));
                branch_picker(
                    ui,
                    &format!("{} source branch", repository.label),
                    &mut selection.source_branch,
                    &repository.source_branches,
                );
                branch_picker(
                    ui,
                    &format!("{} destination branch", repository.label),
                    &mut selection.destination_branch,
                    &repository.destination_branches,
                );
                ui.label(theme::helper_text("Publication mode"));
                egui::ComboBox::from_id_salt(("publication-mode", &repository.id))
                    .selected_text(publication_mode_label(selection.publication_mode))
                    .show_ui(ui, |ui| {
                        for mode in [
                            crate::core::git_intent::PublicationModeV1::HoldForReview,
                            crate::core::git_intent::PublicationModeV1::CreatePullRequest,
                            crate::core::git_intent::PublicationModeV1::AutoPublish,
                        ] {
                            ui.selectable_value(
                                &mut selection.publication_mode,
                                mode,
                                publication_mode_label(mode),
                            );
                        }
                    });
            }
        });
    }
}

pub(crate) fn selected_intents(
    draft: &BTreeMap<String, RepositoryDraft>,
) -> BTreeMap<String, crate::core::git_intent::GitIntentSelectionV1> {
    draft
        .iter()
        .filter(|(_, selection)| selection.enabled)
        .map(|(repository_id, selection)| {
            (
                repository_id.clone(),
                crate::core::git_intent::GitIntentSelectionV1 {
                    source_ref: selection.source_branch.clone(),
                    destination_ref: selection.destination_branch.clone(),
                    publication_mode: selection.publication_mode,
                },
            )
        })
        .collect()
}

pub(crate) fn drafts_for_intents(
    repositories: &[RepositoryGitOptions],
    intents: &BTreeMap<String, crate::core::git_intent::GitIntentV1>,
) -> BTreeMap<String, RepositoryDraft> {
    repositories
        .iter()
        .map(|repository| {
            let intent = intents.get(&repository.id);
            (
                repository.id.clone(),
                RepositoryDraft {
                    enabled: intent.is_some(),
                    source_branch: intent.map_or_else(
                        || repository.default_branch.clone(),
                        |intent| intent.source_ref.clone(),
                    ),
                    destination_branch: intent.map_or_else(
                        || repository.default_branch.clone(),
                        |intent| intent.destination_ref.clone(),
                    ),
                    publication_mode: intent
                        .map_or(Default::default(), |intent| intent.publication_mode),
                },
            )
        })
        .collect()
}

fn publication_mode_label(mode: crate::core::git_intent::PublicationModeV1) -> &'static str {
    match mode {
        crate::core::git_intent::PublicationModeV1::HoldForReview => "Hold for review",
        crate::core::git_intent::PublicationModeV1::CreatePullRequest => "Create pull request",
        crate::core::git_intent::PublicationModeV1::AutoPublish => "Auto publish",
    }
}

fn branch_picker(ui: &mut Ui, label: &str, selected: &mut String, branches: &[String]) {
    ui.label(theme::helper_text(label));
    let selected_text = if selected.is_empty() {
        "No branch available"
    } else {
        selected.as_str()
    };
    egui::ComboBox::from_id_salt(label)
        .selected_text(selected_text)
        .width(ui.available_width())
        .wrap_mode(egui::TextWrapMode::Truncate)
        .show_ui(ui, |ui| {
            for branch in branches {
                ui.selectable_value(selected, branch.clone(), branch);
            }
        });
}
