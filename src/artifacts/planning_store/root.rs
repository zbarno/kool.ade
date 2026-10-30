use std::path::{Path, PathBuf};

use super::{PlanningLayout, PlanningStore, StoreError, StoreMode};

/// Surface shared by types that expose the project's planning store.
///
/// Planning normally lives in a separate planning repository. Embedded legacy
/// packet sources expose the owning checkout through
/// [`PlanningRoot::code_repository_root`].
pub trait PlanningRoot {
    fn planning_layout(&self) -> PlanningLayout;
    fn planning_store(&self) -> PlanningStore;

    /// The code checkout owning this planning store, when planning is
    /// embedded in that checkout rather than a standalone planning repository.
    fn code_repository_root(&self) -> Option<PathBuf> {
        None
    }

    fn read_planning(&self, relative: &str) -> Result<Vec<u8>, StoreError> {
        self.planning_store().read(root_relative(relative))
    }

    fn write_planning(&self, relative: &str, bytes: &[u8]) -> Result<(), StoreError> {
        self.planning_store()
            .atomic_write(root_relative(relative), bytes)
    }

    fn read_planning_path(&self, path: &Path) -> Result<Vec<u8>, StoreError> {
        self.planning_store().read_planning_path(path)
    }

    fn write_planning_path(&self, path: &Path, bytes: &[u8]) -> Result<(), StoreError> {
        self.planning_store().write_planning_path(path, bytes)
    }
}

fn root_relative(relative: &str) -> &str {
    relative
        .strip_prefix(".koolade-packet/")
        .unwrap_or(relative)
}

impl PlanningRoot for PlanningStore {
    fn planning_layout(&self) -> PlanningLayout {
        self.layout()
    }

    fn planning_store(&self) -> PlanningStore {
        self.clone()
    }

    fn code_repository_root(&self) -> Option<PathBuf> {
        (self.mode == StoreMode::LegacyPacketSource).then(|| self.git_root())
    }
}

impl<T: PlanningRoot + ?Sized> PlanningRoot for &T {
    fn planning_layout(&self) -> PlanningLayout {
        (**self).planning_layout()
    }

    fn planning_store(&self) -> PlanningStore {
        (**self).planning_store()
    }

    fn code_repository_root(&self) -> Option<PathBuf> {
        (**self).code_repository_root()
    }
}

/// Test-fixture adapter: treats a path as the checkout of a legacy
/// embedded project (`.koolade-packet/` inside it). Exists so the existing
/// fixture corpus keeps compiling while production code is moved onto
/// explicit stores; never usable from non-test builds.
#[cfg(test)]
impl PlanningRoot for Path {
    fn planning_layout(&self) -> PlanningLayout {
        self.planning_store().layout()
    }

    fn planning_store(&self) -> PlanningStore {
        PlanningStore::legacy_embedded(uuid::Uuid::nil(), self)
    }

    fn code_repository_root(&self) -> Option<PathBuf> {
        Some(self.to_path_buf())
    }
}

#[cfg(test)]
impl PlanningRoot for std::path::PathBuf {
    fn planning_layout(&self) -> PlanningLayout {
        PlanningRoot::planning_layout(self.as_path())
    }

    fn planning_store(&self) -> PlanningStore {
        PlanningRoot::planning_store(self.as_path())
    }

    fn code_repository_root(&self) -> Option<PathBuf> {
        Some(self.clone())
    }
}
