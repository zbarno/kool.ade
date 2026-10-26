use std::sync::mpsc::TryRecvError;

use crate::execution::docker::engine::ImagePullProgress;

use super::super::{Controller, WorkKind};

impl Controller {
    pub(crate) fn cancel_preparation(&mut self) {
        if self.pull_cancel_requested || !matches!(self.work_kind, Some(WorkKind::Prepare)) {
            return;
        }
        self.pull_cancel_requested = true;
        if let Some(control) = &self.pull_control {
            control.cancel();
        }
    }

    pub(super) fn poll_pull_progress(&mut self) -> bool {
        let mut latest = None;
        let mut disconnected = false;
        if let Some(receiver) = &self.progress_receiver {
            loop {
                match receiver.try_recv() {
                    Ok(progress) => latest = Some(progress),
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => {
                        disconnected = true;
                        break;
                    }
                }
            }
        }
        if disconnected {
            self.progress_receiver = None;
        }
        if let Some(progress) = latest {
            self.pull_progress = Some(progress);
            true
        } else {
            false
        }
    }

    pub(super) fn latest_pull_progress(&self) -> Option<&ImagePullProgress> {
        self.pull_progress.as_ref()
    }
}
