//! Explicitly consented feature-document publication.

#[path = "publication/model.rs"]
mod model;
#[path = "publication/navigation.rs"]
mod navigation;
#[path = "publication/publish.rs"]
mod publish;
#[path = "publication/simulate.rs"]
mod simulate;
#[path = "publication/stage.rs"]
mod stage;
#[path = "publication/validation.rs"]
mod validation;

pub(super) use model::{
    FeaturePublicationReceipt, PendingFeaturePublication, document_path, paths,
};
pub(super) use navigation::{add_navigation_entry, add_navigation_link};
pub(crate) use publish::publish;
#[cfg(test)]
pub(super) use publish::publish_document_set_with_test_injection;
#[cfg(test)]
pub(crate) use simulate::simulate_merged_feature;
pub(crate) use stage::stage;
pub(crate) use validation::pin_is_published;
pub(super) use validation::{
    reviewed_snapshot, sha256, validate_identity, validate_pending_changes,
};

#[cfg(test)]
pub(crate) fn has_pending(
    store: &crate::artifacts::planning_store::PlanningStore,
    feature_uid: &str,
    snapshot_hash: &str,
) -> bool {
    let (_, pending_path) = paths(feature_uid, snapshot_hash);
    store.read(&pending_path).is_ok()
}

#[cfg(test)]
#[path = "publication_tests.rs"]
mod tests;
