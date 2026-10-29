use std::collections::BTreeSet;

use crate::{
    app::session::Project,
    project_runtime::{AppSurface, ProjectCapabilityManifestV1, ProjectRuntimeStore},
    project_runtime_capability::{
        ApprovedCapabilityInventory, CapabilityGap, assess_manifest_target,
    },
    ui::task_detail::{RuntimeCapabilityView, RuntimeTargetChoice},
};

mod toolchain_profiles;
use toolchain_profiles::{
    LockedCapabilities, incompatible_recipe_toolchain_profiles, locked_capabilities,
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
    let available_dependency_ids = view
        .prepared_dependencies
        .iter()
        .map(|dependency| dependency.request_id.clone())
        .collect::<BTreeSet<_>>();
    let approved_inventory = locked_capabilities();
    let receipts = match store.receipts_for_task(&identity.uid) {
        Ok(receipts) => receipts,
        Err(error) => {
            view.message = Some(format!("Runtime build status could not be read: {error}"));
            Vec::new()
        }
    };
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
                format!(
                    "{:?}",
                    crate::project_runtime_capability::observed_build_capability_state(
                        target,
                        &identity.uid,
                        &manifest.source_sha,
                        &receipts,
                    )
                    .unwrap_or_else(|| target.capability_state.clone())
                )
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
            diagnostics: match &approved_inventory {
                Ok(capabilities) => capability_diagnostics(
                    &manifest,
                    &target.target_id,
                    &identity.uid,
                    capabilities,
                    &available_dependency_ids,
                )
                .unwrap_or_else(|error| vec![error]),
                Err(error) => vec![format!(
                    "Approved runtime image capabilities could not be loaded ({error}); review the locked toolchain and driver inventory."
                )],
            },
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

fn capability_diagnostics(
    manifest: &ProjectCapabilityManifestV1,
    target_id: &str,
    task_id: &str,
    capabilities: &LockedCapabilities,
    available_dependency_ids: &BTreeSet<String>,
) -> Result<Vec<String>, String> {
    let inventory = ApprovedCapabilityInventory {
        manifest,
        available_tool_ids: &capabilities.tool_ids,
        available_dependency_ids,
        available_driver_ids: &capabilities.driver_ids,
    };
    let assessment = assess_manifest_target(manifest, target_id, task_id, &inventory)
        .map_err(|error| format!("Runtime capability assessment failed: {error}"))?;
    let mut diagnostics = assessment
        .report
        .gaps
        .iter()
        .map(CapabilityGap::remediation)
        .collect::<Vec<_>>();
    diagnostics.extend(incompatible_recipe_toolchain_profiles(
        manifest,
        target_id,
        &capabilities.tool_ids,
        &capabilities.profile_tool_ids,
    ));
    Ok(diagnostics)
}

#[cfg(test)]
#[path = "runtime_capability/tests.rs"]
mod tests;
