use crate::ui::Surface;

pub(super) fn paint(
    ui: &mut egui::Ui,
    surface: &mut dyn Surface,
    work: &crate::core::planning_work::Work,
    offer: &crate::core::planning_work::FollowUpTaskOffer,
) {
    let id = egui::Id::new(("koolade_related_task_git_intent", &work.uid));
    let repositories = surface.repository_git_options();
    let mut selections = ui
        .ctx()
        .data_mut(|data| data.get_temp::<Selections>(id))
        .unwrap_or_else(|| {
            Selections(crate::ui::layout::new_task::git::drafts_for_intents(
                &repositories,
                &work.git_intents,
            ))
        });
    let defaults =
        crate::ui::layout::new_task::git::drafts_for_intents(&repositories, &work.git_intents);
    for (repository_id, selection) in defaults {
        selections.0.entry(repository_id).or_insert(selection);
    }
    ui.collapsing("Related task", |ui| {
        ui.label(&offer.description);
        if repositories.is_empty() {
            ui.label(crate::ui::theme::helper_text(
                "No code repository checkout is available for this task.",
            ));
            return;
        }
        crate::ui::layout::new_task::git::paint_repository_options(
            ui,
            &repositories,
            &mut selections.0,
            &work.git_intents.keys().cloned().collect(),
        );
        let has_selection = repositories
            .iter()
            .any(|repository| selections.0[&repository.id].enabled);
        let all_valid = repositories.iter().all(|repository| {
            let selection = &selections.0[&repository.id];
            !selection.enabled
                || (repository.unavailable_reason.is_none()
                    && repository
                        .source_branches
                        .contains(&selection.source_branch)
                    && repository
                        .destination_branches
                        .contains(&selection.destination_branch))
        });
        let valid = has_selection && all_valid;
        if ui
            .add_enabled(valid, egui::Button::new("Create related Feature task"))
            .clicked()
        {
            surface.dispatch(crate::ui::ApplicationCommand::CreatePlanningTask {
                kind: crate::core::planning_work::WorkKind::Feature,
                description: offer.description.clone(),
                parent_uid: Some(work.uid.clone()),
                git_intents: crate::ui::layout::new_task::git::selected_intents(&selections.0),
                routing_overrides: Default::default(),
            });
        }
    });
    ui.ctx().data_mut(|data| data.insert_temp(id, selections));
}

#[derive(Clone, Default)]
struct Selections(
    std::collections::BTreeMap<String, crate::ui::layout::new_task::git::RepositoryDraft>,
);
