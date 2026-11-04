use crate::ui::Surface;

pub(super) fn paint(
    ui: &mut egui::Ui,
    surface: &mut dyn Surface,
    work: &crate::core::planning_work::Work,
) -> bool {
    let repositories = surface.repository_git_options();
    let id = egui::Id::new(("koolade_feature_git_intents", &work.uid));
    let intents = work
        .feature_id
        .as_deref()
        .map(|feature_id| surface.feature_git_intents(feature_id))
        .filter(|intents| !intents.is_empty())
        .unwrap_or_else(|| work.git_intents.clone());
    let mut drafts = ui
        .ctx()
        .data_mut(|data| data.get_temp::<Drafts>(id))
        .unwrap_or_else(|| {
            Drafts(crate::ui::layout::new_task::git::drafts_for_intents(
                &repositories,
                &intents,
            ))
        });
    let defaults = crate::ui::layout::new_task::git::drafts_for_intents(&repositories, &intents);
    for (repository_id, draft) in defaults {
        drafts.0.entry(repository_id).or_insert(draft);
    }
    let selected = crate::ui::layout::new_task::git::selected_intents(&drafts.0);
    let valid = !selected.is_empty()
        && repositories.iter().all(|repository| {
            let Some(selection) = selected.get(&repository.id) else {
                return true;
            };
            repository.unavailable_reason.is_none()
                && repository.source_branches.contains(&selection.source_ref)
                && repository
                    .destination_branches
                    .contains(&selection.destination_ref)
        });
    let matches_approved = !selected.is_empty()
        && selected.len() == intents.len()
        && selected.iter().all(|(repository_id, selection)| {
            intents.get(repository_id).is_some_and(|intent| {
                intent.source_ref == selection.source_ref
                    && intent.destination_ref == selection.destination_ref
                    && intent.publication_mode == selection.publication_mode
            })
        });
    ui.collapsing("Repository Git intent", |ui| {
        ui.label("Choose each repository, source commit, destination, and publication mode before generating tasks.");
        crate::ui::layout::new_task::git::paint_repository_options(
            ui,
            &repositories,
            &mut drafts.0,
            &std::collections::BTreeSet::new(),
        );
        if ui
            .add_enabled(valid, egui::Button::new("Approve repository choices"))
            .clicked()
        {
            surface.dispatch(crate::ui::ApplicationCommand::ApproveFeatureGitIntents {
                work_key: work.key.clone(),
                feature_id: work.feature_id.clone().unwrap_or_default(),
                selections: selected.clone(),
            });
        }
    });
    ui.ctx().data_mut(|data| data.insert_temp(id, drafts));
    matches_approved
}

#[derive(Clone, Default)]
struct Drafts(
    std::collections::BTreeMap<String, crate::ui::layout::new_task::git::RepositoryDraft>,
);
