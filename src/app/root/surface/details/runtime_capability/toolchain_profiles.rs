use std::collections::BTreeSet;

use crate::{
    execution::docker::images::{ImageLock, RuntimeProfile},
    project_runtime::{ExecutionRecipeV1, ProjectCapabilityManifestV1},
};

pub(super) struct LockedCapabilities {
    pub(super) tool_ids: BTreeSet<String>,
    pub(super) profile_tool_ids: Vec<BTreeSet<String>>,
    pub(super) driver_ids: BTreeSet<String>,
}

pub(super) fn locked_capabilities() -> Result<LockedCapabilities, String> {
    let lock = ImageLock::bundled().map_err(|failure| failure.message)?;
    let profiles = [
        RuntimeProfile::General,
        RuntimeProfile::Rust,
        RuntimeProfile::Node,
        RuntimeProfile::Python,
        RuntimeProfile::Go,
    ];
    let mut tool_ids = BTreeSet::new();
    let mut profile_tool_ids = Vec::new();
    for profile in profiles {
        for architecture in ["amd64", "arm64"] {
            if let Ok(image) = lock.select(profile, architecture) {
                let mut tools = image
                    .tool_versions
                    .into_keys()
                    .filter(|tool| !matches!(tool.as_str(), "base" | "shell"))
                    .collect::<BTreeSet<_>>();
                if tools.contains("python") {
                    tools.insert("python3".into());
                }
                tool_ids.extend(tools.iter().cloned());
                profile_tool_ids.push(tools);
            }
        }
    }
    let driver_ids = [
        ("web-driver", RuntimeProfile::BrowserQa),
        ("linux-desktop-driver", RuntimeProfile::DesktopQa),
    ]
    .into_iter()
    .filter(|(_, profile)| {
        ["amd64", "arm64"]
            .into_iter()
            .any(|architecture| lock.select(*profile, architecture).is_ok())
    })
    .map(|(id, _)| id.to_owned())
    .collect();
    Ok(LockedCapabilities {
        tool_ids,
        profile_tool_ids,
        driver_ids,
    })
}

pub(super) fn incompatible_recipe_toolchain_profiles(
    manifest: &ProjectCapabilityManifestV1,
    target_id: &str,
    available_tool_ids: &BTreeSet<String>,
    profile_tool_ids: &[BTreeSet<String>],
) -> Vec<String> {
    let Some(target) = manifest
        .candidate_targets
        .iter()
        .find(|target| target.target_id == target_id)
    else {
        return Vec::new();
    };
    let mut seen_recipe_ids = BTreeSet::new();
    target
        .build_recipe_ref
        .iter()
        .chain(target.test_recipe_refs.iter())
        .chain(target.start_recipe_ref.iter())
        .filter(|recipe_id| seen_recipe_ids.insert((*recipe_id).clone()))
        .filter_map(|recipe_id| {
            let recipe = manifest
                .user_overrides
                .iter()
                .chain(manifest.recipes.iter())
                .find(|recipe| recipe.recipe_id == *recipe_id)?;
            incompatible_recipe_message(recipe, available_tool_ids, profile_tool_ids)
        })
        .collect()
}

fn incompatible_recipe_message(
    recipe: &ExecutionRecipeV1,
    available_tool_ids: &BTreeSet<String>,
    profile_tool_ids: &[BTreeSet<String>],
) -> Option<String> {
    let required_tools = &recipe.prerequisite_tool_ids;
    if !recipe.is_explicitly_user_approved()
        || required_tools.is_empty()
        || !required_tools
            .iter()
            .all(|tool| available_tool_ids.contains(tool))
        || profile_tool_ids.iter().any(|profile| {
            required_tools
                .iter()
                .all(|tool| profile_provides(profile, tool))
        })
    {
        return None;
    }
    Some(format!(
        "Recipe `{}` requires [{}], but no single locked executor image contains the full tool set. Request an approved compatible profile or split the work into recipes that each fit one profile.",
        recipe.recipe_id,
        required_tools.join(", ")
    ))
}

fn profile_provides(profile: &BTreeSet<String>, tool_id: &str) -> bool {
    profile.contains(tool_id) || (tool_id == "python3" && profile.contains("python"))
}
