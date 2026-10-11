use super::{ClaimError, ClaimLease, ClaimRecord, git};

impl ClaimLease {
    /// Atomically claim a stable task UID on the planning repository remote.
    /// `Ok(None)` means the repository has no configured push remote.
    pub fn acquire(
        repo: &std::path::Path,
        project_uid: uuid::Uuid,
        task_uid: &str,
        code_repository_id: &str,
        base_commit: &str,
    ) -> Result<Option<Self>, ClaimError> {
        git::acquire(repo, project_uid, task_uid, code_repository_id, base_commit)
    }

    /// Explicit compare-and-swap takeover. The caller must present the session
    /// shown to the operator; the planning claim must also be expired.
    pub fn take_over_stale(
        repo: &std::path::Path,
        project_uid: uuid::Uuid,
        task_uid: &str,
        code_repository_id: &str,
        expected_session_id: &str,
        base_commit: &str,
    ) -> Result<Self, ClaimError> {
        git::take_over_stale(
            repo,
            project_uid,
            task_uid,
            code_repository_id,
            expected_session_id,
            base_commit,
        )
    }

    pub fn record(&self) -> Result<ClaimRecord, ClaimError> {
        git::read_claim(&self.repo, &self.remote, &self.reference, &self.object)
    }

    pub(crate) fn verify(&self) -> Result<(), ClaimError> {
        self.verify_identity(&self.record()?)?;
        git::verify(&self.repo, &self.remote, &self.reference, &self.object)
    }

    pub(crate) fn refresh(&mut self) -> Result<(), ClaimError> {
        self.verify_identity(&self.record()?)?;
        self.object = git::refresh(&self.repo, &self.remote, &self.reference, &self.object)?;
        Ok(())
    }

    fn verify_identity(&self, record: &ClaimRecord) -> Result<(), ClaimError> {
        if record.project_uid != self.project_uid
            || record.task_uid != self.task_uid
            || record.code_repository_id != self.code_repository_id
        {
            return Err(ClaimError::CoordinationRejected(
                "Planning claim identity no longer matches the selected project task and code repository".into(),
            ));
        }
        Ok(())
    }

    pub(crate) fn fenced_push(
        &mut self,
        source: &std::path::Path,
        commit: &str,
        destination_ref: &str,
    ) -> Result<(), ClaimError> {
        self.verify_identity(&self.record()?)?;
        self.object = git::fenced_push(
            &self.repo,
            &self.remote,
            &self.reference,
            &self.object,
            source,
            commit,
            destination_ref,
        )?;
        self.verify()
    }
}

impl Drop for ClaimLease {
    fn drop(&mut self) {
        let _ = git::release(&self.repo, &self.remote, &self.reference, &self.object);
    }
}
