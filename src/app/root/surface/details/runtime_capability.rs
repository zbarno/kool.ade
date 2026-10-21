use crate::{
    app::session::Project,
    execution::docker::images::ImageLock,
    project_runtime::{ProjectRuntimeStore, TargetCapabilityState},
    ui::task_detail::{RuntimeCapabilityView, RuntimeTargetChoice},
};

pub(super) fn for_ticket(project: &Project, ticket: &str) -> Option<RuntimeCapabilityView> {
    let document = project
        .task_documents
        .iter()
        .find(|document| document.path == ticket)?;
    let identity = document.metadata.as_ref()?;
    let implementation = project.implementation_states.get(ticket)?;
    let source_sha = implementation.verified_head.clone();
    let mut view = RuntimeCapabilityView {
        source_sha: source_sha.clone(),
        languages: Vec::new(),
        frameworks: Vec::new(),
        manifest_revision: None,
        selected_target_id: None,
        targets: Vec::new(),
        message: None,
    };
    let store = ProjectRuntimeStore::new(&project.state.planning_store);
    let manifest = match store.load_manifest() {
        Ok(Some(manifest)) => manifest,
        Ok(None) => {
            view.message = Some(
                "No project runtime targets have been discovered for this source revision.".into(),
            );
            return Some(view);
        }
        Err(error) => {
            view.message = Some(format!(
                "Runtime capability state could not be read: {error}"
            ));
            return Some(view);
        }
    };
    if manifest.workspace_id != implementation.workspace_uid.as_deref().unwrap_or_default()
        || manifest.repository_id != identity.repository_id
        || source_sha.as_deref() != Some(manifest.source_sha.as_str())
    {
        view.message = Some(
            "Saved runtime targets belong to a different task workspace, repository, or source revision. Discover them again for this task.".into(),
        );
        return Some(view);
    }
    let receipts = match store.receipts_for_task(&identity.uid) {
        Ok(receipts) => receipts,
        Err(error) => {
            view.message = Some(format!(
                "Runtime recipe receipts could not be read: {error}"
            ));
            return Some(view);
        }
    };
    view.languages = manifest.detected_languages.clone();
    view.frameworks = manifest.frameworks.clone();
    view.manifest_revision = Some(manifest.revision);
    view.selected_target_id = manifest.selected_target_id.clone();
    let inventory = ImageLock::bundled().ok().map(|lock| {
        super::super::runtime_capability_inventory::RuntimeCapabilityInventory::from_locked_images(
            &lock,
        )
    });
    if inventory.is_none() {
        view.message = Some(
            "Pinned runtime image inventory is unavailable; target readiness cannot be assessed."
                .into(),
        );
    }
    view.targets = manifest
        .candidate_targets
        .iter()
        .filter(|target| target.owning_task_id == identity.uid)
        .map(|target| {
            let available_dependencies = inventory
                .as_ref()
                .map(|_| {
                    super::super::runtime_capability_inventory::prepared_dependency_ids_for_target(
                        &receipts,
                        &identity.uid,
                        &target.target_id,
                        &manifest.source_sha,
                        target.start_recipe_ref.as_deref().unwrap_or_default(),
                        &target.required_dependencies,
                    )
                })
                .unwrap_or_default();
            let locally_unauthorized = target
                .build_recipe_ref
                .iter()
                .chain(target.test_recipe_refs.iter())
                .chain(target.start_recipe_ref.iter())
                .filter_map(|id| {
                    manifest
                        .recipes
                        .iter()
                        .chain(manifest.user_overrides.iter())
                        .find(|recipe| recipe.recipe_id == *id)
                })
                .any(|recipe| !store.recipe_is_locally_approved(&manifest, target, recipe));
            let assessment = inventory.as_ref().map(|inventory| {
                inventory.assess_target(
                    &manifest,
                    &target.target_id,
                    &identity.uid,
                    &available_dependencies,
                )
            });
            let diagnostics = match assessment.as_ref() {
                Some(Ok(assessment)) => assessment
                    .report
                    .gaps
                    .iter()
                    .map(|gap| gap.remediation())
                    .collect(),
                Some(Err(error)) => vec![format!("Capability assessment failed closed: {error}.")],
                None => vec![
                    "Pinned runtime image inventory is unavailable; readiness is unknown.".into(),
                ],
            };
            let state = if locally_unauthorized {
                TargetCapabilityState::UnauthorizedRecipe
            } else if let Some(Ok(assessment)) = assessment.as_ref() {
                if assessment.capability_state == TargetCapabilityState::Ready
                    && target.capability_state == TargetCapabilityState::ReadyButUntested
                {
                    TargetCapabilityState::ReadyButUntested
                } else {
                    assessment.capability_state.clone()
                }
            } else {
                target.capability_state.clone()
            };
            RuntimeTargetChoice {
                target_id: target.target_id.clone(),
                label: format!(
                    "{:?} · {}",
                    target.app_surface, target.project_relative_root
                ),
                surface: target.app_surface,
                selected: manifest.is_active_target(&identity.uid, &target.target_id),
                state: format!("{state:?}"),
                diagnostics,
                recipes: [
                    target
                        .build_recipe_ref
                        .as_deref()
                        .map(|id| format!("build: {id}")),
                    (!target.test_recipe_refs.is_empty())
                        .then(|| format!("test: {}", target.test_recipe_refs.join(", "))),
                    target
                        .start_recipe_ref
                        .as_deref()
                        .map(|id| format!("start: {id}")),
                ]
                .into_iter()
                .flatten()
                .collect::<Vec<_>>()
                .join(" · "),
            }
        })
        .collect();
    Some(view)
}
