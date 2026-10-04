//! Single source of truth for Koolade-owned paths in a project repository.
//!
//! `canonical` is the live layout. `legacy` names migration inputs and paths
//! that may need rollback before a pre-migration journal is recovered.
use std::path::{Component, Path, PathBuf};

pub mod canonical {
    pub const ROOT: &str = ".koolade-packet";
    pub const MANIFEST: &str = ".koolade-packet/manifest.json";
    pub const CONFIG: &str = ".koolade-packet/config";
    pub const PROJECT_CONFIG: &str = ".koolade-packet/config/project.md";
    pub const PROJECT_MANIFEST: &str = ".koolade-packet/config/repositories.json";
    pub const MCP_CONFIG: &str = ".koolade-packet/config/mcp.json";
    pub const PLANNING: &str = ".koolade-packet/planning";
    pub const PRODUCT: &str = ".koolade-packet/planning/product";
    pub const PRODUCT_INDEX: &str = ".koolade-packet/planning/product/index.md";
    pub const PRODUCT_MANIFEST: &str = ".koolade-packet/planning/product/manifest.json";
    pub const CHANGES: &str = ".koolade-packet/planning/changes";
    pub const DECISIONS: &str = ".koolade-packet/planning/decisions";
    pub const OPEN_ITEMS: &str = ".koolade-packet/planning/open-items.md";
    pub const RESOLVED_ITEMS: &str = ".koolade-packet/planning/resolved-items.json";
    pub const IMPORTS: &str = ".koolade-packet/planning/imports";
    pub const TASKS: &str = ".koolade-packet/planning/tasks";
    pub const WORKFLOW: &str = ".koolade-packet/state/workflow.json";
    pub const WORK: &str = ".koolade-packet/state/work.json";
    pub const STATE: &str = ".koolade-packet/state";
    pub const LEDGER: &str = ".koolade-packet/state/time-ledger.log";
    pub const IMPLEMENTATION: &str = ".koolade-packet/implementation";
    pub const ARCHIVE: &str = ".koolade-packet/planning/archive";
    pub const LEGACY_SPEC_ARCHIVE: &str =
        ".koolade-packet/planning/archive/specification-pre-modules.md";
}

pub mod legacy {
    pub const PLANNING: &str = "planning";
    pub const SPECIFICATION: &str = "planning/specification.md";
    pub const PRODUCT: &str = "planning/product";
    pub const PRODUCT_INDEX: &str = "planning/product/index.md";
    pub const FEATURES: &str = "planning/features";
    pub const OPEN_ITEMS: &str = "planning/open-items.md";
    pub const RESOLVED_ITEMS: &str = "planning/resolved-items.json";
    pub const IMPORTS: &str = "planning/imports";
    pub const TASKS: &str = "planning/tasks";
    pub const ARCHIVE: &str = "planning/archive";
    pub const SPEC_ARCHIVE: &str = "planning/archive/specification-pre-modules.md";
    pub const CONFIG: &str = ".planner";
    pub const PROJECT_CONFIG: &str = ".planner/config.md";
    pub const MCP_CONFIG: &str = ".planner/mcp.json";
    pub const PROJECT_MANIFEST: &str = ".planner/project.json";
    pub const WORKFLOW: &str = ".planner/workflow.json";
    pub const ADR: &str = "adr";
    pub const ROOT_SPECIFICATION: &str = "SPECIFICATION.md";
}

/// Transitional paths emitted by earlier canonical-layout versions.
pub mod previous {
    pub const ROOT: &str = ".koolade";
    pub const WORK: &str = ".koolade/planning/work.json";
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
        safe_repository_path(&self.repository.join(canonical::ROOT), relative)
    }

    pub fn legacy_path(&self, relative: &str) -> Option<PathBuf> {
        safe_repository_path(&self.repository, relative)
    }

    pub fn koolade_root(&self) -> PathBuf {
        self.at(canonical::ROOT)
    }
    pub fn manifest(&self) -> PathBuf {
        self.at(canonical::MANIFEST)
    }
    pub fn config_root(&self) -> PathBuf {
        self.at(canonical::CONFIG)
    }
    pub fn project_config(&self) -> PathBuf {
        self.at(canonical::PROJECT_CONFIG)
    }
    pub fn project_manifest(&self) -> PathBuf {
        self.at(canonical::PROJECT_MANIFEST)
    }
    pub fn mcp_config(&self) -> PathBuf {
        self.at(canonical::MCP_CONFIG)
    }
    pub fn planning_root(&self) -> PathBuf {
        self.at(canonical::PLANNING)
    }
    pub fn product_root(&self) -> PathBuf {
        self.at(canonical::PRODUCT)
    }
    pub fn product_index(&self) -> PathBuf {
        self.at(canonical::PRODUCT_INDEX)
    }
    pub fn product_manifest(&self) -> PathBuf {
        self.at(canonical::PRODUCT_MANIFEST)
    }
    pub fn product_module(&self, name: &str) -> Option<PathBuf> {
        safe_component(name).then(|| self.product_root().join(name))
    }
    pub fn changes_root(&self) -> PathBuf {
        self.at(canonical::CHANGES)
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
        self.at(canonical::DECISIONS)
    }
    pub fn decision_record(&self, name: &str) -> Option<PathBuf> {
        safe_component(name).then(|| self.decisions_root().join(name))
    }
    pub fn open_items(&self) -> PathBuf {
        self.at(canonical::OPEN_ITEMS)
    }
    pub fn resolved_items(&self) -> PathBuf {
        self.at(canonical::RESOLVED_ITEMS)
    }
    pub fn imports_root(&self) -> PathBuf {
        self.at(canonical::IMPORTS)
    }
    pub fn tasks_root(&self) -> PathBuf {
        self.at(canonical::TASKS)
    }
    pub fn task_batch(&self, name: &str) -> Option<PathBuf> {
        safe_component(name).then(|| self.tasks_root().join(name))
    }
    pub fn workflow_state(&self) -> PathBuf {
        self.at(canonical::WORKFLOW)
    }
    pub fn work_state(&self) -> PathBuf {
        self.at(canonical::WORK)
    }
    pub fn time_ledger(&self) -> PathBuf {
        self.at(canonical::LEDGER)
    }
    pub fn implementation_root(&self) -> PathBuf {
        self.at(canonical::IMPLEMENTATION)
    }
    pub fn archive_root(&self) -> PathBuf {
        self.at(canonical::ARCHIVE)
    }

    pub fn legacy_specification(&self) -> PathBuf {
        self.at(legacy::SPECIFICATION)
    }
    pub fn legacy_planning_root(&self) -> PathBuf {
        self.at(legacy::PLANNING)
    }
    pub fn legacy_product_root(&self) -> PathBuf {
        self.at(legacy::PRODUCT)
    }
    pub fn legacy_product_index(&self) -> PathBuf {
        self.at(legacy::PRODUCT_INDEX)
    }
    pub fn legacy_features_root(&self) -> PathBuf {
        self.at(legacy::FEATURES)
    }
    pub fn legacy_feature_directory(&self, name: &str) -> Option<PathBuf> {
        safe_component(name).then(|| self.legacy_features_root().join(name))
    }
    pub fn legacy_feature_specification(&self, name: &str) -> Option<PathBuf> {
        self.legacy_feature_directory(name)
            .map(|dir| dir.join("specification.md"))
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
        self.at(legacy::OPEN_ITEMS)
    }
    pub fn legacy_resolved_items(&self) -> PathBuf {
        self.at(legacy::RESOLVED_ITEMS)
    }
    pub fn legacy_imports_root(&self) -> PathBuf {
        self.at(legacy::IMPORTS)
    }
    pub fn legacy_import_path(&self, name: &str) -> Option<PathBuf> {
        safe_component(name).then(|| self.legacy_imports_root().join(name))
    }
    pub fn legacy_import_relative(&self, name: &str) -> Option<String> {
        self.legacy_import_path(name)?
            .strip_prefix(&self.repository)
            .ok()
            .map(|path| path.to_string_lossy().into_owned())
    }
    pub fn legacy_tasks_root(&self) -> PathBuf {
        self.at(legacy::TASKS)
    }
    pub fn legacy_config_root(&self) -> PathBuf {
        self.at(legacy::CONFIG)
    }
    pub fn legacy_archive_root(&self) -> PathBuf {
        self.at(legacy::ARCHIVE)
    }
    pub fn legacy_spec_archive(&self) -> PathBuf {
        self.at(legacy::SPEC_ARCHIVE)
    }
    pub fn canonical_spec_archive(&self) -> PathBuf {
        self.at(canonical::LEGACY_SPEC_ARCHIVE)
    }
    pub fn legacy_project_config(&self) -> PathBuf {
        self.at(legacy::PROJECT_CONFIG)
    }
    pub fn legacy_mcp_config(&self) -> PathBuf {
        self.at(legacy::MCP_CONFIG)
    }
    pub fn legacy_project_manifest(&self) -> PathBuf {
        self.at(legacy::PROJECT_MANIFEST)
    }
    pub fn legacy_workflow(&self) -> PathBuf {
        self.at(legacy::WORKFLOW)
    }
    pub fn legacy_workflow_temporary(&self, run: &str) -> PathBuf {
        self.legacy_config_root()
            .join(format!(".workflow-{run}.tmp"))
    }
    pub fn legacy_adr_root(&self) -> PathBuf {
        self.at(legacy::ADR)
    }

    pub fn is_task_ticket_path(relative: &str) -> bool {
        relative.starts_with(&format!("{}/", canonical::TASKS))
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
