use super::*;

macro_rules! plan_review_surface_methods {
    () => {
        fn lifecycle_plan_review(
            &self,
            feature_uid: &str,
        ) -> Result<crate::ui::task_detail::LifecyclePlanReviewView, String> {
            self.load_lifecycle_plan_review(feature_uid)
        }
    };
}

pub(super) use plan_review_surface_methods;

impl KooladeApp {
    pub(super) fn load_lifecycle_plan_review(
        &self,
        feature_uid: &str,
    ) -> Result<crate::ui::task_detail::LifecyclePlanReviewView, String> {
        let Screen::Connected(project) = &self.screen else {
            return Err("Connect a workspace to view plan review evidence.".into());
        };
        let store = crate::core::lifecycle::LifecycleStore::new(&project.state.planning_store);
        let feature = store
            .load_feature(feature_uid)
            .map_err(|error| error.to_string())?;
        if feature.phase != crate::core::lifecycle::FeaturePhase::PlanReviewPending {
            return Err("This feature is no longer awaiting independent plan review.".into());
        }
        let snapshot_uid = feature
            .plan_snapshot_uid
            .as_deref()
            .ok_or_else(|| "The confirmed plan snapshot is unavailable.".to_owned())?;
        let snapshot = crate::core::lifecycle::plan_snapshot::load_persisted(
            &project.state.planning_store,
            snapshot_uid,
        )
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "The confirmed plan snapshot has not been persisted.".to_owned())?;
        if snapshot.feature_uid != feature_uid
            || feature.plan_snapshot_sha256.as_deref() != Some(snapshot.content_hash.as_str())
        {
            return Err("The plan review snapshot does not match the current feature.".into());
        }
        let confirmation = crate::core::lifecycle::plan_snapshot::user_confirmation_receipt(
            &project.state.planning_store,
            &snapshot,
        )
        .map_err(|error| error.to_string())?;
        let review = crate::core::lifecycle::plan_snapshot::plan_review_receipt(
            &project.state.planning_store,
            &snapshot,
        )
        .map_err(|error| error.to_string())?;
        let verdict = crate::core::lifecycle::plan_snapshot::plan_review_verdict(
            &project.state.planning_store,
            &snapshot,
        )
        .map_err(|error| error.to_string())?;
        Ok(crate::ui::task_detail::LifecyclePlanReviewView {
            feature_uid: feature_uid.into(),
            snapshot,
            confirmation,
            review,
            verdict,
        })
    }
}
