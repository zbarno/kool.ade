use crate::persistence::setup_tasks::{FailureCode, SetupTaskTransitionV1};

pub(super) struct AttemptFailureGuard {
    task_id: String,
    operation_id: String,
    armed: bool,
}

impl AttemptFailureGuard {
    pub(super) fn new(task_id: &str, operation_id: &str) -> Self {
        Self {
            task_id: task_id.to_owned(),
            operation_id: operation_id.to_owned(),
            armed: true,
        }
    }

    pub(super) fn complete(&mut self) {
        self.armed = false;
    }
}

impl Drop for AttemptFailureGuard {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        let at_unix_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
            .min(u64::MAX as u128) as u64;
        let _ =
            crate::persistence::setup_tasks::apply_transition(SetupTaskTransitionV1::FailAttempt {
                task_id: self.task_id.clone(),
                operation_id: self.operation_id.clone(),
                failure: FailureCode::OperationFailed,
                at_unix_ms,
            });
    }
}
