//! Recoverable planning-store transactions.
//!
//! Writes land in the planning repository's Git metadata as a journaled,
//! multi-file transaction with optimistic revision fencing. See
//! `docs/planning-repository-v1.md` §"Writes are journaled transactions".

mod store;

#[cfg(test)]
pub(crate) use store::apply_store_with_record_revisions_interruption_for_test;
pub(crate) use store::{
    apply_store, apply_store_with_record_revisions, apply_store_with_removals,
    apply_store_with_removals_and_record_revisions, apply_store_with_removals_and_revision,
    apply_store_with_revision, apply_store_with_revision_and_record_revisions, recover_store,
    revision, with_consistent_read,
};
