use crate::execution::{docker::backend::internal_error_with_context, errors::ExecutionError};

pub(super) struct WorkspaceStartReservation {
    prepared: std::sync::Arc<std::sync::Mutex<super::super::super::super::PreparedWorkspace>>,
    active: bool,
    execution_id: Option<String>,
}

impl WorkspaceStartReservation {
    pub(super) fn new(
        prepared: std::sync::Arc<std::sync::Mutex<super::super::super::super::PreparedWorkspace>>,
    ) -> Self {
        Self {
            prepared,
            active: true,
            execution_id: None,
        }
    }

    pub(super) fn activate(&mut self, execution_id: &str) -> Result<(), ExecutionError> {
        let mut prepared = self
            .prepared
            .lock()
            .map_err(|_| internal_error_with_context("activating the workspace execution"))?;
        prepared.starting = false;
        prepared.active_execution_id = Some(execution_id.to_owned());
        self.execution_id = Some(execution_id.to_owned());
        Ok(())
    }

    pub(super) fn commit(&mut self) {
        self.active = false;
    }
}

impl Drop for WorkspaceStartReservation {
    fn drop(&mut self) {
        if self.active
            && let Ok(mut prepared) = self.prepared.lock()
        {
            if let Some(execution_id) = &self.execution_id {
                if prepared.active_execution_id.as_deref() == Some(execution_id) {
                    prepared.active_execution_id = None;
                }
            } else {
                prepared.starting = false;
            }
        }
    }
}
