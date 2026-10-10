use std::path::{Path, PathBuf};

use super::{PlanningLayout, PlanningStore, StoreError, StoreMode};

/// Adapter for callers that still pass a code repository path. Runtime
/// project code passes its `PlanningStore` explicitly.
pub trait PlanningRoot {
    fn planning_layout(&self) -> PlanningLayout;
    fn planning_store(&self) -> PlanningStore;
    fn code_repository_root(&self) -> Option<PathBuf>;

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

impl PlanningRoot for Path {
    fn planning_layout(&self) -> PlanningLayout {
        PlanningLayout::legacy_repository(self)
    }

    fn planning_store(&self) -> PlanningStore {
        PlanningStore::legacy_embedded(uuid::Uuid::nil(), self)
    }

    fn code_repository_root(&self) -> Option<PathBuf> {
        Some(self.to_path_buf())
    }
}

impl PlanningRoot for PathBuf {
    fn planning_layout(&self) -> PlanningLayout {
        self.as_path().planning_layout()
    }

    fn planning_store(&self) -> PlanningStore {
        self.as_path().planning_store()
    }

    fn code_repository_root(&self) -> Option<PathBuf> {
        Some(self.clone())
    }
}

impl PlanningRoot for PlanningStore {
    fn planning_layout(&self) -> PlanningLayout {
        self.layout()
    }

    fn planning_store(&self) -> PlanningStore {
        self.clone()
    }

    fn code_repository_root(&self) -> Option<PathBuf> {
        (self.mode == StoreMode::LegacyEmbedded)
            .then(|| self.root.parent().map(Path::to_path_buf))
            .flatten()
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
