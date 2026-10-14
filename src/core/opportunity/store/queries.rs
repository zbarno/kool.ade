use super::*;
use decisions::record_user_decision;

impl OpportunityStore<'_> {
    pub fn load(&self, uid: &str) -> Result<(OpportunityV1, u64), StoreError> {
        let path = opportunity_path(uid)?;
        let record = load_record(self.store, &path)?.ok_or_else(|| {
            StoreError::MalformedState(format!("opportunity {uid} does not exist"))
        })?;
        Ok((record.data, record.revision))
    }

    pub fn list(&self) -> Result<Vec<OpportunityV1>, StoreError> {
        Ok(self
            .list_with_revisions()?
            .into_iter()
            .map(|(proposal, _)| proposal)
            .collect())
    }

    pub fn list_with_revisions(&self) -> Result<Vec<(OpportunityV1, u64)>, StoreError> {
        let mut proposals = self
            .store
            .list_record_paths("state/opportunities")?
            .into_iter()
            .map(|path| {
                load_record(self.store, &path)?
                    .map(|record| (record.data, record.revision))
                    .ok_or_else(|| {
                        StoreError::MalformedState(format!("opportunity disappeared: {path}"))
                    })
            })
            .collect::<Result<Vec<_>, _>>()?;
        proposals.sort_by(|left, right| {
            (left.0.discovered_at_ms, &left.0.opportunity_uid)
                .cmp(&(right.0.discovered_at_ms, &right.0.opportunity_uid))
        });
        Ok(proposals)
    }

    pub fn record_user_decision(
        &self,
        expected_record_revision: u64,
        decision: &UiDecisionV1,
        authenticated_user_id: &str,
    ) -> Result<OpportunityDecisionResult, StoreError> {
        record_user_decision(
            self,
            expected_record_revision,
            decision,
            authenticated_user_id,
        )
    }

    #[cfg(test)]
    pub(crate) fn record_user_decision_at(
        &self,
        expected_record_revision: u64,
        decision: &UiDecisionV1,
        authenticated_user_id: &str,
        now_ms: u64,
    ) -> Result<OpportunityDecisionResult, StoreError> {
        decisions::record_user_decision_at(
            self,
            expected_record_revision,
            decision,
            authenticated_user_id,
            now_ms,
        )
    }

    pub fn decision(&self, uid: &str) -> Result<OpportunityDecisionV1, StoreError> {
        decisions::load_decision(self.store, uid)
    }

    pub fn approved_to_plan(&self) -> Result<Vec<OpportunityV1>, StoreError> {
        Ok(self
            .list()?
            .into_iter()
            .filter(|proposal| proposal.status == OpportunityStatusV1::ApprovedToPlan)
            .collect())
    }

    pub fn mark_planning(
        &self,
        uid: &str,
        feature_uid: &str,
        expected_record_revision: u64,
    ) -> Result<(OpportunityV1, u64), StoreError> {
        lifecycle::mark_planning(self, uid, feature_uid, expected_record_revision)
    }
}
