use crate::artifacts::planning_store::StoreError;

use super::SelfImprovementStore;
use crate::core::self_improvement::{SelfImprovementPhaseV1, SelfImprovementRunV1};

pub struct TaskSourceAuthorization<'a> {
    pub feature_uid: &'a str,
    pub task_uid: &'a str,
    pub repository_uid: &'a str,
    pub repository_remote: &'a str,
    pub source_ref: Option<&'a str>,
    pub source_sha: Option<&'a str>,
    pub destination_ref: Option<&'a str>,
    pub isolated_clone: bool,
}

impl SelfImprovementStore<'_> {
    /// Fence ordinary task dispatch to the reviewed self-improvement plan and
    /// its canonical repository. Parent tasks define PR units; their child
    /// tasks still use this same execution fence.
    pub fn authorize_task_start(
        &self,
        feature_uid: &str,
        task_uid: Option<&str>,
        repository_uid: &str,
        repository_remote: &str,
    ) -> Result<(), StoreError> {
        let Some(run) = self.run_for_feature(feature_uid)? else {
            if self.is_linked_product_opportunity(feature_uid)? {
                return Err(StoreError::MalformedState(
                    "Kool.ad/e opportunity is missing its persisted self-improvement run".into(),
                ));
            }
            return Ok(());
        };
        let executable = matches!(
            run.phase,
            SelfImprovementPhaseV1::Ready
                | SelfImprovementPhaseV1::Implementing
                | SelfImprovementPhaseV1::Reviewing
                | SelfImprovementPhaseV1::Qa
                | SelfImprovementPhaseV1::SpecSync
                | SelfImprovementPhaseV1::DoneAwaitingPr
                | SelfImprovementPhaseV1::PrPending
                | SelfImprovementPhaseV1::NeedsAttention
        );
        let planned_task = run
            .task_uids
            .iter()
            .any(|uid| Some(uid.as_str()) == task_uid);
        if !executable
            || !planned_task
            || repository_uid != run.target_repository_uid
            || super::super::canonical_product_remote(repository_remote)
                != Some(run.canonical_remote.as_str())
        {
            return Err(StoreError::MalformedState(
                "self-improvement task dispatch is not authorized for this run, planned task, or target repository".into(),
            ));
        }
        Ok(())
    }

    fn is_linked_product_opportunity(&self, feature_uid: &str) -> Result<bool, StoreError> {
        use crate::core::opportunity::{OpportunityStore, OpportunityTargetKindV1};

        let opportunities = OpportunityStore::new(self.store);
        if opportunities.list()?.iter().any(|proposal| {
            proposal.target.kind == OpportunityTargetKindV1::KooladeProduct
                && proposal
                    .related_feature_uids
                    .iter()
                    .any(|uid| uid == feature_uid)
        }) {
            return Ok(true);
        }

        let feature =
            crate::core::lifecycle::LifecycleStore::new(self.store).load_feature(feature_uid)?;
        let opportunity_uid = feature.draft.lines().find_map(|line| {
            line.strip_prefix("<!-- opportunity-uid: ")
                .and_then(|line| line.strip_suffix(" -->"))
        });
        let Some(opportunity_uid) = opportunity_uid else {
            return Ok(false);
        };
        let (proposal, _) = opportunities.load(opportunity_uid)?;
        Ok(proposal.target.kind == OpportunityTargetKindV1::KooladeProduct)
    }

    pub fn run_for_feature(
        &self,
        feature_uid: &str,
    ) -> Result<Option<SelfImprovementRunV1>, StoreError> {
        let paths = self.store.list_record_paths("state/self_improvements")?;
        let mut linked = None;
        for path in paths {
            let Some(uid) = path
                .strip_prefix("state/self_improvements/")
                .and_then(|file| file.strip_suffix(".json"))
            else {
                continue;
            };
            let run = self.load(uid)?;
            if run.feature_uid == feature_uid {
                if linked.is_some() {
                    return Err(StoreError::MalformedState(
                        "multiple self-improvement runs are linked to one feature".into(),
                    ));
                }
                linked = Some(run);
            }
        }
        Ok(linked)
    }

    /// Confirm that the resolved task clone is pinned to the approved source
    /// identity, SHA, and branch targets before implementation starts.
    pub fn authorize_task_source(
        &self,
        authorization: TaskSourceAuthorization<'_>,
    ) -> Result<(), StoreError> {
        self.authorize_task_start(
            authorization.feature_uid,
            Some(authorization.task_uid),
            authorization.repository_uid,
            authorization.repository_remote,
        )?;
        let Some(run) = self.run_for_feature(authorization.feature_uid)? else {
            return Ok(());
        };
        if !authorization.isolated_clone
            || authorization.source_ref != run.source_ref.as_deref()
            || authorization.source_sha != Some(run.source_sha.as_str())
            || authorization.destination_ref != run.destination_ref.as_deref()
        {
            return Err(StoreError::MalformedState(
                "self-improvement clone does not match the approved source SHA and branch targets"
                    .into(),
            ));
        }
        Ok(())
    }
}
