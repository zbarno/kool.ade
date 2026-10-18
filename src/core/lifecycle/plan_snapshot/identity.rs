use super::{PlanSnapshotV1, snapshot_uid};

impl PlanSnapshotV1 {
    pub(crate) fn with_original_goal_ref(mut self, reference: String) -> Result<Self, String> {
        if reference.trim().is_empty() {
            return Err("original goal reference cannot be empty".into());
        }
        self.original_goal_ref = reference;
        self.content_hash = self.compute_content_hash()?;
        self.uid = snapshot_uid(&self.feature_uid, &self.content_hash)?;
        self.validate()?;
        Ok(self)
    }

    pub(crate) fn with_deferred_assumptions(
        mut self,
        deferred_assumptions: Vec<String>,
    ) -> Result<Self, String> {
        self.deferred_assumptions = deferred_assumptions;
        self.content_hash = self.compute_content_hash()?;
        self.uid = snapshot_uid(&self.feature_uid, &self.content_hash)?;
        self.validate()?;
        Ok(self)
    }
}
