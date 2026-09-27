//! Build or validate an ordered, project-specific product document set.
use std::{fs, path::Path};

use super::plan::Generated;
use crate::artifacts::layout::{ArtifactLayout, canonical, legacy};
use crate::artifacts::product_docs::{CoreConcept, ProductManifest, ProductModule};

pub(super) fn bootstrap_files(
    repo: &Path,
    title_override: Option<&str>,
) -> anyhow::Result<Vec<Generated>> {
    let layout = ArtifactLayout::new(repo);
    let old = directory_state(&layout.legacy_product_root())?;
    let current = directory_state(&layout.product_root())?;
    if current {
        let manifest = if layout.product_manifest().is_file() {
            crate::artifacts::product_docs::load_manifest(repo)?
        } else {
            crate::artifacts::product_docs::legacy_manifest(&layout.product_root())?
        }
        .ok_or_else(|| anyhow::anyhow!("Existing product documents have no manifest"))?;
        validate_product(&layout.product_root(), &manifest)?;
        if layout.product_manifest().is_file() {
            return Ok(Vec::new());
        }
        return Ok(vec![manifest_file(&manifest)]);
    }
    if old {
        let root = layout.legacy_product_root();
        let manifest = crate::artifacts::product_docs::legacy_manifest(&root)?
            .ok_or_else(|| anyhow::anyhow!("Legacy product documents are incomplete"))?;
        validate_product(&root, &manifest)?;
        return Ok(vec![manifest_file(&manifest)]);
    }

    let source = layout.legacy_specification();
    let existing = match fs::read_to_string(&source) {
        Ok(text) => Some(text),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(error.into()),
    };
    let (title, archive_note, documents, manifest) = match existing {
        Some(text) => documents_from_spec(&text)?,
        None => {
            let title = title_override.map(str::to_owned).unwrap_or_else(|| {
                repo.file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "project".into())
            });
            let template = crate::artifacts::spec_doc::bootstrap_template(&title);
            let sections = crate::artifacts::product_docs::split_sections(&template)?;
            let manifest = ProductManifest::core();
            let documents = sections
                .into_iter()
                .map(|(_, body)| body)
                .collect::<Vec<_>>();
            (
                title,
                "Packet created this initial structure; describe the project in chat to begin.",
                documents,
                manifest,
            )
        }
    };

    let mut files = Vec::new();
    anyhow::ensure!(
        documents.len() == manifest.modules.len(),
        "Product documents and manifest entries do not match"
    );
    for (module, body) in manifest.modules.iter().zip(documents) {
        crate::artifacts::product_docs::validate_module(&body)?;
        files.push(Generated {
            target: format!("{}/{}", canonical::PRODUCT, module.path),
            bytes: body.into_bytes(),
        });
    }
    let mut index = format!(
        "# {title} — Living Technical Specification\n\nThe modules below are the current product authority. {archive_note}\n\n## Product modules\n\n"
    );
    for module in &manifest.modules {
        index.push_str(&format!("- [{}]({})\n", module.title, module.path));
    }
    index.push_str("\n## Active features\n\n");
    append_active_features(repo, &layout, &mut index)?;
    files.push(Generated {
        target: canonical::PRODUCT_INDEX.into(),
        bytes: index.into_bytes(),
    });
    files.push(manifest_file(&manifest));
    Ok(files)
}

fn documents_from_spec(
    text: &str,
) -> anyhow::Result<(String, &'static str, Vec<String>, ProductManifest)> {
    let (documents, modules) = match crate::artifacts::product_docs::split_legacy(text) {
        Ok(legacy) => {
            let mut modules = Vec::new();
            for (index, (path, body)) in crate::artifacts::product_docs::LEGACY_MODULES
                .iter()
                .zip(legacy.iter())
                .enumerate()
            {
                let id = path.trim_end_matches(".md");
                let title = crate::artifacts::product_docs::module_title(body).unwrap_or(id);
                let mut module = ProductModule::new(id, title, legacy_core_concept(index));
                module.path = (*path).to_owned();
                modules.push(module);
            }
            (legacy.into_iter().collect::<Vec<_>>(), modules)
        }
        Err(_) => {
            let sections = crate::artifacts::product_docs::split_sections(text)?;
            let titles = sections
                .iter()
                .map(|(title, _)| title.as_str())
                .collect::<Vec<_>>();
            for concept in CoreConcept::all() {
                anyhow::ensure!(
                    titles
                        .iter()
                        .filter(|title| **title == concept.title())
                        .count()
                        == 1,
                    "Legacy specification must contain the required {} concept exactly once",
                    concept.title()
                );
            }
            let mut used = std::collections::BTreeSet::new();
            let mut modules = Vec::new();
            let mut documents = Vec::new();
            for (title, body) in sections {
                let concept = CoreConcept::all()
                    .into_iter()
                    .find(|concept| concept.title() == title);
                let base =
                    concept.map_or_else(|| slug_id(&title), |concept| concept.id().to_owned());
                let mut id = base.clone();
                let mut suffix = 2;
                while !used.insert(id.clone()) {
                    id = format!("{base}-{suffix}");
                    suffix += 1;
                }
                anyhow::ensure!(
                    crate::artifacts::product_docs::valid_id(&id),
                    "Cannot make a safe product module ID from section {title:?}"
                );
                modules.push(ProductModule::new(&id, &title, concept));
                documents.push(body);
            }
            (documents, modules)
        }
    };
    let title = text
        .lines()
        .find_map(|line| line.strip_prefix("# "))
        .unwrap_or("Product")
        .trim_end_matches(" — Living Technical Specification")
        .to_owned();
    Ok((
        title,
        "The pre-migration source is archived and retained in git history.",
        documents,
        ProductManifest::with_modules(modules),
    ))
}

fn slug_id(title: &str) -> String {
    let mut slug = String::new();
    for character in title.chars().flat_map(char::to_lowercase) {
        if character.is_ascii_lowercase() || character.is_ascii_digit() {
            slug.push(character);
        } else if !slug.is_empty() && !slug.ends_with('-') {
            slug.push('-');
        }
    }
    slug.trim_matches('-').to_owned()
}

fn legacy_core_concept(index: usize) -> Option<CoreConcept> {
    match index {
        0 => Some(CoreConcept::Overview),
        2 => Some(CoreConcept::UsersAndOutcomes),
        4 => Some(CoreConcept::CurrentCapabilities),
        7 => Some(CoreConcept::ArchitectureAndConstraints),
        9 => Some(CoreConcept::Decisions),
        5 => Some(CoreConcept::QualityAndAcceptance),
        _ => None,
    }
}

fn append_active_features(
    repo: &Path,
    layout: &ArtifactLayout,
    index: &mut String,
) -> anyhow::Result<()> {
    let changes = if repo.join(legacy::FEATURES).is_dir() {
        repo.join(legacy::FEATURES)
    } else {
        layout.changes_root()
    };
    let mut features = Vec::new();
    if changes.is_dir() {
        for entry in fs::read_dir(changes)? {
            let entry = entry?;
            let metadata = fs::symlink_metadata(entry.path())?;
            anyhow::ensure!(
                metadata.is_dir() && !metadata.file_type().is_symlink(),
                "Feature entry {} is not a real directory",
                entry.path().display()
            );
            let name = entry
                .file_name()
                .into_string()
                .map_err(|_| anyhow::anyhow!("Feature directory name is not valid UTF-8"))?;
            let feature = entry.path().join("specification.md");
            let Some(text) = fs::read_to_string(&feature).ok() else {
                continue;
            };
            let Some((id, title)) = text
                .lines()
                .find_map(|line| line.strip_prefix("# "))
                .and_then(|heading| heading.split_once(": "))
            else {
                continue;
            };
            if !crate::artifacts::product_docs::valid_feature_id(id) || title.trim().is_empty() {
                continue;
            }
            let status = crate::domain::ChangeMetadata::from_markdown(&text)?
                .map(|metadata| metadata.status)
                .map_or_else(
                    || crate::domain::ChangeMetadata::parse_legacy_markdown(&text),
                    Ok,
                )?;
            if !status.is_terminal() {
                features.push(name);
            }
        }
    }
    features.sort();
    if features.is_empty() {
        index.push_str("None.\n");
    } else {
        for feature in features {
            index.push_str(&format!(
                "- [`{feature}`](../changes/{feature}/specification.md)\n"
            ));
        }
    }
    Ok(())
}

fn manifest_file(manifest: &ProductManifest) -> Generated {
    Generated {
        target: canonical::PRODUCT_MANIFEST.into(),
        bytes: serde_json::to_vec_pretty(manifest).expect("product manifest serializes"),
    }
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
