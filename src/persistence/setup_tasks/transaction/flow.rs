//! Load and update the persisted "Prepare this computer" flow position.
//!
//! All mutations reuse the store's file-lock transaction boundary and never
//! advance the scan reservation generation. Inserting is idempotently gated:
//! a second insert into an occupied slot is a no-op, so racing startups
//! cannot fork the flow.

use anyhow::Result;

use super::update_at;
use crate::persistence::setup_tasks::SetupFlowStateV1;

fn load_at(path: &std::path::Path) -> Result<Option<SetupFlowStateV1>> {
    update_at(path, |store| Ok((store.setup_flow.clone(), false)))
}

fn insert_at_impl(path: &std::path::Path, state: SetupFlowStateV1) -> Result<bool> {
    update_at(path, |store| {
        if store.setup_flow.is_some() {
            return Ok((false, false));
        }
        let mut state = state;
        state.validate_identity()?;
        state.trim_records();
        store.setup_flow = Some(state);
        Ok((true, true))
    })
}

fn update_flow_impl(
    path: &std::path::Path,
    update: impl FnOnce(&mut SetupFlowStateV1) -> bool,
) -> Result<Option<SetupFlowStateV1>> {
    update_at(path, |store| {
        let changed = store.setup_flow.as_mut().map(update).unwrap_or(false);
        if changed {
            // Storage hygiene: bounds are re-established at the write edge so
            // no client can persist an untrimmed record list.
            store
                .setup_flow
                .as_mut()
                .expect("updated flow is present")
                .trim_records();
        }
        Ok((store.setup_flow.clone(), changed))
    })
}

/// Replace the recorded flow, but only when absent or finished. A live
/// (running/paused/waiting) flow always wins, which keeps racing startups
/// from forking an in-progress preparation.
fn insert_or_replace_terminal_impl(
    path: &std::path::Path,
    state: SetupFlowStateV1,
) -> Result<bool> {
    update_at(path, |store| {
        let dominated = store
            .setup_flow
            .as_ref()
            .is_some_and(|incumbent| incumbent.phase.is_terminal());
        if store.setup_flow.is_some() && !dominated {
            return Ok((false, false));
        }
        let mut state = state;
        state.validate_identity()?;
        state.trim_records();
        store.setup_flow = Some(state);
        Ok((true, true))
    })
}

/// Current flow position, or `None` before the operator starts the flow.
pub fn load_flow() -> Result<Option<SetupFlowStateV1>> {
    load_at(&super::path()?)
}

/// Record a freshly-started flow. Occupied stores win; the starter's state
/// is then reconciled by the caller against the incumbent.
pub fn insert_flow(state: SetupFlowStateV1) -> Result<bool> {
    insert_at_impl(&super::path()?, state)
}

/// Apply a mutation to the existing flow. Stores without a flow read back
/// `None` and the update is not invoked. Returning `true` persists.
pub fn update_flow(
    update: impl FnOnce(&mut SetupFlowStateV1) -> bool,
) -> Result<Option<SetupFlowStateV1>> {
    update_flow_impl(&super::path()?, update)
}

/// Start a fresh flow when none exists; an unfinished incumbent always
/// wins, a finished one is replaced so an explicit restart is a clean slate.
pub fn insert_or_replace_terminal_flow(state: SetupFlowStateV1) -> Result<bool> {
    insert_or_replace_terminal_impl(&super::path()?, state)
}

#[cfg(test)]
pub(super) fn insert_flow_at(path: &std::path::Path, state: SetupFlowStateV1) -> Result<bool> {
    insert_at_impl(path, state)
}

#[cfg(test)]
pub(super) fn update_flow_at(
    path: &std::path::Path,
    update: impl FnOnce(&mut SetupFlowStateV1) -> bool,
) -> Result<Option<SetupFlowStateV1>> {
    update_flow_impl(path, update)
}
