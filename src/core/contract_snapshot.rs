//! Immutable inputs for one feature task batch. The planning root owns this
//! snapshot; checkout paths are deliberately excluded from the committed file.
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use crate::core::state::PlannerState;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchContract {
    pub feature_id: String,
    pub feature_specification: String,
    pub product_modules: BTreeMap<String, String>,
    pub repository_bases: BTreeMap<String, String>,
    pub configuration: String,
}

pub fn freeze(state: &PlannerState) -> anyhow::Result<Option<BatchContract>> {
    let Some((feature_id, feature)) = &state.active_feature else {
        return Ok(None);
    };
    anyhow::ensure!(
        crate::core::project_repos::ProjectManifest::load(&state.repo_root)? == state.repositories,
        "Project repository manifest changed during generation"
    );
    let layout = crate::artifacts::layout::ArtifactLayout::new(&state.repo_root);
    let config = std::fs::read_to_string(layout.legacy_project_config())?;
    anyhow::ensure!(
        crate::artifacts::config_io::parse(&config).map_err(anyhow::Error::msg)? == state.config,
        "Planning configuration changed during generation"
    );
    let mut product_modules = BTreeMap::new();
    for name in crate::artifacts::product_docs::MODULES {
        let id = name.trim_end_matches(".md");
        if feature.contains(name) || feature.contains(&format!("product:{id}")) {
            product_modules.insert(
                id.to_string(),
                std::fs::read_to_string(
                    layout
                        .legacy_product_module(name)
                        .expect("product module names are application-owned single components"),
                )?,
            );
        }
    }
    anyhow::ensure!(
        !product_modules.is_empty(),
        "Feature {feature_id} does not identify affected product modules"
    );
    let mut repository_bases = BTreeMap::new();
    for repository in &state.repositories.repositories {
        let checkout = state
            .repositories
            .target(&state.repo_root, &repository.id)?;
        let output = std::process::Command::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(checkout)
            .output()?;
        anyhow::ensure!(
            output.status.success(),
            "Cannot read base revision for {}",
            repository.id
        );
        repository_bases.insert(
            repository.id.clone(),
            String::from_utf8(output.stdout)?.trim().to_string(),
        );
    }
    Ok(Some(BatchContract {
        feature_id: feature_id.clone(),
        feature_specification: feature.clone(),
        product_modules,
        repository_bases,
        configuration: crate::artifacts::config_io::serialize(&state.config),
    }))
}
