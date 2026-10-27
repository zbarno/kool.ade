use std::{
    collections::BTreeMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc::{SyncSender, TrySendError},
    },
};

use sha2::{Digest, Sha256};

use crate::execution::docker::discovery::{PrerequisiteCode, PrerequisiteFailure};

use super::failure;

const MAX_PROGRESS_EVENTS: usize = 128;
const MAX_PROGRESS_LAYERS: usize = 64;
const MAX_LAYER_ID_BYTES: usize = 256;
pub(crate) const MAX_REPORTED_PULL_BYTES: u64 = 8 * 1024 * 1024 * 1024;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ImagePullProgress {
    pub(crate) current_bytes: Option<u64>,
    pub(crate) total_bytes: Option<u64>,
    pub(crate) percent: Option<u8>,
}

#[derive(Clone, Default)]
struct LayerProgress {
    current: u64,
    total: u64,
}

#[derive(Default)]
struct ProgressState {
    layers: BTreeMap<String, LayerProgress>,
    incomplete: bool,
}

/// One-use pull control shared by the UI worker and its engine operation.
/// The channel must be bounded; updates use `try_send` and never block Docker.
#[derive(Clone)]
pub(crate) struct ImagePullControl {
    cancelled: Arc<AtomicBool>,
    progress: SyncSender<ImagePullProgress>,
    emitted_events: Arc<Mutex<usize>>,
    state: Arc<Mutex<ProgressState>>,
}

impl ImagePullControl {
    pub(crate) fn new(progress: SyncSender<ImagePullProgress>) -> Self {
        Self {
            cancelled: Arc::new(AtomicBool::new(false)),
            progress,
            emitted_events: Arc::new(Mutex::new(0)),
            state: Arc::new(Mutex::new(ProgressState::default())),
        }
    }

    pub(crate) fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
    }

    pub(crate) fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }

    pub(crate) fn record_progress(
        &self,
        layer_id: Option<&str>,
        current: Option<i64>,
        total: Option<i64>,
    ) -> Result<(), PrerequisiteFailure> {
        let (Some(current), Some(total)) = (
            current.and_then(|value| u64::try_from(value).ok()),
            total.and_then(|value| u64::try_from(value).ok()),
        ) else {
            return Ok(());
        };
        if total == 0 {
            return Ok(());
        }
        if total > MAX_REPORTED_PULL_BYTES || current > MAX_REPORTED_PULL_BYTES {
            return Err(failure(
                PrerequisiteCode::ImageUnavailable,
                "Docker reported image transfer progress outside the supported size bound",
                false,
            ));
        }

        let Some(key) = layer_key(layer_id) else {
            self.state
                .lock()
                .map_err(|_| {
                    failure(
                        PrerequisiteCode::CapabilitiesIncompatible,
                        "Image pull progress state is unavailable",
                        false,
                    )
                })?
                .incomplete = true;
            self.send_progress(None, None, None);
            return Ok(());
        };
        let mut state = self.state.lock().map_err(|_| {
            failure(
                PrerequisiteCode::CapabilitiesIncompatible,
                "Image pull progress state is unavailable",
                false,
            )
        })?;
        if !state.layers.contains_key(&key) && state.layers.len() >= MAX_PROGRESS_LAYERS {
            state.incomplete = true;
        } else {
            let layer = state.layers.entry(key).or_default();
            layer.total = layer.total.max(total);
            layer.current = layer.current.max(current.min(layer.total));
        }
        let mut aggregate_current = 0_u64;
        let mut aggregate_total = 0_u64;
        for layer in state.layers.values() {
            aggregate_current = aggregate_current.saturating_add(layer.current);
            aggregate_total = aggregate_total.saturating_add(layer.total);
        }
        if aggregate_total > MAX_REPORTED_PULL_BYTES {
            return Err(failure(
                PrerequisiteCode::ImageUnavailable,
                "Docker reported aggregate image transfer progress outside the supported size bound",
                false,
            ));
        }
        let complete = !state.incomplete;
        drop(state);
        let percent = (complete && aggregate_total != 0).then(|| {
            ((u128::from(aggregate_current) * 100 / u128::from(aggregate_total)).min(100)) as u8
        });
        self.send_progress(
            complete.then_some(aggregate_current),
            complete.then_some(aggregate_total),
            percent,
        );
        Ok(())
    }

    fn send_progress(
        &self,
        current_bytes: Option<u64>,
        total_bytes: Option<u64>,
        percent: Option<u8>,
    ) {
        let Ok(mut emitted_events) = self.emitted_events.lock() else {
            return;
        };
        if *emitted_events >= MAX_PROGRESS_EVENTS {
            return;
        }
        match self.progress.try_send(ImagePullProgress {
            current_bytes,
            total_bytes,
            percent,
        }) {
            Ok(()) => *emitted_events += 1,
            Err(TrySendError::Full(_)) | Err(TrySendError::Disconnected(_)) => {}
        }
    }
}

fn layer_key(value: Option<&str>) -> Option<String> {
    let value = value.filter(|value| !value.is_empty() && value.len() <= MAX_LAYER_ID_BYTES)?;
    Some(format!("{:x}", Sha256::digest(value.as_bytes())))
}

#[cfg(test)]
mod tests;
