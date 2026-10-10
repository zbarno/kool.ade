//! Koolade-owned working artifacts and ADR publication.
//!
//! `.koolade` is intentionally separate from product documentation:
//! it contains resumable planning/task material and implementation evidence.
//! A verified implementation is summarized into a decision record under
//! `.koolade-packet/planning/decisions/` in the implementation repository.

mod decision;
mod identity;
mod plan_choice;

pub(crate) use decision::prepare_decision_record;
pub(crate) use plan_choice::prepare_plan_choice_record;

use std::path::{Path, PathBuf};

use crate::artifacts::layout::{ArtifactLayout, canonical};
use crate::artifacts::planning_store::{PlanningRoot, StoreMode};

pub const KOOLADE_DIR: &str = canonical::ROOT;
pub const KOOLADE_PLANNING_DIR: &str = canonical::PLANNING;
pub const KOOLADE_IMPLEMENTATION_DIR: &str = canonical::IMPLEMENTATION;
pub const KOOLADE_TASKS_DIR: &str = canonical::TASKS;
pub const ADR_DIR: &str = canonical::DECISIONS;

pub fn task_dir<R: PlanningRoot + ?Sized>(repo: &R) -> String {
    let store = repo.planning_store();
    if store.mode == StoreMode::LegacyEmbedded {
        return KOOLADE_TASKS_DIR.to_owned();
    }
    crate::artifacts::planning_store::paths::TASKS.to_owned()
}

pub fn koolade_root(repo: &Path) -> PathBuf {
    ArtifactLayout::new(repo).koolade_root()
}

pub fn planning_root(repo: &Path) -> PathBuf {
    ArtifactLayout::new(repo).planning_root()
}

pub fn implementation_root(repo: &Path) -> PathBuf {
    ArtifactLayout::new(repo).implementation_root()
}
