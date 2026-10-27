use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{
        Arc, Mutex,
        atomic::AtomicU64,
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
    pending_layers: BTreeSet<String>,
    incomplete: bool,
}

/// One-use pull control shared by the UI worker and its engine operation.
/// The channel must be bounded; updates use `try_send` and never block Docker.
#[derive(Clone)]
pub(crate) struct ImagePullControl {
    cancelled: Arc<AtomicBool>,
    maximum_reported_bytes: Arc<AtomicU64>,
    progress: SyncSender<ImagePullProgress>,
    emitted_events: Arc<Mutex<usize>>,
    state: Arc<Mutex<ProgressState>>,
}

impl ImagePullControl {
    pub(crate) fn new(progress: SyncSender<ImagePullProgress>) -> Self {
        Self {
            cancelled: Arc::new(AtomicBool::new(false)),
            maximum_reported_bytes: Arc::new(AtomicU64::new(MAX_REPORTED_PULL_BYTES)),
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

    pub(crate) fn restrict_reported_bytes(&self, maximum: u64) {
        self.maximum_reported_bytes
            .fetch_min(maximum, Ordering::AcqRel);
    }

    pub(crate) fn record_unmeasured_layer(
        &self,
        layer_id: Option<&str>,
        terminal_without_transfer: bool,
    ) -> Result<(), PrerequisiteFailure> {
        let maximum = self.maximum_reported_bytes.load(Ordering::Acquire);
        if maximum == MAX_REPORTED_PULL_BYTES {
            return Ok(());
        }
        let Some(key) = layer_key(layer_id) else {
            return Err(bounded_progress_failure());
        };
        let mut state = self.state.lock().map_err(|_| {
            failure(
                PrerequisiteCode::CapabilitiesIncompatible,
                "Image pull progress state is unavailable",
                false,
            )
        })?;
        if state.layers.contains_key(&key) {
            state.pending_layers.remove(&key);
            return Ok(());
        }
        if terminal_without_transfer {
            if state.pending_layers.contains(&key) {
                return Err(bounded_progress_failure());
            }
            return Ok(());
        }
        if !state.pending_layers.contains(&key) && state.pending_layers.len() >= MAX_PROGRESS_LAYERS
        {
            return Err(bounded_progress_failure());
        }
        state.pending_layers.insert(key);
        Ok(())
    }

    pub(crate) fn finish_pull_attempt(&self) -> Result<(), PrerequisiteFailure> {
        let maximum = self.maximum_reported_bytes.load(Ordering::Acquire);
        if maximum == MAX_REPORTED_PULL_BYTES {
            return Ok(());
        }
        let state = self.state.lock().map_err(|_| {
            failure(
                PrerequisiteCode::CapabilitiesIncompatible,
                "Image pull progress state is unavailable",
                false,
            )
        })?;
        if state.pending_layers.is_empty() {
            Ok(())
        } else {
            Err(bounded_progress_failure())
        }
    }

    pub(crate) fn record_progress(
        &self,
        layer_id: Option<&str>,
        current: Option<i64>,
        total: Option<i64>,
    ) -> Result<(), PrerequisiteFailure> {
        let maximum = self.maximum_reported_bytes.load(Ordering::Acquire);
        let strict = maximum < MAX_REPORTED_PULL_BYTES;
        let (Some(current), Some(total)) = (
            current.and_then(|value| u64::try_from(value).ok()),
            total.and_then(|value| u64::try_from(value).ok()),
        ) else {
            if strict {
                return Err(bounded_progress_failure());
            }
            return Ok(());
        };
        if total == 0 {
            if strict {
                return Err(bounded_progress_failure());
            }
            return Ok(());
        }
        if total > maximum || current > maximum {
            return Err(failure(
                PrerequisiteCode::ImageUnavailable,
                "Docker reported image transfer progress outside the supported size bound",
                false,
            ));
        }

        let Some(key) = layer_key(layer_id) else {
            if strict {
                return Err(bounded_progress_failure());
            }
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
        state.pending_layers.remove(&key);
        if !state.layers.contains_key(&key) && state.layers.len() >= MAX_PROGRESS_LAYERS {
            if strict {
                return Err(bounded_progress_failure());
            }
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
        if aggregate_total > maximum {
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

fn bounded_progress_failure() -> PrerequisiteFailure {
    failure(
        PrerequisiteCode::ImageUnavailable,
        "Docker did not provide complete size-bounded progress for the bootstrap image pull",
        false,
    )
}

#[cfg(test)]
mod tests;
