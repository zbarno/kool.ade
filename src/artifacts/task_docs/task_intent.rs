use crate::{artifacts::planning_store::PlanningRoot, core::workflow::TaskBatch};

/// Resolve the root request from the planning work that supplied task routing.
/// This keeps intent ownership distinct from batch, nesting, and dependency IDs.
pub(crate) fn uid_for_batch<R: PlanningRoot + ?Sized>(
    repo: &R,
    batch: &TaskBatch,
) -> anyhow::Result<Option<String>> {
    let Some(source_uid) = batch.task_routing.source_work_uid.as_deref() else {
        return Ok(None);
    };
    let work = crate::core::planning_work::load(repo)?
        .into_iter()
        .find(|work| work.uid == source_uid);
    Ok(work.and_then(|work| work.task_intent_root_uid().map(str::to_owned)))
}

pub(crate) fn cycle_uid_for_batch<R: PlanningRoot + ?Sized>(
    repo: &R,
    batch: &TaskBatch,
) -> anyhow::Result<Option<String>> {
    let Some(source_uid) = batch.task_routing.source_work_uid.as_deref() else {
        return Ok(None);
    };
    let work = crate::core::planning_work::load(repo)?
        .into_iter()
        .find(|work| work.uid == source_uid);
    Ok(work.and_then(|work| work.task_intent_cycle_uid))
}

/// Read the explicit TaskIntent owner written into current task metadata.
pub fn uid_for_document(
    document: &super::TaskDocument,
    _documents: &[super::TaskDocument],
    _work: &[crate::core::planning_work::Work],
) -> Option<String> {
    document.metadata.as_ref()?.task_intent_uid.clone()
}

pub fn cycle_uid_for_document(
    document: &super::TaskDocument,
    _documents: &[super::TaskDocument],
    _work: &[crate::core::planning_work::Work],
) -> Option<String> {
    document.metadata.as_ref()?.task_intent_cycle_uid.clone()
}
