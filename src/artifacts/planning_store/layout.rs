use std::path::{Component, Path, PathBuf};

use super::{PlanningStore, StoreMode};

/// Resolved planning paths. The root is the only root owned by a planning
/// store; legacy project paths are available only for compatibility reads.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlanningLayout {
    root: PathBuf,
    legacy_repository: Option<PathBuf>,
}

pub mod paths {
    pub const MANIFEST: &str = "manifest.json";
    pub const CONFIG: &str = "config";
    pub const PROJECT_CONFIG: &str = "config/project.md";
    pub const PROJECT_MANIFEST: &str = "config/repositories.json";
    pub const PLANNING: &str = "planning";
    pub const PRODUCT: &str = "planning/product";
    pub const PRODUCT_INDEX: &str = "planning/product/index.md";
    pub const PRODUCT_MANIFEST: &str = "planning/product/manifest.json";
    pub const CHANGES: &str = "planning/changes";
    pub const DECISIONS: &str = "planning/decisions";
    pub const OPEN_ITEMS: &str = "planning/open-items.md";
    pub const RESOLVED_ITEMS: &str = "planning/resolved-items.json";
    pub const IMPORTS: &str = "planning/imports";
    pub const TASKS: &str = "planning/tasks";
    pub const WORKFLOW: &str = "state/workflow.json";
    pub const WORK: &str = "state/work.json";
    pub const CANCELLED_WORK: &str = "state/cancelled-work.json";
    pub const STATE: &str = "state";
    pub const IMPLEMENTATION: &str = "implementation";
    pub const ARCHIVE: &str = "planning/archive";
    pub const LEGACY_SPEC_ARCHIVE: &str = "planning/archive/specification-pre-modules.md";
}

impl PlanningLayout {
    pub(super) fn for_store(store: &PlanningStore) -> Self {
        Self {
            root: store.root.clone(),
            legacy_repository: (store.mode == StoreMode::LegacyEmbedded)
                .then(|| store.root.parent().map(Path::to_path_buf))
                .flatten(),
        }
    }

    pub(super) fn legacy_repository(repository: &Path) -> Self {
        Self {
            root: repository.join(crate::artifacts::layout::canonical::ROOT),
            legacy_repository: Some(repository.to_path_buf()),
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Accepts the old `.koolade-packet/...` spelling or a new root-relative
    /// path. The returned path is always underneath this planning root.
    pub fn canonical_path(&self, relative: &str) -> Option<PathBuf> {
        crate::artifacts::layout::ArtifactLayout::resolve_planning_root_relative(
            &self.root, relative,
        )
    }

    pub fn legacy_path(&self, relative: &str) -> Option<PathBuf> {
        safe_path(self.legacy_repository.as_deref()?, relative)
    }

    pub fn manifest(&self) -> PathBuf {
        self.at(paths::MANIFEST)
    }
    pub fn config_root(&self) -> PathBuf {
        self.at(paths::CONFIG)
    }
    pub fn project_config(&self) -> PathBuf {
        self.at(paths::PROJECT_CONFIG)
    }
    pub fn project_manifest(&self) -> PathBuf {
        self.at(paths::PROJECT_MANIFEST)
    }
    pub fn planning_root(&self) -> PathBuf {
        self.at(paths::PLANNING)
    }
    pub fn product_root(&self) -> PathBuf {
        self.at(paths::PRODUCT)
    }
    pub fn product_index(&self) -> PathBuf {
        self.at(paths::PRODUCT_INDEX)
    }
    pub fn product_manifest(&self) -> PathBuf {
        self.at(paths::PRODUCT_MANIFEST)
    }
    pub fn product_module(&self, name: &str) -> Option<PathBuf> {
        safe_component(name).then(|| self.product_root().join(name))
    }
    pub fn changes_root(&self) -> PathBuf {
        self.at(paths::CHANGES)
    }
    pub fn change_specification(&self, id: &str) -> Option<PathBuf> {
        safe_component(id).then(|| self.changes_root().join(id).join("specification.md"))
    }
    pub fn change_directory(&self, name: &str) -> Option<PathBuf> {
        safe_component(name).then(|| self.changes_root().join(name))
    }
    pub fn change_link(&self, name: &str) -> Option<String> {
        let path = self.change_specification(name)?;
        let relative = path.strip_prefix(self.planning_root()).ok()?;
        Some(format!("../{}", relative.display()))
    }
    pub fn decisions_root(&self) -> PathBuf {
        self.at(paths::DECISIONS)
    }
    pub fn decision_record(&self, name: &str) -> Option<PathBuf> {
        safe_component(name).then(|| self.decisions_root().join(name))
    }
    pub fn open_items(&self) -> PathBuf {
        self.at(paths::OPEN_ITEMS)
    }
    pub fn resolved_items(&self) -> PathBuf {
        self.at(paths::RESOLVED_ITEMS)
    }
    pub fn imports_root(&self) -> PathBuf {
        self.at(paths::IMPORTS)
    }
    pub fn imports_path(&self, name: &str) -> Option<PathBuf> {
        safe_component(name).then(|| self.imports_root().join(name))
    }
    pub fn tasks_root(&self) -> PathBuf {
        self.at(paths::TASKS)
    }
    pub fn task_batch(&self, name: &str) -> Option<PathBuf> {
        safe_component(name).then(|| self.tasks_root().join(name))
    }
    pub fn workflow_state(&self) -> PathBuf {
        self.at(paths::WORKFLOW)
    }
    pub fn work_state(&self) -> PathBuf {
        self.at(paths::WORK)
    }
    pub fn archive_root(&self) -> PathBuf {
        self.at(paths::ARCHIVE)
    }
    pub fn implementation_root(&self) -> PathBuf {
        self.at(paths::IMPLEMENTATION)
    }

    pub fn legacy_specification(&self) -> PathBuf {
        self.legacy("planning/specification.md")
    }
    pub fn legacy_planning_root(&self) -> PathBuf {
        self.legacy("planning")
    }
    pub fn legacy_product_root(&self) -> PathBuf {
        self.legacy("planning/product")
    }
    pub fn legacy_product_index(&self) -> PathBuf {
        self.legacy("planning/product/index.md")
    }
    pub fn legacy_features_root(&self) -> PathBuf {
        self.legacy("planning/features")
    }
    pub fn legacy_feature_directory(&self, name: &str) -> Option<PathBuf> {
        safe_component(name).then(|| self.legacy_features_root().join(name))
    }
    pub fn legacy_feature_specification(&self, name: &str) -> Option<PathBuf> {
        self.legacy_feature_directory(name)
            .map(|directory| directory.join("specification.md"))
    }
    pub fn legacy_feature_link(&self, name: &str) -> Option<String> {
        let path = self.legacy_feature_specification(name)?;
        let relative = path.strip_prefix(self.legacy_planning_root()).ok()?;
        Some(format!("../{}", relative.display()))
    }
    pub fn legacy_product_module(&self, name: &str) -> Option<PathBuf> {
        safe_component(name).then(|| self.legacy_product_root().join(name))
    }
    pub fn legacy_open_items(&self) -> PathBuf {
        self.legacy("planning/open-items.md")
    }
    pub fn legacy_resolved_items(&self) -> PathBuf {
        self.legacy("planning/resolved-items.json")
    }
    pub fn legacy_imports_root(&self) -> PathBuf {
        self.legacy("planning/imports")
    }
    pub fn legacy_import_path(&self, name: &str) -> Option<PathBuf> {
        safe_component(name).then(|| self.legacy_imports_root().join(name))
    }
    pub fn legacy_import_relative(&self, name: &str) -> Option<String> {
        self.legacy_import_path(name)?
            .strip_prefix(self.legacy_repository.as_deref()?)
            .ok()
            .map(|path| path.to_string_lossy().into_owned())
    }
    pub fn legacy_tasks_root(&self) -> PathBuf {
        self.legacy("planning/tasks")
    }
    pub fn legacy_config_root(&self) -> PathBuf {
        self.legacy(".planner")
    }
    pub fn legacy_archive_root(&self) -> PathBuf {
        self.legacy("planning/archive")
    }
    pub fn legacy_spec_archive(&self) -> PathBuf {
        self.legacy("planning/archive/specification-pre-modules.md")
    }
    pub fn legacy_project_config(&self) -> PathBuf {
        self.legacy(".planner/config.md")
    }
    pub fn legacy_mcp_config(&self) -> PathBuf {
        self.legacy(".planner/mcp.json")
    }
    pub fn legacy_project_manifest(&self) -> PathBuf {
        self.legacy(".planner/project.json")
    }
    pub fn legacy_workflow(&self) -> PathBuf {
        self.legacy(".planner/workflow.json")
    }
    pub fn legacy_workflow_temporary(&self, run: &str) -> PathBuf {
        self.legacy_config_root()
            .join(format!(".workflow-{run}.tmp"))
    }
    pub fn legacy_adr_root(&self) -> PathBuf {
        self.legacy("adr")
    }

    fn at(&self, relative: &str) -> PathBuf {
        self.root.join(relative)
    }

    fn legacy(&self, relative: &str) -> PathBuf {
        self.legacy_repository
            .as_ref()
            .map(|root| root.join(relative))
            .unwrap_or_else(|| self.root.join(relative))
    }
}

fn safe_component(name: &str) -> bool {
    let mut parts = Path::new(name).components();
    matches!(parts.next(), Some(Component::Normal(_))) && parts.next().is_none()
}

fn safe_path(root: &Path, relative: &str) -> Option<PathBuf> {
    let path = Path::new(relative);
    if path.as_os_str().is_empty()
        || path.is_absolute()
        || !path
            .components()
            .all(|part| matches!(part, Component::Normal(_)))
    {
        return None;
    }
    Some(root.join(path))
}
