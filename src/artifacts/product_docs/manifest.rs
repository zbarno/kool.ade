use crate::artifacts::planning_store::PlanningRoot;
use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use super::{safe_module_path, valid_id};

const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CoreConcept {
    Overview,
    UsersAndOutcomes,
    CurrentCapabilities,
    ArchitectureAndConstraints,
    Decisions,
    QualityAndAcceptance,
}

impl CoreConcept {
    pub fn id(self) -> &'static str {
        match self {
            Self::Overview => "overview",
            Self::UsersAndOutcomes => "users-and-outcomes",
            Self::CurrentCapabilities => "current-capabilities",
            Self::ArchitectureAndConstraints => "architecture-and-constraints",
            Self::Decisions => "decisions",
            Self::QualityAndAcceptance => "quality-and-acceptance",
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Self::Overview => "Overview",
            Self::UsersAndOutcomes => "Users and Outcomes",
            Self::CurrentCapabilities => "Current Capabilities",
            Self::ArchitectureAndConstraints => "Architecture and Constraints",
            Self::Decisions => "Decisions",
            Self::QualityAndAcceptance => "Quality and Acceptance",
        }
    }

    pub fn all() -> [Self; 6] {
        [
            Self::Overview,
            Self::UsersAndOutcomes,
            Self::CurrentCapabilities,
            Self::ArchitectureAndConstraints,
            Self::Decisions,
            Self::QualityAndAcceptance,
        ]
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProductModule {
    pub uid: String,
    pub id: String,
    pub title: String,
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub core_concept: Option<CoreConcept>,
}

impl ProductModule {
    pub fn new(id: &str, title: &str, core_concept: Option<CoreConcept>) -> Self {
        Self {
            uid: uuid::Uuid::new_v4().hyphenated().to_string(),
            id: id.to_owned(),
            title: title.to_owned(),
            path: format!("{id}.md"),
            core_concept,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProductManifest {
    pub schema_version: u32,
    pub modules: Vec<ProductModule>,
}

impl ProductManifest {
    pub fn with_modules(modules: Vec<ProductModule>) -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            modules,
        }
    }

    pub fn core() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            modules: CoreConcept::all()
                .into_iter()
                .map(|concept| ProductModule::new(concept.id(), concept.title(), Some(concept)))
                .collect(),
        }
    }

    pub fn validate(&self, root: &Path, check_files: bool) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.schema_version == SCHEMA_VERSION,
            "Unsupported product manifest schema {}",
            self.schema_version
        );
        let mut ids = BTreeSet::new();
        let mut paths = BTreeSet::new();
        let mut uids = BTreeSet::new();
        let mut concepts = BTreeSet::new();
        for module in &self.modules {
            anyhow::ensure!(
                valid_id(&module.id),
                "Invalid product module ID {}",
                module.id
            );
            anyhow::ensure!(
                !module.title.trim().is_empty(),
                "Product module {} has no title",
                module.id
            );
            anyhow::ensure!(
                uuid::Uuid::parse_str(&module.uid).is_ok() && uids.insert(&module.uid),
                "Product module {} has a missing, invalid or duplicate UID",
                module.id
            );
            anyhow::ensure!(
                ids.insert(&module.id),
                "Duplicate product module ID {}",
                module.id
            );
            anyhow::ensure!(
                module.path == format!("{}.md", module.id)
                    && safe_module_path(root, &module.path).is_some(),
                "Product module {} has an unsafe source path",
                module.id
            );
            anyhow::ensure!(
                paths.insert(module.path.clone()),
                "Duplicate product module path {}",
                module.path
            );
            if let Some(concept) = module.core_concept {
                anyhow::ensure!(
                    concepts.insert(concept),
                    "Duplicate required product concept {}",
                    concept.id()
                );
            }
            if check_files {
                let path = safe_module_path(root, &module.path).unwrap();
                let meta = fs::symlink_metadata(&path)?;
                anyhow::ensure!(
                    meta.is_file() && !meta.file_type().is_symlink(),
                    "Product module {} is not a regular file",
                    module.path
                );
            }
        }
        anyhow::ensure!(!self.modules.is_empty(), "Product manifest has no modules");
        for required in CoreConcept::all() {
            anyhow::ensure!(
                concepts.contains(&required),
                "Product manifest is missing required concept {}",
                required.id()
            );
        }
        if check_files && let Ok(entries) = fs::read_dir(root) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().is_none_or(|extension| extension != "md")
                    || path.file_name().is_some_and(|name| name == "index.md")
                {
                    continue;
                }
                let name = path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or_default();
                anyhow::ensure!(
                    paths.contains(name),
                    "Product module file {name} is not registered in the manifest"
                );
            }
        }
        Ok(())
    }
}

pub fn read<R: PlanningRoot + ?Sized>(repo: &R) -> anyhow::Result<Option<ProductManifest>> {
    let layout = repo.planning_layout();
    let root = layout.product_root();
    let path = layout.product_manifest();
    match fs::symlink_metadata(&root) {
        Ok(meta) => anyhow::ensure!(
            meta.is_dir() && !meta.file_type().is_symlink(),
            "Product root must be a real directory"
        ),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    }
    match fs::symlink_metadata(&path) {
        Ok(meta) => {
            anyhow::ensure!(
                meta.is_file() && !meta.file_type().is_symlink(),
                "Product manifest must be a regular file"
            );
            let bytes = repo.read_planning_path(&path)?;
            let manifest: ProductManifest = serde_json::from_slice(&bytes)?;
            manifest.validate(&root, true)?;
            Ok(Some(manifest))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let unregistered = fs::read_dir(&root).ok().is_some_and(|entries| {
                entries.flatten().any(|entry| {
                    entry
                        .path()
                        .extension()
                        .is_some_and(|extension| extension == "md")
                        && entry.file_name() != "index.md"
                })
            });
            anyhow::ensure!(
                !unregistered,
                "Product module manifest is missing; migrate or restore the manifest before editing"
            );
            Ok(None)
        }
        Err(error) => Err(error.into()),
    }
}

pub fn legacy_manifest(root: &Path) -> anyhow::Result<Option<ProductManifest>> {
    let mut modules = Vec::new();
    for (index, file) in super::LEGACY_MODULES.iter().enumerate() {
        let path = root.join(file);
        let meta = match fs::symlink_metadata(&path) {
            Ok(meta) => meta,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        anyhow::ensure!(
            meta.is_file() && !meta.file_type().is_symlink(),
            "Legacy product module {file} must be a regular file"
        );
        let text = fs::read_to_string(&path)?;
        let title = super::schema::module_title(&text).unwrap_or(file.trim_end_matches(".md"));
        modules.push(ProductModule {
            uid: uuid::Uuid::new_v4().hyphenated().to_string(),
            id: file.trim_end_matches(".md").to_owned(),
            title: title.to_owned(),
            path: file.to_string(),
            core_concept: legacy_concept(index),
        });
    }
    let manifest = ProductManifest::with_modules(modules);
    manifest.validate(root, true)?;
    Ok(Some(manifest))
}

fn legacy_concept(index: usize) -> Option<CoreConcept> {
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
