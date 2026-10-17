use super::*;
use crate::core::lifecycle::{
    SpecDeltaClassificationV1, SpecDeltaReferenceV1, SpecOriginOutcomeV1,
};

// Production boundary: file-backed LifecycleStore append, queue acknowledgement
// and exact-history reload. Unit validation cannot prove atomic persistence,
// retry identity or restart behavior. Only temp Git/filesystem fixtures are
// needed; no Docker, network or credentials, with moderate setup cost.
fn history_path(feature_uid: &str, sequence: u64) -> String {
    format!(
        "state/spec_revision_history/{}.json",
        validation::revision_uid(feature_uid, sequence).unwrap()
    )
}

#[path = "origins/batch.rs"]
mod batch;
#[path = "origins/deltas.rs"]
mod deltas;
#[path = "origins/validation.rs"]
mod record_validation;
