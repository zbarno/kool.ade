use super::{ClaimError, ClaimLease, ClaimRequest};

impl ClaimRequest {
    pub(crate) fn task_uid(&self) -> &str {
        &self.task_uid
    }

    pub(crate) fn new(project_uid: uuid::Uuid, task_uid: String) -> Self {
        Self {
            state_root: crate::persistence::state_root(),
            project_uid,
            task_uid,
        }
    }

    #[cfg(test)]
    pub(crate) fn new_at(
        state_root: std::path::PathBuf,
        project_uid: uuid::Uuid,
        task_uid: String,
    ) -> Self {
        Self {
            state_root,
            project_uid,
            task_uid,
        }
    }

    pub(crate) fn acquire(self) -> Result<ClaimLease, ClaimError> {
        ClaimLease::acquire(&self.state_root, self.project_uid, &self.task_uid)
    }
}
