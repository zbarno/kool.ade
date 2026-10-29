use super::cache_locks;
use crate::harness::resource_bridge::{cargo, go, npm, pip};
use std::{
    path::Path,
    path::PathBuf,
    sync::{Arc, Mutex},
};

pub(super) struct BrokerCaches {
    pub(super) npm: PathBuf,
    pub(super) npm_snapshot: PathBuf,
    pub(super) npm_gate: Arc<Mutex<()>>,
    pub(super) cargo: PathBuf,
    pub(super) cargo_gate: Arc<Mutex<()>>,
    pub(super) pip: PathBuf,
    pub(super) pip_snapshot: PathBuf,
    pub(super) pip_gate: Arc<Mutex<()>>,
    pub(super) go: PathBuf,
    pub(super) go_snapshot: PathBuf,
    pub(super) go_gate: Arc<Mutex<()>>,
}

impl BrokerCaches {
    pub(super) fn create(temp: &Path, state_root: &Path, worktree: &Path) -> anyhow::Result<Self> {
        let npm = npm::persistent_cache_at(state_root, worktree)?;
        let npm_gate = cache_locks::gate_for(&npm)?;
        let npm_snapshot = temp.join("npm-index-snapshots");
        {
            let _guard = npm_gate
                .lock()
                .map_err(|_| anyhow::anyhow!("npm cache coordination lock is unavailable"))?;
            npm::publish_index_snapshot(&npm, &npm_snapshot)?;
        }

        let cargo = cargo::persistent_cache_at(state_root)?;
        let cargo_gate = cache_locks::gate_for(&cargo)?;
        let pip = pip::persistent_cache_at(state_root)?;
        let pip_gate = cache_locks::gate_for(&pip)?;
        let pip_snapshot = temp.join("pip-wheelhouse-staging");
        std::fs::create_dir(&pip_snapshot)?;
        super::super::set_private_dir(&pip_snapshot)?;
        let go = go::persistent_cache_at(state_root)?;
        let go_gate = cache_locks::gate_for(&go)?;
        let go_snapshot = temp.join("go-module-staging");
        std::fs::create_dir(&go_snapshot)?;
        super::super::set_private_dir(&go_snapshot)?;

        Ok(Self {
            npm,
            npm_snapshot,
            npm_gate,
            cargo,
            cargo_gate,
            pip,
            pip_snapshot,
            pip_gate,
            go,
            go_snapshot,
            go_gate,
        })
    }
}
