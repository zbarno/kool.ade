//! Optimistic synchronization for managed planning Git repositories.
mod git;
mod journal;
mod merge;
mod model;
mod runner;

#[cfg(test)]
mod tests;

pub use model::{PlanningSyncConflict, PlanningSyncMode, PlanningSyncState, PlanningSyncStatus};

pub(crate) use runner::{commit_shared_intent, finish_shared_intent, sync_workspace_at};

pub(crate) use runner::status_at as sync_status_at;
