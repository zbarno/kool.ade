use std::{
    cell::{Cell, RefCell},
    sync::{Arc, atomic::AtomicBool, mpsc},
};

use super::ProbeMessage;

thread_local! {
    static STUB_WORKER_LAUNCH: Cell<bool> = const { Cell::new(false) };
    static LAUNCHED_GENERATIONS: RefCell<Vec<u64>> = const { RefCell::new(Vec::new()) };
    static RETRIED_PROVIDERS: RefCell<Vec<(u64, String)>> = const { RefCell::new(Vec::new()) };
}

struct RestoreLauncher(bool);

impl Drop for RestoreLauncher {
    fn drop(&mut self) {
        STUB_WORKER_LAUNCH.with(|slot| slot.set(self.0));
    }
}

pub(crate) fn with_worker_launch_stub<R>(run: impl FnOnce() -> R) -> R {
    let previous = STUB_WORKER_LAUNCH.with(|slot| slot.replace(true));
    let _restore = RestoreLauncher(previous);
    LAUNCHED_GENERATIONS.with(|generations| generations.borrow_mut().clear());
    RETRIED_PROVIDERS.with(|providers| providers.borrow_mut().clear());
    run()
}

pub(super) fn launch_if_configured(
    generation: u64,
    tx: mpsc::Sender<ProbeMessage>,
    cancelled: Arc<AtomicBool>,
) -> bool {
    if !STUB_WORKER_LAUNCH.with(Cell::get) {
        return false;
    }
    LAUNCHED_GENERATIONS.with(|generations| generations.borrow_mut().push(generation));
    drop((tx, cancelled));
    true
}

pub(crate) fn launched_generations() -> Vec<u64> {
    LAUNCHED_GENERATIONS.with(|generations| generations.borrow().clone())
}

pub(super) fn retry_if_configured(generation: u64, provider: &str) -> bool {
    if !STUB_WORKER_LAUNCH.with(Cell::get) {
        return false;
    }
    RETRIED_PROVIDERS.with(|providers| providers.borrow_mut().push((generation, provider.into())));
    true
}

pub(crate) fn retried_providers() -> Vec<(u64, String)> {
    RETRIED_PROVIDERS.with(|providers| providers.borrow().clone())
}
