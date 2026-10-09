use crate::harness::DependencyRequest;
use std::{
    path::Path,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize},
    },
};

pub(super) struct BrokerContext<'a> {
    pub(super) worktree: &'a Path,
    pub(super) private_configuration: bool,
    pub(super) resource_dir: &'a Path,
    pub(super) npm_cache: &'a Path,
    pub(super) npm_snapshot: &'a Path,
    pub(super) cargo_cache: &'a Path,
    pub(super) npm_operations: Option<&'a super::super::npm::SharedPreparationOperations>,
    pub(super) attention: &'a Arc<Mutex<Option<String>>>,
    pub(super) dependency: &'a Arc<Mutex<Vec<DependencyRequest>>>,
    pub(super) task_id: Option<&'a str>,
    pub(super) baseline_commit: Option<&'a str>,
    pub(super) progress: &'a std::sync::mpsc::Sender<crate::harness::LiveProgress>,
    pub(super) cancel: &'a AtomicBool,
    pub(super) stop: &'a AtomicBool,
    pub(super) request_count: &'a AtomicUsize,
    pub(super) downloaded_bytes: &'a AtomicUsize,
    pub(super) before_authorized: Option<&'a (dyn Fn() + Send + Sync)>,
    pub(super) npm_cache_gate: &'a Mutex<()>,
    pub(super) cargo_cache_gate: &'a Mutex<()>,
}
