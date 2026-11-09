use std::{fs, path::Path};

use super::*;

const REV: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const NOW: u64 = 10_000;

pub(super) fn identity() -> crate::computer_use::ComputerIdentityV1 {
    crate::computer_use::ComputerIdentityV1 {
        project_id: uuid::Uuid::new_v4().to_string(),
        task_id: uuid::Uuid::new_v4().to_string(),
        user_id: uuid::Uuid::new_v4().to_string(),
        session_id: uuid::Uuid::new_v4().to_string(),
    }
}

pub(super) fn bridge() -> (tempfile::TempDir, ComputerArtifactBridgeV1) {
    let root = tempfile::tempdir().unwrap();
    let store = crate::artifacts::planning_store::PlanningStore::new(
        uuid::Uuid::new_v4(),
        root.path().join("private-store"),
        crate::artifacts::planning_store::StoreMode::ManagedLocal,
    );
    (root, ComputerArtifactBridgeV1::new(store))
}

pub(super) fn grant_request(
    identity: &crate::computer_use::ComputerIdentityV1,
    flow: ComputerArtifactFlowV1,
    path: &str,
    user_approved: bool,
) -> ComputerTransferGrantRequestV1 {
    ComputerTransferGrantRequestV1 {
        identity: identity.clone(),
        flow,
        task_revision: REV.into(),
        relative_path: path.into(),
        max_bytes: 1024,
        now_unix_ms: NOW,
        expires_at_unix_ms: NOW + 60_000,
        lease_generation: 3,
        approval_generation: 2,
        user_approved,
    }
}

/// Crosses the app-local grant, staging, durable manifest, and task-apply read
/// boundaries; a validator unit test cannot prove the bytes and identity stay
/// coupled through persistence. No QEMU, Docker, SSH, or network; low setup cost.
mod clipboard;
mod persistence;
mod staging;
mod validation;
