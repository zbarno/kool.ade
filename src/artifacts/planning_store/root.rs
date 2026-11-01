use std::path::Path;

#[cfg(test)]
use super::StoreMode;
use super::{PlanningLayout, PlanningStore, StoreError};

/// Surface shared by types that expose the separate planning repository.
pub trait PlanningRoot {
    fn planning_layout(&self) -> PlanningLayout;
    fn planning_store(&self) -> PlanningStore;

    fn read_planning(&self, relative: &str) -> Result<Vec<u8>, StoreError> {
        self.planning_store().read(relative)
    }

    fn write_planning(&self, relative: &str, bytes: &[u8]) -> Result<(), StoreError> {
        self.planning_store().atomic_write(relative, bytes)
    }

    fn read_planning_path(&self, path: &Path) -> Result<Vec<u8>, StoreError> {
        self.planning_store().read_planning_path(path)
    }

    fn write_planning_path(&self, path: &Path, bytes: &[u8]) -> Result<(), StoreError> {
        self.planning_store().write_planning_path(path, bytes)
    }
}

impl PlanningRoot for PlanningStore {
    fn planning_layout(&self) -> PlanningLayout {
        self.layout()
    }

    fn planning_store(&self) -> PlanningStore {
        self.clone()
    }
}

impl<T: PlanningRoot + ?Sized> PlanningRoot for &T {
    fn planning_layout(&self) -> PlanningLayout {
        (**self).planning_layout()
    }

    fn planning_store(&self) -> PlanningStore {
        (**self).planning_store()
    }
}

/// Test-fixture adapter: treats a path as the planning repository root.
/// Never usable from non-test builds.
#[cfg(test)]
impl PlanningRoot for Path {
    fn planning_layout(&self) -> PlanningLayout {
        self.planning_store().layout()
    }

    fn planning_store(&self) -> PlanningStore {
        PlanningStore::new(uuid::Uuid::nil(), self, StoreMode::ManagedLocal)
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
}
