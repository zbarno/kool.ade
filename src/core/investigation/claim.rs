//! Nonblocking cross-window investigation ownership, released by worker exit.
use super::*;
use crate::artifacts::planning_store::PlanningStore;
use std::fs::{File, OpenOptions, TryLockError};

pub(crate) fn try_claim(state: &PlannerState, item_id: &str) -> anyhow::Result<Option<File>> {
    let uid = state
        .items
        .iter()
        .find(|item| item.id == item_id)
        .and_then(|item| item.uid.as_deref())
        .ok_or_else(|| anyhow::anyhow!("Investigation item has no immutable identity"))?;
    acquire(&state.planning_store, uid)
}

fn acquire(store: &PlanningStore, uid: &str) -> anyhow::Result<Option<File>> {
    let uid = uuid::Uuid::parse_str(uid)?.to_string();
    let root = store.root.join("implementation/investigation-locks");
    std::fs::create_dir_all(&root)?;
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(root.join(format!("{uid}.lock")))?;
    match file.try_lock() {
        Ok(()) => Ok(Some(file)),
        Err(TryLockError::WouldBlock) => Ok(None),
        Err(TryLockError::Error(error)) => Err(error.into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    /// Production boundary: independent worker file handles share one OS lock.
    /// In-memory admission cannot prove exclusion or release after worker exit.
    /// Temporary files only; no Docker/provider/network, low setup cost.
    #[test]
    fn investigation_worker_ownership_excludes_peer_and_releases_on_exit() {
        let root = tempfile::tempdir().unwrap();
        let store = PlanningStore::new(
            uuid::Uuid::new_v4(),
            root.path(),
            crate::artifacts::planning_store::StoreMode::ManagedLocal,
        );
        let uid = uuid::Uuid::new_v4().to_string();
        let first = acquire(&store, &uid).unwrap().unwrap();
        let other_store = store.clone();
        let other_uid = uid.clone();
        assert!(
            std::thread::spawn(move || acquire(&other_store, &other_uid).unwrap().is_none())
                .join()
                .unwrap()
        );
        drop(first);
        assert!(acquire(&store, &uid).unwrap().is_some());
    }
}
