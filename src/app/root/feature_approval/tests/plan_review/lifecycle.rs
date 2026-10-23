pub(super) fn assert_review_reconciled(
    planning: &crate::artifacts::planning_store::PlanningStore,
    workflow: &crate::core::workflow::Workflow,
    snapshot: &crate::core::lifecycle::PlanSnapshotV1,
) {
    assert!(crate::core::workflow::feature_approved(
        planning, workflow, "CHG-004"
    ));
    let lifecycle = crate::core::lifecycle::LifecycleStore::new(planning);
    let feature = lifecycle.load_feature(&snapshot.feature_uid).unwrap();
    assert_eq!(
        feature.phase,
        crate::core::lifecycle::FeaturePhase::ReadyToDecompose
    );
    assert_eq!(
        feature.approved_plan_sha256.as_deref(),
        Some(snapshot.content_hash.as_str())
    );
    assert_eq!(feature.decision_refs.len(), 1);
    assert_eq!(feature.review_refs.len(), 1);
    let user = crate::core::lifecycle::plan_snapshot::user_confirmation_receipt(planning, snapshot)
        .unwrap()
        .unwrap();
    let review = crate::core::lifecycle::plan_snapshot::plan_review_receipt(planning, snapshot)
        .unwrap()
        .unwrap();
    let events = lifecycle.events_for_feature(&snapshot.feature_uid).unwrap();
    assert!(events.iter().any(|record| {
        record.actor == crate::core::lifecycle::LifecycleActor::User
            && matches!(
                &record.event,
                crate::core::lifecycle::LifecycleEventKind::Feature(
                    crate::core::lifecycle::FeatureEvent::UserPlanDecision(receipt)
                ) if receipt == &user
            )
    }));
    assert!(events.iter().any(|record| {
        record.actor == crate::core::lifecycle::LifecycleActor::IndependentReviewer
            && matches!(
                &record.event,
                crate::core::lifecycle::LifecycleEventKind::Feature(
                    crate::core::lifecycle::FeatureEvent::PlanReview(receipt)
                ) if receipt == &review
            )
    }));
    assert!(
        lifecycle.list_tasks().unwrap().is_empty(),
        "plan review must not generate or dispatch an implementation task"
    );
}
