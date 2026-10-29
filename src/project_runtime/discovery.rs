use std::path::Path;

use sha2::Digest;

use super::{
    AppSurface, AppTargetV1, ExecutionRecipeV1, ProjectCapabilityManifestV1, TargetCapabilityState,
};

mod scan;
pub use scan::{DiscoveryEvidence, detect_candidate_targets};

pub(crate) fn target_id_for_evidence_path(evidence_path: &str) -> String {
    let digest = format!("{:x}", sha2::Sha256::digest(evidence_path.as_bytes()));
    format!("target-{}", &digest[..20])
}

/// Identity pinned by the task controller before inspecting a project tree.
/// Discovery never infers repository, task, or source revision from project files.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DiscoveryContext {
    pub workspace_id: String,
    pub repository_id: String,
    pub owning_task_id: String,
    pub source_sha: String,
}

/// Produces advisory targets from bounded metadata inspection. No recipes are
/// synthesized or executed: each discovered target remains untested until an
/// approved recipe and executor capability are supplied.
pub fn discover_manifest(
    root: &Path,
    context: &DiscoveryContext,
) -> Result<ProjectCapabilityManifestV1, String> {
    let evidence = detect_candidate_targets(root)?;
    let mut languages = std::collections::BTreeSet::new();
    let mut frameworks = std::collections::BTreeSet::new();
    let mut candidate_targets = Vec::with_capacity(evidence.len());
    let mut evidence_refs = Vec::with_capacity(evidence.len());
    let mut confidence = 0;

    for item in evidence {
        languages.insert(item.language.clone());
        if let Some(framework) = &item.framework {
            frameworks.insert(framework.clone());
        }
        confidence = confidence.max(item.confidence);
        evidence_refs.push(item.evidence_path.clone());
        evidence_refs.extend(item.supporting_evidence_paths.iter().cloned());
        let path = Path::new(&item.evidence_path);
        let project_relative_root = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .map(|parent| parent.to_string_lossy().replace('\\', "/"))
            .unwrap_or_else(|| ".".into());
        let capability_state = if item.candidate_surface == AppSurface::UnsupportedTarget {
            TargetCapabilityState::UnsupportedPlatform
        } else {
            TargetCapabilityState::ReadyButUntested
        };
        candidate_targets.push(AppTargetV1 {
            target_id: target_id_for_evidence_path(&item.evidence_path),
            owning_task_id: context.owning_task_id.clone(),
            repository_id: context.repository_id.clone(),
            app_surface: item.candidate_surface,
            project_relative_root,
            build_recipe_ref: None,
            test_recipe_refs: Vec::new(),
            start_recipe_ref: None,
            readiness_probe_ref: None,
            artifact_spec_ref: None,
            fixture_ref: None,
            required_dependencies: Vec::new(),
            supported_driver_ids: Vec::new(),
            port_needs: Vec::new(),
            environment_policy_ref: "default-deny".into(),
            capability_state,
        });
    }

    let manifest = ProjectCapabilityManifestV1 {
        schema_version: 1,
        workspace_id: context.workspace_id.clone(),
        repository_id: context.repository_id.clone(),
        source_sha: context.source_sha.clone(),
        detected_languages: languages.into_iter().collect(),
        frameworks: frameworks.into_iter().collect(),
        target_capabilities: candidate_targets
            .iter()
            .map(|target| target.capability_state.clone())
            .collect(),
        candidate_targets,
        selected_target_id: None,
        recipes: Vec::<ExecutionRecipeV1>::new(),
        readiness_probes: Default::default(),
        artifact_specs: Default::default(),
        confidence,
        evidence_refs,
        user_overrides: Vec::new(),
        revision: 0,
    };
    manifest.validate()?;
    Ok(manifest)
}

/// Binds an explicit, validated recipe to one target and operation. This is
/// deliberately separate from discovery so project metadata cannot authorize
/// execution by itself.
pub fn apply_user_recipe_override(
    manifest: &mut ProjectCapabilityManifestV1,
    target_id: &str,
    operation: super::RuntimeOperation,
    recipe: ExecutionRecipeV1,
) -> Result<(), String> {
    recipe.validate()?;
    if recipe.source != super::RecipeSource::UserApproved {
        return Err("project recipe override must carry explicit user approval".into());
    }
    if manifest
        .recipes
        .iter()
        .chain(manifest.user_overrides.iter())
        .any(|existing| existing.recipe_id == recipe.recipe_id)
    {
        return Err("project recipe identifier is already in use".into());
    }
    let mut candidate = manifest.clone();
    let target = candidate
        .candidate_targets
        .iter_mut()
        .find(|target| target.target_id == target_id)
        .ok_or_else(|| "selected runtime target is missing".to_owned())?;
    if target.app_surface == AppSurface::UnsupportedTarget {
        return Err("unsupported application targets cannot be authorized by a recipe".into());
    }
    if recipe
        .working_directory_relative
        .split('/')
        .any(|part| part == ".git")
    {
        return Err("project recipe cannot run from Git metadata".into());
    }
    match operation {
        super::RuntimeOperation::Build => target.build_recipe_ref = Some(recipe.recipe_id.clone()),
        super::RuntimeOperation::Test => target.test_recipe_refs = vec![recipe.recipe_id.clone()],
        super::RuntimeOperation::Start => target.start_recipe_ref = Some(recipe.recipe_id.clone()),
    }
    target.capability_state = TargetCapabilityState::ReadyButUntested;
    candidate.user_overrides.push(recipe);
    candidate.validate()?;
    *manifest = candidate;
    Ok(())
}

/// Adds an explicitly selected target for an unrecognized or ambiguous stack.
/// The caller must verify the approval reference against the authenticated user
/// event before invoking this controller API.
pub fn add_user_target_override(
    manifest: &mut ProjectCapabilityManifestV1,
    mut target: AppTargetV1,
    recipes: Vec<ExecutionRecipeV1>,
    approval_ref: &str,
) -> Result<(), String> {
    let mut candidate = manifest.clone();
    target.validate()?;
    if target.repository_id != candidate.repository_id
        || approval_ref.trim().is_empty()
        || approval_ref.len() > 256
        || approval_ref.contains('\0')
        || candidate
            .candidate_targets
            .iter()
            .any(|existing| existing.target_id == target.target_id)
        || recipes.is_empty()
    {
        return Err("user-selected runtime target or approval reference is invalid".into());
    }
    for recipe in &recipes {
        recipe.validate()?;
        if recipe.source != super::RecipeSource::UserApproved
            || !recipe
                .evidence_refs
                .iter()
                .any(|reference| reference == approval_ref)
        {
            return Err("user-selected recipes must cite the explicit approval reference".into());
        }
        if candidate
            .recipes
            .iter()
            .chain(candidate.user_overrides.iter())
            .any(|existing| existing.recipe_id == recipe.recipe_id)
        {
            return Err("project recipe identifier is already in use".into());
        }
    }
    let recipe_ids = recipes
        .iter()
        .map(|recipe| recipe.recipe_id.as_str())
        .collect::<std::collections::BTreeSet<_>>();
    if target
        .build_recipe_ref
        .iter()
        .chain(target.test_recipe_refs.iter())
        .chain(target.start_recipe_ref.iter())
        .any(|recipe_id| !recipe_ids.contains(recipe_id.as_str()))
    {
        return Err("user-selected target references a recipe outside its approval".into());
    }
    target.capability_state = if target.app_surface == AppSurface::UnsupportedTarget {
        TargetCapabilityState::UnsupportedPlatform
    } else {
        TargetCapabilityState::ReadyButUntested
    };
    candidate.evidence_refs.push(approval_ref.into());
    candidate.user_overrides.extend(recipes);
    candidate.candidate_targets.push(target);
    candidate.target_capabilities.push(
        candidate
            .candidate_targets
            .last()
            .unwrap()
            .capability_state
            .clone(),
    );
    candidate.validate()?;
    *manifest = candidate;
    Ok(())
}
