use super::{PlanningStore, StoreError, apply_store_inner_with_checks};
use crate::artifacts::planning_store::RecordRevisionCheck;

pub(crate) fn apply_store_with_record_revisions(
    store: &PlanningStore,
    changes: &[(String, Vec<u8>)],
    expected_records: &[RecordRevisionCheck],
) -> Result<(Vec<String>, String), StoreError> {
    apply_store_inner_with_checks(store, changes, &[], None, expected_records, None)
}

pub(crate) fn apply_store_with_revision_and_record_revisions(
    store: &PlanningStore,
    changes: &[(String, Vec<u8>)],
    expected_revision: Option<&str>,
    expected_records: &[RecordRevisionCheck],
) -> Result<(Vec<String>, String), StoreError> {
    apply_store_inner_with_checks(
        store,
        changes,
        &[],
        expected_revision,
        expected_records,
        None,
    )
}

pub(crate) fn apply_store_with_removals_and_record_revisions(
    store: &PlanningStore,
    changes: &[(String, Vec<u8>)],
    removals: &[String],
    expected_revision: Option<&str>,
    expected_records: &[RecordRevisionCheck],
) -> Result<(Vec<String>, String), StoreError> {
    apply_store_inner_with_checks(
        store,
        changes,
        removals,
        expected_revision,
        expected_records,
        None,
    )
}
