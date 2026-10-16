use crate::artifacts::planning_store::{RecordRevisionCheck, StoreError};
use crate::core::lifecycle::LifecycleEventRecord;

use super::super::spec_sync_queue;
use super::encode;

pub(super) fn queued_record(
    event: &LifecycleEventRecord,
) -> Result<(String, Vec<u8>, RecordRevisionCheck), StoreError> {
    let queue = spec_sync_queue::queued_for_event(event)?;
    let path = spec_sync_queue::queue_path(&queue.uid)?;
    let bytes = encode(&queue)?;
    Ok((
        path.clone(),
        bytes,
        RecordRevisionCheck {
            path,
            expected_revision: 0,
        },
    ))
}
