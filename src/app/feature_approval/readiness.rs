pub(crate) fn current(
    store: &crate::artifacts::planning_store::PlanningStore,
    snapshot: &crate::core::lifecycle::PlanSnapshotV1,
) -> Result<(), String> {
    crate::core::lifecycle::plan_snapshot::require_current_readiness(store, snapshot).map(|_| ())
}
