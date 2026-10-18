use super::DockerExecutionBackend;
use crate::execution::backend::ExecutionHandleV1;

impl DockerExecutionBackend {
    pub(super) fn diagnostic_progress(&self, handle: &ExecutionHandleV1) -> Option<u8> {
        let execution = self.lookup_execution(handle).ok()?;
        let state = execution.lock().ok()?;
        Some(
            state
                .diagnostic_progress
                .load(std::sync::atomic::Ordering::Relaxed),
        )
    }

    pub(super) fn diagnostic_metrics(&self, handle: &ExecutionHandleV1) -> Option<u16> {
        let execution = self.lookup_execution(handle).ok()?;
        let state = execution.lock().ok()?;
        Some(
            state
                .diagnostic_metrics
                .load(std::sync::atomic::Ordering::Relaxed),
        )
    }
}
