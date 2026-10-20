use crate::{
    app::session::Project,
    project_runtime::{AppSurface, ProjectRuntimeStore},
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
        prepared_dependencies: Vec::new(),
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
    view.languages = manifest.detected_languages.clone();
    view.frameworks = manifest.frameworks.clone();
    view.manifest_revision = Some(manifest.revision);
    view.selected_target_id = manifest.selected_target_id.clone();
    view.prepared_dependencies =
        crate::app::root::runtime_dependencies::prepared_dependencies_for_ticket(project, ticket)
            .into_iter()
            .map(
                |dependency| crate::ui::task_detail::PreparedDependencyChoice {
                    request_id: dependency.request_id,
                    label: dependency.label,
                },
            )
            .collect();
    view.targets = manifest
        .candidate_targets
        .iter()
        .filter(|target| target.owning_task_id == identity.uid)
        .map(|target| RuntimeTargetChoice {
            target_id: target.target_id.clone(),
            app_surface: target.app_surface,
            detected_language: target_language(&target.target_id, &manifest.evidence_refs),
            label: format!(
                "{:?} · {}",
                target.app_surface, target.project_relative_root
            ),
            selected: manifest.is_active_target(&identity.uid, &target.target_id),
            state: if target
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
                .any(|recipe| !store.recipe_is_locally_approved(&manifest, target, recipe))
            {
                "UnauthorizedRecipe".into()
            } else {
                format!("{:?}", target.capability_state)
            },
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
            required_dependencies: target.required_dependencies.clone(),
            qa_applicability: surface_applicability(target.app_surface),
        })
        .collect();
    Some(view)
}

fn target_language(target_id: &str, evidence_refs: &[String]) -> Option<String> {
    evidence_refs.iter().find_map(|evidence| {
        if crate::project_runtime::target_id_for_evidence_path(evidence) != target_id {
            return None;
        }
        let path = std::path::Path::new(evidence);
        match path.file_name()?.to_str()? {
            "Cargo.toml" => Some("Rust".into()),
            "package.json" => Some("JavaScript".into()),
            "pyproject.toml" | "requirements.txt" => Some("Python".into()),
            "go.mod" => Some("Go".into()),
            _ => None,
        }
    })
}

fn surface_applicability(
    surface: AppSurface,
) -> crate::project_runtime_capability::QaApplicability {
    use crate::project_runtime_capability::QaApplicability;

    match surface {
        AppSurface::Web | AppSurface::LinuxDesktop => QaApplicability::Visual,
        AppSurface::HeadlessService => QaApplicability::ServiceEvidence,
        AppSurface::UnsupportedTarget => QaApplicability::Unsupported,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn target_language_uses_only_manifest_evidence_in_the_target_root() {
        let evidence = vec![
            "apps/web/package.json".to_owned(),
            "services/api/go.mod".to_owned(),
            "Cargo.toml".to_owned(),
            "apps/unknown/project.yaml".to_owned(),
        ];

        assert_eq!(
            target_language(
                &crate::project_runtime::target_id_for_evidence_path("apps/web/package.json"),
                &evidence,
            )
            .as_deref(),
            Some("JavaScript")
        );
        assert_eq!(
            target_language(
                &crate::project_runtime::target_id_for_evidence_path("services/api/go.mod"),
                &evidence,
            )
            .as_deref(),
            Some("Go")
        );
        assert_eq!(
            target_language(
                &crate::project_runtime::target_id_for_evidence_path("Cargo.toml"),
                &evidence,
            )
            .as_deref(),
            Some("Rust")
        );
        assert_eq!(target_language("unknown-target", &evidence), None);
    }

    #[test]
    fn target_language_does_not_confuse_same_root_polyglot_candidates() {
        let evidence = vec!["Cargo.toml".to_owned(), "package.json".to_owned()];
        let rust_target = crate::project_runtime::target_id_for_evidence_path("Cargo.toml");
        let node_target = crate::project_runtime::target_id_for_evidence_path("package.json");

        assert_ne!(rust_target, node_target);
        assert_eq!(
            target_language(&rust_target, &evidence).as_deref(),
            Some("Rust")
        );
        assert_eq!(
            target_language(&node_target, &evidence).as_deref(),
            Some("JavaScript")
        );
    }

    #[test]
    fn surface_applicability_keeps_service_and_unsupported_targets_out_of_visual_qa() {
        use crate::project_runtime_capability::QaApplicability;

        assert_eq!(
            surface_applicability(AppSurface::Web),
            QaApplicability::Visual
        );
        assert_eq!(
            surface_applicability(AppSurface::LinuxDesktop),
            QaApplicability::Visual
        );
        assert_eq!(
            surface_applicability(AppSurface::HeadlessService),
            QaApplicability::ServiceEvidence
        );
        assert_eq!(
            surface_applicability(AppSurface::UnsupportedTarget),
            QaApplicability::Unsupported
        );
    }
}
