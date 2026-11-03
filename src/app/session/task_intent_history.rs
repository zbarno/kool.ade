pub(crate) fn capture(
    state: &crate::core::state::PlannerState,
    task_documents: &[crate::artifacts::task_docs::TaskDocument],
    work: &mut [crate::core::planning_work::Work],
) -> bool {
    let lifecycle = crate::core::lifecycle::LifecycleStore::new(&state.planning_store);
    let features = lifecycle.list_features().unwrap_or_default();
    let plan_decisions = features
        .iter()
        .flat_map(|feature| {
            lifecycle
                .events_for_feature(&feature.feature_uid)
                .unwrap_or_default()
                .into_iter()
                .filter_map(|record| match record.event {
                    crate::core::lifecycle::LifecycleEventKind::Feature(
                        crate::core::lifecycle::FeatureEvent::UserPlanDecision(receipt),
                    ) => Some((feature.feature_uid.clone(), receipt)),
                    _ => None,
                })
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    let tasks = lifecycle.list_tasks().unwrap_or_default();
    let deployments = features
        .iter()
        .filter_map(|feature| {
            let current = lifecycle
                .load_current_implementation(&feature.feature_uid)
                .ok()??;
            let deployment = lifecycle
                .load_deployment_evidence(&feature.feature_uid)
                .ok()??;
            Some((deployment, current))
        })
        .collect::<Vec<_>>();
    let items = state
        .items
        .iter()
        .chain(&state.resolved_items)
        .cloned()
        .collect::<Vec<_>>();
    crate::core::planning_work::capture_authoritative_events(
        work,
        &items,
        task_documents,
        &plan_decisions,
        &tasks,
        &deployments,
    )
}
