use serde::Serialize;

use crate::artifacts::planning_store::{PlanningStore, RecordRevisionCheck, StoreError};

use super::{LifecycleEventRecord, Snapshot, encode, queue::queued_record};

pub(in crate::core::lifecycle::store) struct AdditionalTransition {
    pub changes: Vec<(String, Vec<u8>)>,
    pub checks: Vec<RecordRevisionCheck>,
}

pub(in crate::core::lifecycle::store) fn append_transition_with_additional_checks<T: Serialize>(
    store: &PlanningStore,
    path: &str,
    snapshot: &Snapshot<T>,
    expected_revision: u64,
    event_path: String,
    event: LifecycleEventRecord,
    additional: AdditionalTransition,
) -> Result<(), StoreError> {
    let (queue_path, queue_bytes, queue_check) = queued_record(&event)?;
    let mut changes = vec![
        (path.to_owned(), encode(snapshot)?),
        (event_path.clone(), encode(&event)?),
        (queue_path, queue_bytes),
    ];
    let mut checks = vec![
        RecordRevisionCheck {
            path: path.to_owned(),
            expected_revision,
        },
        RecordRevisionCheck {
            path: event_path,
            expected_revision: 0,
        },
        queue_check,
    ];
    for (path, contents) in additional.changes {
        checks.push(RecordRevisionCheck {
            path: path.clone(),
            expected_revision: 0,
        });
        changes.push((path, contents));
    }
    checks.extend(additional.checks);
    store
        .transaction_with_record_revisions(&changes, &checks)
        .map(|_| ())
}
