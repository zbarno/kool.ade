use crate::core::lifecycle::{LifecycleStore, TaskRecord};

pub(super) fn record(lifecycle: &LifecycleStore<'_>, task: &TaskRecord) {
    let store = lifecycle.store;
    crate::artifacts::migration::bootstrap_product_with_store(
        &store.git_root(),
        store,
        "Completion fixture",
    )
    .unwrap();
    let origins = lifecycle
        .list_spec_sync_queue(&task.feature_uid, false)
        .unwrap()
        .into_iter()
        .map(|entry| entry.uid)
        .collect();
    super::super::super::spec_sync::append_current_revision(
        lifecycle,
        store,
        &task.feature_uid,
        0,
        origins,
        task.reviewed_code_head_sha.clone(),
    );
}
