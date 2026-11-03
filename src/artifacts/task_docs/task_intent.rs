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

/// Resolve newly stored or legacy task-document ownership through explicit
/// root identity, routing source identity, or a validated parent-task chain.
pub fn uid_for_document(
    document: &super::TaskDocument,
    documents: &[super::TaskDocument],
    work: &[crate::core::planning_work::Work],
) -> Option<String> {
    let mut current = document;
    let mut visited = std::collections::BTreeSet::new();
    for _ in 0..=documents.len() {
        let metadata = current.metadata.as_ref()?;
        if !visited.insert(metadata.uid.as_str()) {
            return None;
        }
        if let Some(uid) = metadata.task_intent_uid.as_deref() {
            return Some(uid.to_owned());
        }
        if let Some(uid) = metadata
            .routing_source_uid
            .as_deref()
            .and_then(|source| work.iter().find(|candidate| candidate.uid == source))
            .and_then(crate::core::planning_work::Work::task_intent_root_uid)
        {
            return Some(uid.to_owned());
        }
        let parent_uid = metadata.parent_task_uid.as_deref()?;
        current = documents.iter().find(|candidate| {
            candidate
                .metadata
                .as_ref()
                .is_some_and(|metadata| metadata.uid == parent_uid)
        })?;
    }
    None
}

pub fn cycle_uid_for_document(
    document: &super::TaskDocument,
    documents: &[super::TaskDocument],
    work: &[crate::core::planning_work::Work],
) -> Option<String> {
    let mut current = document;
    let mut visited = std::collections::BTreeSet::new();
    for _ in 0..=documents.len() {
        let metadata = current.metadata.as_ref()?;
        if !visited.insert(metadata.uid.as_str()) {
            return None;
        }
        if let Some(uid) = metadata.task_intent_cycle_uid.as_deref() {
            return Some(uid.to_owned());
        }
        if let Some(uid) = metadata
            .routing_source_uid
            .as_deref()
            .and_then(|source_uid| work.iter().find(|candidate| candidate.uid == source_uid))
            .and_then(|source| {
                let root_uid = source.task_intent_root_uid()?;
                let root = work.iter().find(|candidate| candidate.uid == root_uid)?;
                if source.uid == root.uid && root.task_intent_cycles.len() > 1 {
                    None
                } else {
                    source.task_intent_cycle_uid.as_deref()
                }
            })
        {
            return Some(uid.to_owned());
        }
        let parent_uid = metadata.parent_task_uid.as_deref()?;
        current = documents.iter().find(|candidate| {
            candidate
                .metadata
                .as_ref()
                .is_some_and(|metadata| metadata.uid == parent_uid)
        })?;
    }
    None
}
