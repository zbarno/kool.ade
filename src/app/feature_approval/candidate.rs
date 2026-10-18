pub(super) fn from_feature(
    planning_store: &crate::artifacts::planning_store::PlanningStore,
    feature_id: &str,
    markdown: &str,
) -> Option<crate::core::lifecycle::PlanSnapshotV1> {
    let identity = crate::domain::ArtifactIdentity::from_markdown(markdown)
        .ok()
        .flatten()?;
    crate::core::lifecycle::plan_snapshot::from_feature_with_author(
        planning_store,
        feature_id,
        &identity,
        markdown,
        &crate::core::workflow::feature_contract(markdown),
    )
    .ok()
}
