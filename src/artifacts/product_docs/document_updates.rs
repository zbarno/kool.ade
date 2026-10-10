//! Resolve logical document IDs and prepare manifest changes for model output.
use super::{ProductManifest, ProductModule};
use crate::artifacts::planning_store::PlanningRoot;
use std::path::PathBuf;

pub fn document_path_for_update<R: PlanningRoot + ?Sized>(
    repo: &R,
    id: &str,
    content: &str,
) -> anyhow::Result<PathBuf> {
    if let Some(module_id) = id.strip_prefix("product:") {
        anyhow::ensure!(
            module_id != "index",
            "The product index is application-maintained"
        );
        anyhow::ensure!(super::valid_id(module_id), "Invalid product module ID");
        super::validate_module(content)?;
        let layout = repo.planning_layout();
        anyhow::ensure!(
            super::feature_index::real_dir(&layout.product_root())?,
            "Product root does not exist"
        );
        let mut manifest = super::load_manifest(repo)?
            .ok_or_else(|| anyhow::anyhow!("Product module manifest is missing"))?;
        if let Some(module) = manifest
            .modules
            .iter_mut()
            .find(|module| module.id == module_id)
        {
            module.title = super::module_title(content)
                .ok_or_else(|| anyhow::anyhow!("Product module title is missing"))?
                .to_owned();
            return super::safe_module_path(&layout.product_root(), &module.path)
                .ok_or_else(|| anyhow::anyhow!("Product module path is outside the product root"));
        }
        let root = layout.product_root();
        let module = ProductModule::new(
            module_id,
            super::module_title(content).unwrap_or(module_id),
            None,
        );
        let path = super::safe_module_path(&root, &module.path)
            .ok_or_else(|| anyhow::anyhow!("Product module path is outside the product root"))?;
        anyhow::ensure!(
            !path.exists(),
            "Product module ID {module_id} collides with an unregistered file"
        );
        manifest.modules.push(module);
        manifest.validate(&root, false)?;
        return Ok(path);
    }
    if let Ok(existing) = document_path(repo, id) {
        return Ok(existing);
    }
    let feature_id = id
        .strip_prefix("feature:")
        .ok_or_else(|| anyhow::anyhow!("Unknown logical document ID"))?;
    anyhow::ensure!(
        feature_id == super::feature_index::next_feature_id(repo),
        "New feature ID must be application-assigned next ID"
    );
    crate::core::specification::validate_feature(feature_id, content)?;
    let title = content
        .lines()
        .find_map(|line| line.strip_prefix(&format!("# {feature_id}: ")))
        .ok_or_else(|| anyhow::anyhow!("Feature title missing"))?;
    let slug = crate::artifacts::task_docs::slug(title);
    let layout = repo.planning_layout();
    let root = layout.changes_root();
    let _ = super::feature_index::real_dir(&root)?;
    let dir = layout
        .change_directory(&format!("{feature_id}-{slug}"))
        .ok_or_else(|| anyhow::anyhow!("Invalid application-assigned feature path"))?;
    anyhow::ensure!(!dir.exists(), "Feature directory already exists");
    Ok(dir.join("specification.md"))
}

pub fn updated_manifest<R: PlanningRoot + ?Sized>(
    repo: &R,
    updates: &[(String, String)],
) -> anyhow::Result<ProductManifest> {
    let mut manifest = super::load_manifest(repo)?
        .ok_or_else(|| anyhow::anyhow!("Product module manifest is missing"))?;
    let root = repo.planning_layout().product_root();
    for (document_id, content) in updates {
        let Some(id) = document_id.strip_prefix("product:") else {
            continue;
        };
        if id == "index" {
            continue;
        }
        anyhow::ensure!(super::valid_id(id), "Invalid product module ID {id}");
        super::validate_module(content)?;
        let title = super::module_title(content)
            .ok_or_else(|| anyhow::anyhow!("Product module title is missing"))?;
        if let Some(module) = manifest.modules.iter_mut().find(|module| module.id == id) {
            module.title = title.to_owned();
        } else {
            let module = ProductModule::new(id, title, None);
            let path = super::safe_module_path(&root, &module.path).ok_or_else(|| {
                anyhow::anyhow!("Product module path is outside the product root")
            })?;
            anyhow::ensure!(
                !path.exists(),
                "Product module ID {id} collides with an unregistered file"
            );
            manifest.modules.push(module);
        }
    }
    manifest.validate(&root, false)?;
    Ok(manifest)
}

pub fn preserved_ids(old: &str, new: &str) -> anyhow::Result<()> {
    fn definitions(text: &str) -> std::collections::BTreeSet<String> {
        text.lines()
            .filter_map(|line| {
                let line = line.trim_start();
                let candidate = line
                    .strip_prefix("| ")
                    .or_else(|| line.strip_prefix("- **"))
                    .or_else(|| line.strip_prefix("- "))?;
                let id = candidate
                    .split(|c: char| !c.is_ascii_alphanumeric() && c != '-')
                    .next()?;
                let (prefix, digits) = id.rsplit_once('-')?;
                (matches!(prefix, "G" | "F" | "FR" | "NFR" | "D" | "CLR")
                    && !digits.is_empty()
                    && digits.bytes().all(|b| b.is_ascii_digit()))
                .then(|| id.to_string())
            })
            .collect()
    }
    let missing = definitions(old)
        .difference(&definitions(new))
        .cloned()
        .collect::<Vec<_>>();
    anyhow::ensure!(
        missing.is_empty(),
        "Stable identifiers removed: {}",
        missing.join(", ")
    );
    Ok(())
}

pub fn document_path<R: PlanningRoot + ?Sized>(repo: &R, id: &str) -> anyhow::Result<PathBuf> {
    if id == "product:index" {
        return Ok(repo.planning_layout().product_index());
    }
    if let Some(name) = id.strip_prefix("product:") {
        let layout = repo.planning_layout();
        anyhow::ensure!(
            super::feature_index::real_dir(&layout.product_root())?,
            "Product root does not exist"
        );
        let manifest = super::load_manifest(repo)?
            .ok_or_else(|| anyhow::anyhow!("Product module manifest is missing"))?;
        let module = manifest
            .modules
            .iter()
            .find(|module| module.id == name)
            .ok_or_else(|| anyhow::anyhow!("Unknown product module ID {name}"))?;
        return super::safe_module_path(&layout.product_root(), &module.path)
            .ok_or_else(|| anyhow::anyhow!("Product module path is outside the product root"));
    }
    if let Some(id) = id.strip_prefix("feature:") {
        anyhow::ensure!(
            super::feature_index::valid_feature_id(id),
            "Invalid feature document ID"
        );
        let layout = repo.planning_layout();
        let root = layout.changes_root();
        anyhow::ensure!(
            super::feature_index::real_dir(&root)?,
            "Feature directory does not exist"
        );
        let mut matches = std::fs::read_dir(&root)?
            .map(|entry| entry.map(|entry| entry.path()))
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .filter(|path| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.starts_with(&format!("{id}-")))
            })
            .collect::<Vec<_>>();
        anyhow::ensure!(
            matches.len() == 1,
            "Feature ID must identify exactly one directory (found {} for {id})",
            matches.len()
        );
        let dir = matches.remove(0);
        anyhow::ensure!(
            super::feature_index::real_dir(&dir)?,
            "Feature directory is not a real directory"
        );
        return Ok(layout
            .change_specification(dir.file_name().and_then(|name| name.to_str()).unwrap())
            .expect("existing feature directory name is one path component"));
    }
    anyhow::bail!("Unknown logical document ID: {id}")
}
