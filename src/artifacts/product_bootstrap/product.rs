//! Build or validate the current ordered product-document set.
use std::{fs, path::Path};

use crate::artifacts::layout::store;
use crate::artifacts::planning_store::PlanningStore;
use crate::artifacts::product_docs::ProductManifest;

mod active_features;
use active_features::append_active_features;

#[derive(Debug, Clone)]
pub(super) struct Generated {
    pub(super) target: String,
    pub(super) bytes: Vec<u8>,
}

pub(super) fn bootstrap_files_with_store(
    _repo: &Path,
    store: &PlanningStore,
    title: &str,
) -> anyhow::Result<Vec<Generated>> {
    let layout = store.layout();
    let root = layout.product_root();
    if directory_state(&root)? {
        let manifest = crate::artifacts::product_docs::load_manifest(store)?
            .ok_or_else(|| anyhow::anyhow!("Existing product documents have no manifest"))?;
        validate_product(&root, &manifest)?;
        return Ok(Vec::new());
    }

    let title = title.to_owned();
    let template = crate::artifacts::spec_doc::bootstrap_template(&title);
    let documents = crate::artifacts::product_docs::split_sections(&template)?
        .into_iter()
        .map(|(_, body)| body)
        .collect::<Vec<_>>();
    let manifest = ProductManifest::core();
    anyhow::ensure!(
        documents.len() == manifest.modules.len(),
        "Product documents and manifest entries do not match"
    );

    let mut files = Vec::new();
    for (module, body) in manifest.modules.iter().zip(documents) {
        crate::artifacts::product_docs::validate_module(&body)?;
        files.push(Generated {
            target: format!("{}/{}", store::PRODUCT, module.path),
            bytes: body.into_bytes(),
        });
    }
    let mut index = format!(
        "# {title} — Living Technical Specification\n\nThe modules below are the current product authority. Describe the project in chat to develop this initial draft.\n\n## Product modules\n\n"
    );
    for module in &manifest.modules {
        index.push_str(&format!("- [{}]({})\n", module.title, module.path));
    }
    index.push_str("\n## Active features\n\n");
    append_active_features(store, &layout, &mut index)?;
    files.push(Generated {
        target: store::PRODUCT_INDEX.into(),
        bytes: index.into_bytes(),
    });
    files.push(Generated {
        target: store::PRODUCT_MANIFEST.into(),
        bytes: serde_json::to_vec_pretty(&manifest).expect("product manifest serializes"),
    });
    Ok(files)
}

fn directory_state(path: &Path) -> anyhow::Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(meta) => {
            anyhow::ensure!(
                meta.is_dir() && !meta.file_type().is_symlink(),
                "Product directory {} must be a real directory",
                path.display()
            );
            Ok(true)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error.into()),
    }
}

fn validate_product(root: &Path, manifest: &ProductManifest) -> anyhow::Result<()> {
    let index = root.join("index.md");
    let metadata = fs::symlink_metadata(&index)?;
    anyhow::ensure!(
        metadata.is_file() && !metadata.file_type().is_symlink(),
        "Product index {} must be a regular file",
        index.display()
    );
    manifest.validate(root, true)?;
    for module in &manifest.modules {
        let path = root.join(&module.path);
        let content = fs::read_to_string(path)?;
        crate::artifacts::product_docs::validate_module(&content)?;
        anyhow::ensure!(
            crate::artifacts::product_docs::module_title(&content) == Some(module.title.as_str()),
            "Product module {} title differs from its manifest",
            module.id
        );
    }
    Ok(())
}
