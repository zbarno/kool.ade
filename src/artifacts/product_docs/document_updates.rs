//! Resolve logical document IDs and prepare manifest changes for model output.
#[cfg(test)]
use super::{ProductManifest, ProductModule};
use crate::artifacts::planning_store::PlanningRoot;
use std::path::PathBuf;

/// Read a logical product or feature document from the current local draft,
/// then from the code repository's immutable published-spec pin.
pub fn current_document<R: PlanningRoot + ?Sized>(
    repo: &R,
    id: &str,
) -> anyhow::Result<Option<String>> {
    let store = repo.planning_store();
    let pin = pinned_reader(&store)?;
    if store.published_repository_root.is_none() {
        #[cfg(not(test))]
        return Ok(None);
    }
    if let Some(draft) = crate::core::apply::load_current(&store)? {
        let current_pin = pin.as_ref().map(|(_, pin)| pin);
        anyhow::ensure!(
            draft.base_published_spec_pin.as_ref() == current_pin,
            "local specification proposal is based on a stale published revision; review it against the current revision"
        );
        if let Some(change) = draft.changes.iter().find(|change| change.document_id == id) {
            return Ok(Some(change.content.clone()));
        }
    }

    if let Some((reader, pin)) = pin {
        return published_document(&reader, &pin, id);
    }

    // Only test adapters with local-only stores use the fixture layout.
    #[cfg(test)]
    if let Ok(path) = document_path(repo, id) {
        return match repo.read_planning_path(&path) {
            Ok(bytes) => Ok(Some(String::from_utf8(bytes)?)),
            Err(crate::artifacts::planning_store::StoreError::Io { source, .. })
                if source.kind() == std::io::ErrorKind::NotFound =>
            {
                Ok(None)
            }
            Err(error) => Err(error.into()),
        };
    }
    Ok(None)
}

fn pinned_reader(
    store: &crate::artifacts::planning_store::PlanningStore,
) -> anyhow::Result<
    Option<(
        crate::core::published_specs::PublishedSpecReader,
        crate::core::published_specs::PublishedSpecPin,
    )>,
> {
    let Some(code_root) = store.published_repository_root.as_deref() else {
        return Ok(None);
    };
    let reader =
        crate::core::published_specs::PublishedSpecReader::for_primary_repository(code_root)?;
    let pin = match &store.published_spec_pin {
        Some(pin) => pin.clone(),
        None => reader.pin()?,
    };
    Ok(Some((reader, pin)))
}

fn published_document(
    reader: &crate::core::published_specs::PublishedSpecReader,
    pin: &crate::core::published_specs::PublishedSpecPin,
    id: &str,
) -> anyhow::Result<Option<String>> {
    if let Some(module_id) = id.strip_prefix("product:") {
        anyhow::ensure!(
            module_id != "index" && super::valid_id(module_id),
            "Invalid product module ID"
        );
        let matches = reader
            .read_documentation(pin)?
            .into_iter()
            .filter(|document| {
                document.path.starts_with("docs/koolade/product/")
                    && document.path != "docs/koolade/product/index.md"
                    && std::path::Path::new(&document.path)
                        .file_stem()
                        .and_then(|stem| stem.to_str())
                        == Some(module_id)
            })
            .collect::<Vec<_>>();
        anyhow::ensure!(
            matches.len() <= 1,
            "published product module ID {module_id} is ambiguous"
        );
        return Ok(matches.into_iter().next().map(|document| document.content));
    }
    let feature_id = id
        .strip_prefix("feature:")
        .ok_or_else(|| anyhow::anyhow!("Unknown logical document ID"))?;
    anyhow::ensure!(
        super::feature_index::valid_feature_id(feature_id),
        "Invalid feature document ID"
    );
    let mut found = None;
    for document in reader
        .read_documentation(pin)?
        .into_iter()
        .filter(|document| {
            document.path.starts_with("docs/koolade/features/")
                && document.path.ends_with(".md")
                && !document.path.ends_with("/AGENTS.md")
        })
    {
        let Some(identity) = crate::domain::ArtifactIdentity::from_markdown(&document.content)?
        else {
            continue;
        };
        if identity.display_id != feature_id {
            continue;
        }
        let filename = std::path::Path::new(&document.path)
            .file_stem()
            .and_then(|value| value.to_str());
        anyhow::ensure!(
            filename == Some(identity.uid.as_str()),
            "published feature path does not match its immutable identity"
        );
        crate::core::specification::validate_feature(feature_id, &document.content)?;
        let metadata = crate::domain::ChangeMetadata::require_markdown(&document.content)?;
        anyhow::ensure!(
            metadata.uid == identity.uid && metadata.display_id == identity.display_id,
            "published feature metadata does not match its identity"
        );
        anyhow::ensure!(
            found.replace(document.content).is_none(),
            "published feature ID {feature_id} is ambiguous"
        );
    }
    Ok(found)
}

pub fn document_path_for_update<R: PlanningRoot + ?Sized>(
    repo: &R,
    id: &str,
    content: &str,
) -> anyhow::Result<PathBuf> {
    let store = repo.planning_store();
    if store.published_repository_root.is_some() {
        let previous = current_document(repo, id)?;
        if let Some(module_id) = id.strip_prefix("product:") {
            anyhow::ensure!(
                module_id != "index" && super::valid_id(module_id),
                "Invalid product module ID"
            );
            super::validate_module(content)?;
            if let Some(previous) = &previous {
                preserved_ids(previous, content)?;
            }
            return Ok(PathBuf::from(format!(
                "docs/koolade/product/{module_id}.md"
            )));
        }
        let feature_id = id
            .strip_prefix("feature:")
            .ok_or_else(|| anyhow::anyhow!("Unknown logical document ID"))?;
        anyhow::ensure!(
            super::feature_index::valid_feature_id(feature_id),
            "Invalid feature document ID"
        );
        crate::core::specification::validate_feature(feature_id, content)?;
        if previous.is_none() {
            anyhow::ensure!(
                feature_id == next_feature_id_for_update(repo)?,
                "New feature ID must be application-assigned next ID"
            );
            let identity = crate::domain::ArtifactIdentity::from_markdown(content)?
                .ok_or_else(|| anyhow::anyhow!("New feature has no stable identity"))?;
            return Ok(PathBuf::from(format!(
                "docs/koolade/features/{}.md",
                identity.uid
            )));
        }
        let identity = crate::domain::ArtifactIdentity::from_markdown(content)?
            .ok_or_else(|| anyhow::anyhow!("Feature has no stable identity"))?;
        return Ok(PathBuf::from(format!(
            "docs/koolade/features/{}.md",
            identity.uid
        )));
    }
    #[cfg(not(test))]
    anyhow::bail!("specification updates require the primary code repository");
    #[cfg(test)]
    {
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
                return super::safe_module_path(&layout.product_root(), &module.path).ok_or_else(
                    || anyhow::anyhow!("Product module path is outside the product root"),
                );
            }
            let root = layout.product_root();
            let module = ProductModule::new(
                module_id,
                super::module_title(content).unwrap_or(module_id),
                None,
            );
            let path = super::safe_module_path(&root, &module.path).ok_or_else(|| {
                anyhow::anyhow!("Product module path is outside the product root")
            })?;
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
}

pub fn next_feature_id_for_update<R: PlanningRoot + ?Sized>(repo: &R) -> anyhow::Result<String> {
    let store = repo.planning_store();
    let Some((reader, pin)) = pinned_reader(&store)? else {
        #[cfg(not(test))]
        anyhow::bail!("feature identifiers require the primary code repository");
        #[cfg(test)]
        return Ok(super::feature_index::next_local_feature_id(repo));
    };
    let mut maximum = 0u32;
    for document in reader
        .read_documentation(&pin)?
        .into_iter()
        .filter(|document| {
            document.path.starts_with("docs/koolade/features/")
                && document.path.ends_with(".md")
                && !document.path.ends_with("/AGENTS.md")
        })
    {
        let Some(identity) = crate::domain::ArtifactIdentity::from_markdown(&document.content)?
        else {
            continue;
        };
        let filename = std::path::Path::new(&document.path)
            .file_stem()
            .and_then(|value| value.to_str());
        anyhow::ensure!(
            filename == Some(identity.uid.as_str()),
            "published feature path does not match its immutable identity"
        );
        let metadata = crate::domain::ChangeMetadata::require_markdown(&document.content)?;
        anyhow::ensure!(
            metadata.uid == identity.uid && metadata.display_id == identity.display_id,
            "published feature metadata does not match its identity"
        );
        if let Some(number) = identity
            .display_id
            .strip_prefix('F')
            .or_else(|| identity.display_id.strip_prefix("CHG-"))
            .and_then(|digits| digits.parse::<u32>().ok())
        {
            maximum = maximum.max(number);
        }
    }
    if let Some(draft) = crate::core::apply::load_current(&store)? {
        anyhow::ensure!(
            draft.base_published_spec_pin.as_ref() == Some(&pin),
            "local specification proposal is based on a stale published revision; review it against the current revision"
        );
        for change in draft.changes {
            if let Some(id) = change.document_id.strip_prefix("feature:")
                && let Some(number) = id
                    .strip_prefix('F')
                    .or_else(|| id.strip_prefix("CHG-"))
                    .and_then(|digits| digits.parse::<u32>().ok())
            {
                maximum = maximum.max(number);
            }
        }
    }
    Ok(format!("F{}", maximum + 1))
}

#[cfg(test)]
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
    #[cfg(not(test))]
    {
        let _ = (repo, id);
        anyhow::bail!("published documents are addressed by pinned Git paths");
    }
    #[cfg(test)]
    {
        anyhow::ensure!(
            repo.planning_store().published_repository_root.is_none(),
            "Published documents must be read through the pinned specification reader"
        );
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
}
