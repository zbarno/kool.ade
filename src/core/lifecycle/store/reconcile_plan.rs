use super::{FeatureEvent, FeatureGoal, LifecycleEventKind, LifecycleStore, Snapshot, records};
use crate::artifacts::planning_store::StoreError;
use crate::core::lifecycle::{FeaturePhase, GateActor, GateDecision, GateReceipt, PlanSnapshotV1};

impl LifecycleStore<'_> {
    /// Seed the canonical lifecycle from the exact persisted user and reviewer
    /// receipts for a plan. Replays are accepted only when all bound identities
    /// and evidence match the existing lifecycle record.
    pub fn reconcile_reviewed_plan(
        &self,
        plan: &PlanSnapshotV1,
        draft: &str,
        readiness_evidence_refs: Vec<String>,
        user_receipt: &GateReceipt,
        review_receipt: &GateReceipt,
    ) -> Result<FeatureGoal, StoreError> {
        validate_receipts(plan, &readiness_evidence_refs, user_receipt, review_receipt)?;
        let current_product_hash =
            crate::core::lifecycle::product_spec::current_product_hash(self.store)?;
        if crate::core::lifecycle::plan_snapshot::confirmed_product_spec_hash(user_receipt)
            != Some(current_product_hash.as_str())
        {
            return Err(StoreError::MalformedState(
                "product specification changed after plan confirmation; review current drift before task generation".into(),
            ));
        }
        super::spec_sync::require_product_specification(self)?;
        let expected_event_uids = transition_ids(plan)?;
        let transitions = [
            FeatureEvent::ReadinessRecorded {
                evidence_refs: readiness_evidence_refs.clone(),
            },
            FeatureEvent::PlanReady {
                plan_sha256: plan.content_hash.clone(),
            },
            FeatureEvent::UserPlanDecision(user_receipt.clone()),
            FeatureEvent::PlanReview(review_receipt.clone()),
        ];
        let feature_path = records::feature_path(&plan.feature_uid);
        let feature = if let Ok(existing) = self
            .store
            .read_record::<Snapshot<FeatureGoal>>(&feature_path)
        {
            let (snapshot, actual) = existing;
            validate_partial(&snapshot, actual, plan)?;
            let existing_events = self.events_for_feature(&plan.feature_uid)?;
            let prefix_len = actual.min(expected_event_uids.len() as u64) as usize;
            for (index, expected_uid) in expected_event_uids.iter().take(prefix_len).enumerate() {
                let expected_event = if index == 0 {
                    LifecycleEventKind::FeatureCreated
                } else {
                    LifecycleEventKind::Feature(transitions[index - 1].clone())
                };
                if !existing_events
                    .iter()
                    .any(|record| record.uid == *expected_uid && record.event == expected_event)
                {
                    return Err(StoreError::MalformedState(
                        "lifecycle plan snapshot has an invalid transition prefix".into(),
                    ));
                }
            }
            if actual >= expected_event_uids.len() as u64 {
                validate_existing(
                    &snapshot,
                    actual,
                    plan,
                    &readiness_evidence_refs,
                    user_receipt,
                    review_receipt,
                )?;
                snapshot.record
            } else {
                self.apply_reviewed_plan_suffix(
                    snapshot.record,
                    actual,
                    plan,
                    &expected_event_uids,
                    &transitions,
                )?
            }
        } else {
            let goal = FeatureGoal {
                schema_version: 1,
                feature_uid: plan.feature_uid.clone(),
                original_intent: plan.intent.trim().to_owned(),
                draft: draft.to_owned(),
                scope: plan.scope.clone(),
                non_goals: plan.non_goals.clone(),
                revision: 0,
                phase: FeaturePhase::Exploring,
                approved_plan_sha256: None,
                plan_snapshot_uid: Some(plan.uid.clone()),
                plan_snapshot_sha256: Some(plan.content_hash.clone()),
                unresolved_questions: Vec::new(),
                decision_refs: Vec::new(),
                review_refs: Vec::new(),
                readiness_evidence_refs: Vec::new(),
                task_uids: Vec::new(),
            };
            self.create_feature(&goal, &expected_event_uids[0])?;
            self.apply_reviewed_plan_suffix(goal, 1, plan, &expected_event_uids, &transitions)?
        };
        super::spec_sync::record_approved_plan_revision(self, &feature, &expected_event_uids)?;
        Ok(feature)
    }

    fn apply_reviewed_plan_suffix(
        &self,
        mut goal: FeatureGoal,
        current_revision: u64,
        plan: &PlanSnapshotV1,
        event_uids: &[String],
        transitions: &[FeatureEvent; 4],
    ) -> Result<FeatureGoal, StoreError> {
        for transition_index in current_revision.saturating_sub(1) as usize..transitions.len() {
            goal = self.apply_feature_event(
                &plan.feature_uid,
                transition_index as u64 + 1,
                &event_uids[transition_index + 1],
                transitions[transition_index].clone(),
            )?;
        }
        Ok(goal)
    }
}

fn validate_partial(
    snapshot: &Snapshot<FeatureGoal>,
    actual_revision: u64,
    plan: &PlanSnapshotV1,
) -> Result<(), StoreError> {
    let goal = &snapshot.record;
    if snapshot.uid != plan.feature_uid
        || snapshot.revision != actual_revision
        || goal.revision != actual_revision
        || goal.plan_snapshot_uid.as_deref() != Some(plan.uid.as_str())
        || goal.plan_snapshot_sha256.as_deref() != Some(plan.content_hash.as_str())
        || !matches!(
            goal.phase,
            FeaturePhase::Exploring
                | FeaturePhase::AwaitingPlanConfirmation
                | FeaturePhase::PlanReviewPending
                | FeaturePhase::ReadyToDecompose
        )
    {
        return Err(StoreError::MalformedState(
            "existing lifecycle feature is bound to a different or invalid plan prefix".into(),
        ));
    }
    goal.validate().map_err(StoreError::MalformedState)
}

fn validate_receipts(
    plan: &PlanSnapshotV1,
    evidence_refs: &[String],
    user: &GateReceipt,
    review: &GateReceipt,
) -> Result<(), StoreError> {
    plan.validate().map_err(StoreError::MalformedState)?;
    user.validate().map_err(StoreError::MalformedState)?;
    review.validate().map_err(StoreError::MalformedState)?;
    if plan.intent.trim().is_empty()
        || evidence_refs.is_empty()
        || user.actor != GateActor::User
        || user.decision != GateDecision::Approved
        || user.subject_uid != plan.uid
        || user.subject_sha256 != plan.content_hash
        || review.actor != GateActor::IndependentReviewer
        || review.decision != GateDecision::Approved
        || review.subject_uid != plan.uid
        || review.subject_sha256 != plan.content_hash
        || review.prompt_role() != Some(crate::harness::prompt_catalog::PromptRole::PlanReviewer)
        || evidence_refs.len() > 256
        || evidence_refs
            .iter()
            .any(|reference| reference.trim().is_empty() || reference.len() > 2048)
    {
        return Err(StoreError::MalformedState(
            "reviewed plan lifecycle requires exact persisted readiness, user, and PlanReviewer evidence".into(),
        ));
    }
    Ok(())
}

fn validate_existing(
    snapshot: &Snapshot<FeatureGoal>,
    actual_revision: u64,
    plan: &PlanSnapshotV1,
    readiness_refs: &[String],
    user: &GateReceipt,
    review: &GateReceipt,
) -> Result<(), StoreError> {
    let goal = &snapshot.record;
    if snapshot.uid != plan.feature_uid
        || snapshot.revision != actual_revision
        || goal.revision != actual_revision
        || goal.plan_snapshot_uid.as_deref() != Some(plan.uid.as_str())
        || goal.approved_plan_sha256.as_deref() != Some(plan.content_hash.as_str())
        || goal.decision_refs != [user.receipt_uid.clone()]
        || goal.review_refs != [review.receipt_uid.clone()]
        || goal.readiness_evidence_refs != readiness_refs
        || !matches!(
            goal.phase,
            FeaturePhase::ReadyToDecompose
                | FeaturePhase::Decomposing
                | FeaturePhase::Executing
                | FeaturePhase::Complete
        )
    {
        return Err(StoreError::MalformedState(
            "existing lifecycle feature is bound to a different plan or receipt".into(),
        ));
    }
    goal.validate().map_err(StoreError::MalformedState)
}

fn transition_ids(plan: &PlanSnapshotV1) -> Result<Vec<String>, StoreError> {
    let namespace = uuid::Uuid::parse_str(&plan.feature_uid)
        .map_err(|error| StoreError::MalformedState(error.to_string()))?;
    Ok([
        "created",
        "readiness",
        "plan-ready",
        "user-confirmation",
        "plan-review",
    ]
    .into_iter()
    .map(|kind| {
        uuid::Uuid::new_v5(
            &namespace,
            format!("{}:{kind}", plan.content_hash).as_bytes(),
        )
        .hyphenated()
        .to_string()
    })
    .collect())
}
