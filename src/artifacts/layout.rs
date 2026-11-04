//! Single source of truth for Kool.ad/e-owned private record paths.
use std::path::{Component, Path, PathBuf};

pub mod store {
    pub const MANIFEST: &str = "manifest.json";
    pub const CONFIG: &str = "config";
    pub const PROJECT_CONFIG: &str = "config/project.md";
    pub const PROJECT_MANIFEST: &str = "config/repositories.json";
    pub const MCP_CONFIG: &str = "config/mcp.json";
    pub const PLANNING: &str = "planning";
    pub const PRODUCT: &str = "planning/product";
    pub const PRODUCT_INDEX: &str = "planning/product/index.md";
    #[cfg(test)]
    pub const PRODUCT_MANIFEST: &str = "planning/product/manifest.json";
    pub const CHANGES: &str = "planning/changes";
    pub const DECISIONS: &str = "planning/decisions";
    pub const OPEN_ITEMS: &str = "planning/open-items.md";
    pub const RESOLVED_ITEMS: &str = "planning/resolved-items.json";
    pub const IMPORTS: &str = "planning/imports";
    pub const TASKS: &str = "planning/tasks";
    pub const WORKFLOW: &str = "state/workflow.json";
    pub const WORK: &str = "state/work.json";
    pub const STATE: &str = "state";
    pub const LEDGER: &str = "state/time-ledger.log";
    pub const IMPLEMENTATION: &str = "implementation";
    pub const ARCHIVE: &str = "planning/archive";
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArtifactLayout {
    repository: PathBuf,
}

impl ArtifactLayout {
    pub fn new(repository: impl Into<PathBuf>) -> Self {
        Self {
            repository: repository.into(),
        }
    }

    pub fn repository(&self) -> &Path {
        &self.repository
    }

    pub fn canonical_path(&self, relative: &str) -> Option<PathBuf> {
        safe_repository_path(&self.repository, relative)
    }

    /// Resolve a root-relative private record path.
    pub fn resolve_planning_root_relative(root: &Path, relative: &str) -> Option<PathBuf> {
        safe_repository_path(root, relative)
    }

    pub fn koolade_root(&self) -> PathBuf {
        self.repository.clone()
    }
    pub fn manifest(&self) -> PathBuf {
        self.at(store::MANIFEST)
    }
    pub fn config_root(&self) -> PathBuf {
        self.at(store::CONFIG)
    }
    pub fn project_config(&self) -> PathBuf {
        self.at(store::PROJECT_CONFIG)
    }
    pub fn project_manifest(&self) -> PathBuf {
        self.at(store::PROJECT_MANIFEST)
    }
    pub fn mcp_config(&self) -> PathBuf {
        self.at(store::MCP_CONFIG)
    }
    pub fn planning_root(&self) -> PathBuf {
        self.at(store::PLANNING)
    }
    pub fn product_root(&self) -> PathBuf {
        self.at(store::PRODUCT)
    }
    pub fn product_index(&self) -> PathBuf {
        self.at(store::PRODUCT_INDEX)
    }
    #[cfg(test)]
    pub fn product_manifest(&self) -> PathBuf {
        self.at(store::PRODUCT_MANIFEST)
    }
    pub fn product_module(&self, name: &str) -> Option<PathBuf> {
        safe_component(name).then(|| self.product_root().join(name))
    }
    pub fn changes_root(&self) -> PathBuf {
        self.at(store::CHANGES)
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
        self.at(store::DECISIONS)
    }
    pub fn decision_record(&self, name: &str) -> Option<PathBuf> {
        safe_component(name).then(|| self.decisions_root().join(name))
    }
    pub fn open_items(&self) -> PathBuf {
        self.at(store::OPEN_ITEMS)
    }
    pub fn resolved_items(&self) -> PathBuf {
        self.at(store::RESOLVED_ITEMS)
    }
    pub fn imports_root(&self) -> PathBuf {
        self.at(store::IMPORTS)
    }
    pub fn tasks_root(&self) -> PathBuf {
        self.at(store::TASKS)
    }
    pub fn task_batch(&self, name: &str) -> Option<PathBuf> {
        safe_component(name).then(|| self.tasks_root().join(name))
    }
    pub fn workflow_state(&self) -> PathBuf {
        self.at(store::WORKFLOW)
    }
    pub fn work_state(&self) -> PathBuf {
        self.at(store::WORK)
    }
    pub fn time_ledger(&self) -> PathBuf {
        self.at(store::LEDGER)
    }
    pub fn implementation_root(&self) -> PathBuf {
        self.at(store::IMPLEMENTATION)
    }
    pub fn archive_root(&self) -> PathBuf {
        self.at(store::ARCHIVE)
    }

    pub fn is_task_ticket_path(relative: &str) -> bool {
        relative.starts_with(&format!("{}/", store::TASKS))
    }

    fn at(&self, relative: &str) -> PathBuf {
        self.repository.join(relative)
    }
}

fn safe_repository_path(root: &Path, relative: &str) -> Option<PathBuf> {
    let path = Path::new(relative);
    if path.as_os_str().is_empty()
        || !path
            .components()
            .all(|part| matches!(part, Component::Normal(_)))
    {
        return None;
    }
    Some(root.join(path))
}

fn safe_component(name: &str) -> bool {
    let mut parts = Path::new(name).components();
    matches!(parts.next(), Some(Component::Normal(_))) && parts.next().is_none()
}

#[cfg(test)]
mod tests;
