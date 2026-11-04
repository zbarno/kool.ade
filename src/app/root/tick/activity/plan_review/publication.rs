use crate::app::session::Project;

pub(super) fn active_reviewed_feature(
    project: &Project,
    feature_id: &str,
    snapshot_uid: &str,
    snapshot_hash: &str,
) -> Option<(String, String)> {
    let markdown = project
        .state
        .active_features
        .iter()
        .find(|(id, _)| id == feature_id)
        .map(|(_, markdown)| markdown)?;
    let identity = crate::domain::ArtifactIdentity::from_markdown(markdown)
        .ok()
        .flatten()?;
    let contract = crate::core::workflow::feature_contract(markdown);
    let snapshot = crate::core::lifecycle::plan_snapshot::from_feature_with_author(
        &project.state.planning_store,
        feature_id,
        &identity,
        markdown,
        &contract,
    )
    .ok()?;
    (is_current_snapshot(&snapshot, feature_id, snapshot_uid, snapshot_hash)
        && snapshot.feature_uid == identity.uid)
        .then(|| (identity.uid, markdown.clone()))
}

fn is_current_snapshot(
    snapshot: &crate::core::lifecycle::PlanSnapshotV1,
    feature_id: &str,
    snapshot_uid: &str,
    snapshot_hash: &str,
) -> bool {
    snapshot.feature_id == feature_id
        && snapshot.uid == snapshot_uid
        && snapshot.content_hash == snapshot_hash
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reviewed_snapshot_resolves_to_its_feature_identity() {
        // A review snapshot has its own UID. Publication must resolve the active feature UID
        // by feature ID and prove that the current snapshot UID and hash still match.
        let markdown = crate::domain::ArtifactIdentity::preserve_markdown(
            "# F1: Reviewed feature\n\n## Intent\n\nPlan the reviewed feature.\n\n## Current Behavior\n\nNo implementation exists.\n\n## Desired Behavior\n\nThe feature is reviewed before task generation.\n\n## Scope\n\n- Review this feature.\n\n## Requirements\n\n- REQ-001 (MUST). Preserve review freshness.\n\n## Acceptance Criteria\n\n- AC-001: Changed snapshots are rejected.\n",
            None,
            "F1",
            "Reviewed feature",
        )
        .unwrap();
        let identity = crate::domain::ArtifactIdentity::from_markdown(&markdown)
            .unwrap()
            .unwrap();
        let snapshot = crate::core::lifecycle::PlanSnapshotV1::from_feature(
            "F1",
            &identity,
            &markdown,
            "review-contract",
        )
        .unwrap();
        assert_ne!(snapshot.uid, identity.uid);

        assert!(is_current_snapshot(
            &snapshot,
            "F1",
            &snapshot.uid,
            &snapshot.content_hash,
        ));
        assert!(!is_current_snapshot(
            &snapshot,
            "F1",
            &identity.uid,
            &snapshot.content_hash,
        ));
        assert!(!is_current_snapshot(
            &snapshot,
            "F1",
            &snapshot.uid,
            &"0".repeat(64),
        ));
    }
}
