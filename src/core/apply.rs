//! Mutation phase: validate a turn and atomically write private operational
//! records. Published-document changes remain private drafts for review.
mod comparison;
mod identities;
mod lifecycle_bridge;
mod product_documents;
mod store_transaction;
mod summary;

pub(crate) use product_documents::load_current;
pub use product_documents::{LocalSpecDraftChange, LocalSpecDraftV1};

use crate::artifacts::{items_io, spec_doc};
use crate::core::state::PlannerState;
use crate::core::validation::NormalizedTurn;

pub(crate) fn private_document_draft_changes(
    store: &crate::artifacts::planning_store::PlanningStore,
    updates: &[(String, String)],
) -> anyhow::Result<Vec<(String, Vec<u8>)>> {
    product_documents::changes(store, updates).map(|changes| {
        changes
            .into_iter()
            .map(|(path, content)| (path, content.into_bytes()))
            .collect()
    })
}

#[derive(Debug, Clone)]
pub struct ApplyReceipt {
    pub draft_written: bool,
    pub items_written: bool,
    /// Human-readable summary of the local save.
    pub summary: String,
    /// Store-relative paths changed by this save.
    pub changed_store_paths: Vec<String>,
}

pub fn apply(state: &mut PlannerState, nt: &NormalizedTurn) -> anyhow::Result<ApplyReceipt> {
    if let Some(comparison) = &nt.plan_comparison {
        return comparison::apply(state, comparison, &nt.assistant_message);
    }
    if let Some(batch) = &nt.task_batch {
        anyhow::ensure!(
            nt.additional_planning_artifacts.is_empty(),
            "Task generation cannot write supplemental planning artifacts"
        );
        anyhow::ensure!(
            spec_doc::load(&state.planning_store)? == state.spec_text,
            "The specification changed during task generation; review it again before retrying"
        );
        anyhow::ensure!(
            crate::artifacts::product_docs::active_feature_for_workflow(
                &state.planning_store,
                &state.workflow,
            ) == state.active_feature,
            "The active feature changed during task generation; review it again before retrying"
        );
        anyhow::ensure!(
            crate::artifacts::task_docs::load_workflow(&state.planning_store)? == state.workflow,
            "The interview changed during task generation; review it again"
        );
        let mut workflow = nt
            .workflow
            .clone()
            .unwrap_or_else(|| state.workflow.clone());
        let readiness_checks = lifecycle_bridge::readiness::readiness_record_check(state)?
            .into_iter()
            .collect::<Vec<_>>();
        let (paths, revision) =
            crate::artifacts::task_docs::write_batch_expected_with_revision_and_checks(
                &state.planning_store,
                batch,
                &mut workflow,
                &state.baseline_planning_revision,
                &readiness_checks,
            )?;
        state.workflow = workflow;
        state.baseline_planning_revision = revision;
        lifecycle_bridge::record_task_batch(state, batch)?;
        return Ok(ApplyReceipt {
            draft_written: false,
            items_written: false,
            summary: format!(
                "planner: generate task stories for {}",
                batch.brief.feature_name
            ),
            changed_store_paths: paths,
        });
    }
    // 1) Resolutions remove items from the active queue; private history remains in local application storage.
    if !nt.resolved.is_empty() {
        for item in state
            .items
            .iter()
            .filter(|item| nt.resolved.contains(&item.id))
        {
            let mut resolved = item.clone();
            resolved.status = crate::domain::ItemStatus::Resolved;
            resolved
                .evidence
                .push_str(&format!("\n\nResolution outcome: {}", nt.assistant_message));
            state.resolved_items.retain(|old| old.id != resolved.id);
            state.resolved_items.push(resolved);
        }
        state
            .items
            .retain(|i| !nt.resolved.iter().any(|r| r == &i.id));
    }
    // 2) Field patches on surviving items.
    for (id, patch) in &nt.updates {
        if let Some(it) = state.items.iter_mut().find(|i| &i.id == id) {
            if let Some(p) = patch.priority {
                it.priority = p;
            }
            if let Some(authority) = patch.authority {
                it.authority = authority;
            }
            if let Some(k) = patch.kind {
                it.kind = k;
            }
            if let Some(c) = &patch.category {
                it.category = c.clone();
            }
            if let Some(q) = &patch.question {
                it.question = q.clone();
            }
            if let Some(r) = &patch.reason {
                it.reason = r.clone();
            }
            if let Some(id) = &patch.feature_id {
                it.feature_id = Some(id.clone());
            }
            if let Some(value) = &patch.recommendation {
                it.recommendation = value.clone();
            }
            if let Some(value) = &patch.evidence {
                it.evidence = value.clone();
            }
            if let Some(brief) = &patch.decision_brief {
                it.decision_brief = Some(brief.clone());
            }
            if let Some(blocked_by) = &patch.blocked_by {
                it.blocked_by = blocked_by.clone();
            }
        }
    }
    // 3) Agent-added items (validated + numbered already). The request root is
    // resolved by application context after validation, never model-selected.
    let mut added = nt.added.clone();
    if let Some(context) = &nt.task_intent_context {
        for item in &mut added {
            anyhow::ensure!(
                item.task_intent_uid
                    .as_deref()
                    .is_none_or(|uid| uid == context.root_uid),
                "A planning decision cannot be linked to a different TaskIntent"
            );
            item.task_intent_uid = Some(context.root_uid.clone());
            item.task_intent_cycle_uid = context.cycle_uid.clone();
        }
    }
    state.items.extend(added);

    let mut document_updates = nt.document_updates.clone();
    let feature_uids = identities::preserve_feature_updates(
        &state.planning_store,
        &mut document_updates,
        &nt.change_status_updates,
    )?;
    identities::stabilize_open_item_identities(state, &feature_uids)?;

    // 5) Canonical queue order + serialize.
    items_io::sort_queue(&mut state.items);

    // 6) Stage every changed artifact before writing any of them. The journal
    // restores the old complete set on an interrupted or failed apply.
    let mut changes = Vec::new();
    let mut record_checks = Vec::new();
    changes.extend(product_documents::changes(
        &state.planning_store,
        &document_updates,
    )?);
    let (item_changes, item_checks) =
        items_io::record_changes(&state.planning_store, &state.items, &state.resolved_items)?;
    changes.extend(item_changes.into_iter().map(|(path, bytes)| {
        (
            path,
            String::from_utf8(bytes).expect("item records are UTF-8"),
        )
    }));
    record_checks.extend(item_checks);
    if let Some(workflow) = &nt.workflow {
        let (workflow_changes, workflow_checks) =
            crate::artifacts::task_docs::workflow_record_changes(&state.planning_store, workflow)?;
        changes.extend(workflow_changes.into_iter().map(|(path, bytes)| {
            (
                path,
                String::from_utf8(bytes).expect("workflow records are UTF-8 JSON"),
            )
        }));
        record_checks.extend(workflow_checks);
    }
    changes.extend(nt.additional_planning_artifacts.iter().cloned());
    let discovered = crate::core::planning_work::append_discovered(
        &nt.planning_tasks,
        nt.task_intent_context.as_ref(),
    )?;
    if !discovered.is_empty() {
        let (work_changes, checks) =
            crate::core::planning_work::record_changes(&state.planning_store, &discovered)?;
        changes.extend(work_changes.into_iter().map(|(path, bytes)| {
            (
                path,
                String::from_utf8(bytes).expect("work records are UTF-8 JSON"),
            )
        }));
        record_checks.extend(checks);
    }
    let (changed_store_paths, revision) = store_transaction::apply(
        &state.planning_store,
        &changes,
        &state.baseline_planning_revision,
        &record_checks,
    )?;
    let draft_written = changed_store_paths
        .iter()
        .any(|path| path.starts_with("state/spec_drafts/"));
    let items_root = state
        .planning_store
        .relative_path(crate::artifacts::planning_store::paths::ITEM_CONTENTS);
    let item_state_root = state
        .planning_store
        .relative_path(crate::artifacts::planning_store::paths::ITEM_STATES);
    let items_written = changed_store_paths.iter().any(|path| {
        path.starts_with(&format!("{items_root}/"))
            || path.starts_with(&format!("{item_state_root}/"))
    });
    state.spec_text = spec_doc::load(&state.planning_store)?;
    let refreshed_state = PlannerState::load_with_store(&state.repo_root, &state.planning_store)?;
    state.published_spec_pin = refreshed_state.published_spec_pin;
    state.local_spec_draft = refreshed_state.local_spec_draft;
    state.active_features = refreshed_state.active_features;
    state.active_feature = refreshed_state.active_feature;
    state.baseline_spec = state.spec_text.clone();
    let (persisted_items, persisted_resolved_items, _) =
        items_io::load_store(&state.planning_store)?;
    state.items = persisted_items;
    state.resolved_items = persisted_resolved_items;
    state.baseline_items_md = items_io::serialize(&state.items);
    state.baseline_planning_revision = revision;
    if let Some(workflow) = &nt.workflow {
        state.workflow = crate::artifacts::task_docs::load_workflow(&state.planning_store)?;
        debug_assert_eq!(&state.workflow, workflow);
    }
    let summary = summary::compose(
        nt,
        draft_written,
        nt.resolved.len(),
        nt.added.len(),
        nt.updates.len(),
    );
    Ok(ApplyReceipt {
        draft_written,
        items_written,
        summary,
        changed_store_paths,
    })
}

#[cfg(test)]
mod tests;
