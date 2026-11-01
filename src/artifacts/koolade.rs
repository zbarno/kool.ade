//! Koolade-owned working artifacts and ADR publication.
//!
//! Planning artifacts live in a standalone planning repository, separate
//! from code and product documentation. A verified implementation is
//! summarized into an approved decision record under `planning/decisions/`.

mod decision;
mod identity;
mod plan_choice;

pub(crate) use decision::prepare_decision_record;
pub(crate) use plan_choice::prepare_plan_choice_record;

use std::path::{Path, PathBuf};

use crate::artifacts::layout::{ArtifactLayout, store};
use crate::artifacts::planning_store::PlanningRoot;

pub const KOOLADE_PLANNING_DIR: &str = store::PLANNING;
pub const KOOLADE_IMPLEMENTATION_DIR: &str = store::IMPLEMENTATION;
pub const KOOLADE_TASKS_DIR: &str = store::TASKS;
pub const ADR_DIR: &str = store::DECISIONS;

pub fn task_dir<R: PlanningRoot + ?Sized>(_repo: &R) -> String {
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
