use crate::{
    artifacts::planning_store::StoreError,
    core::lifecycle::{FeatureGoal, SpecDeltaRecordV1},
};

use super::super::super::LifecycleStore;

pub(super) fn validate_requirement_tests(
    lifecycle: &LifecycleStore<'_>,
    feature: &FeatureGoal,
    delta: &SpecDeltaRecordV1,
) -> Result<(), StoreError> {
    if delta.test_refs.is_empty() {
        return Ok(());
    }
    let readiness = crate::core::planning::QuestionStore::new(lifecycle.store)
        .load_readiness(&feature.feature_uid)?
        .map(|(readiness, _)| readiness)
        .ok_or_else(|| {
            StoreError::MalformedState(
                "requirement test references require a persisted readiness checklist".into(),
            )
        })?;
    if feature
        .plan_snapshot_uid
        .as_deref()
        .is_none_or(|snapshot_uid| {
            crate::core::lifecycle::plan_snapshot::load_persisted(lifecycle.store, snapshot_uid)
                .ok()
                .flatten()
                .is_none_or(|snapshot| {
                    readiness.spec_revision.as_deref() != Some(snapshot.spec_revision.as_str())
                })
        })
        || delta.test_refs.iter().any(|test_ref| {
            !readiness.acceptance_tests.iter().any(|test| {
                test.requirement_id == delta.requirement_id && test.test_ref == *test_ref
            })
        })
    {
        return Err(StoreError::MalformedState(
            "requirement test reference does not resolve to the approved plan readiness".into(),
        ));
    }
    Ok(())
}
