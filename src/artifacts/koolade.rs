//! Koolade-owned working artifacts and ADR publication.
//!
//! Operational artifacts live in the installation-local store. A verified
//! implementation can be summarized into an approved local decision draft;
//! publication uses the primary code repository's reviewed documentation PR.

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
