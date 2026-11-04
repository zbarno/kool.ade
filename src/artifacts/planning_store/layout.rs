use std::path::{Component, Path, PathBuf};

use super::PlanningStore;

/// Resolved paths inside one installation-local operational store.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlanningLayout {
    root: PathBuf,
}

pub mod paths {
    pub const MANIFEST: &str = "manifest.json";
    pub const CONFIG: &str = "config";
    pub const PROJECT_CONFIG: &str = "config/project.md";
    pub const PROJECT_MANIFEST: &str = "config/repositories.json";
    pub const HARNESS_ROUTES: &str = "config/harness-routes.json";
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
    pub const ITEM_CONTENTS: &str = "planning/items";
    pub const WORK_RECORDS: &str = "state/work";
    pub const WORKFLOW_RECORDS: &str = "state/workflow";
    pub const ITEM_STATES: &str = "state/items";
    /// Typed lifecycle questions share the normalized planning-item collections.
    pub const QUESTION_CONTENTS: &str = ITEM_CONTENTS;
    pub const QUESTION_STATES: &str = ITEM_STATES;
    pub const READINESS_STATES: &str = "state/readiness";
    pub const TASK_STATES: &str = "state/tasks";
    pub const WORKFLOW: &str = "state/workflow.json";
    pub const WORK: &str = "state/work.json";
    pub const CANCELLED_WORK: &str = "state/cancelled-work.json";
    pub const STATE: &str = "state";
    pub const IMPLEMENTATION: &str = "implementation";
    pub const ARCHIVE: &str = "planning/archive";
}

impl PlanningLayout {
    pub(super) fn for_store(store: &PlanningStore) -> Self {
        Self {
            root: store.root.clone(),
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Resolve a root-relative path beneath this operational store.
    pub fn canonical_path(&self, relative: &str) -> Option<PathBuf> {
        safe_path(&self.root, relative)
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
    #[cfg(test)]
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
    pub fn workflow_record(&self, uid: &str) -> Option<PathBuf> {
        record_path(&self.root, paths::WORKFLOW_RECORDS, uid)
    }
    pub fn work_state(&self) -> PathBuf {
        self.at(paths::WORK)
    }
    pub fn work_record(&self, uid: &str) -> Option<PathBuf> {
        record_path(&self.root, paths::WORK_RECORDS, uid)
    }
    pub fn item_content(&self, uid: &str) -> Option<PathBuf> {
        uuid::Uuid::parse_str(uid)
            .ok()
            .map(|uid| self.at(paths::ITEM_CONTENTS).join(format!("{uid}.md")))
    }
    pub fn item_state(&self, uid: &str) -> Option<PathBuf> {
        record_path(&self.root, paths::ITEM_STATES, uid)
    }
    pub fn question_content(&self, uid: &str) -> Option<PathBuf> {
        uuid::Uuid::parse_str(uid)
            .ok()
            .map(|uid| self.at(paths::QUESTION_CONTENTS).join(format!("{uid}.md")))
    }
    pub fn question_state(&self, uid: &str) -> Option<PathBuf> {
        record_path(&self.root, paths::QUESTION_STATES, uid)
    }
    pub fn readiness_state(&self, uid: &str) -> Option<PathBuf> {
        record_path(&self.root, paths::READINESS_STATES, uid)
    }
    pub fn task_state(&self, uid: &str) -> Option<PathBuf> {
        record_path(&self.root, paths::TASK_STATES, uid)
    }
    pub fn archive_root(&self) -> PathBuf {
        self.at(paths::ARCHIVE)
    }
    pub fn implementation_root(&self) -> PathBuf {
        self.at(paths::IMPLEMENTATION)
    }

    fn at(&self, relative: &str) -> PathBuf {
        self.root.join(relative)
    }
}

fn safe_component(name: &str) -> bool {
    let mut parts = Path::new(name).components();
    matches!(parts.next(), Some(Component::Normal(_))) && parts.next().is_none()
}

fn record_path(root: &Path, collection: &str, uid: &str) -> Option<PathBuf> {
    let uid = uuid::Uuid::parse_str(uid).ok()?.hyphenated().to_string();
    Some(root.join(collection).join(format!("{uid}.json")))
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
