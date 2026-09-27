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

fn references_module(feature: &str, id: &str, path: &str, title: &str) -> bool {
    feature.contains(path)
        || feature.contains(&format!("product:{id}"))
        || feature.contains(title)
        || id
            .split_once('-')
            .and_then(|(number, _)| number.parse::<u32>().ok())
            .is_some_and(|number| mentions_module_number(feature, number))
}

fn mentions_module_number(text: &str, number: u32) -> bool {
    let normalized = text.to_ascii_lowercase();
    [format!("module {number:02}"), format!("module {number}")]
        .iter()
        .any(|label| {
            normalized.match_indices(label).any(|(start, _)| {
                let before = normalized[..start].chars().next_back();
                let end = start + label.len();
                let after = normalized[end..].chars().next();
                before.is_none_or(|ch| !ch.is_ascii_alphanumeric())
                    && after.is_none_or(|ch| !ch.is_ascii_digit())
            })
        })
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
    let config = std::fs::read_to_string(layout.project_config())?;
    anyhow::ensure!(
        crate::artifacts::config_io::parse(&config).map_err(anyhow::Error::msg)? == state.config,
        "Planning configuration changed during generation"
    );
    let mut product_modules = BTreeMap::new();
    let modules = crate::artifacts::product_docs::load_documents(&state.repo_root)?
        .ok_or_else(|| anyhow::anyhow!("Product modules are missing"))?;
    for document in modules {
        if references_module(
            feature,
            &document.module.id,
            &document.module.path,
            &document.module.title,
        ) {
            product_modules.insert(document.module.id, document.content);
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

#[cfg(test)]
mod tests;
