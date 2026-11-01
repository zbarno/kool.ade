pub(super) fn feature_branch_targets(
    works: &[crate::core::planning_work::Work],
    feature_id: &str,
    feature_uid: Option<&str>,
    default_branch: &str,
) -> crate::core::workflow::BranchTargets {
    let selected = works.iter().find(|work| {
        !matches!(
            work.kind,
            crate::core::planning_work::WorkKind::TaskGeneration
                | crate::core::planning_work::WorkKind::PlanReviewDisposition
                | crate::core::planning_work::WorkKind::CodeReviewDisposition
                | crate::core::planning_work::WorkKind::QaFindingDisposition
        ) && crate::core::planning_work::belongs_to_feature(work, feature_id, feature_uid)
    });
    crate::core::workflow::BranchTargets {
        source: selected
            .and_then(|work| work.source_branch.clone())
            .unwrap_or_else(|| default_branch.to_owned()),
        destination: selected
            .and_then(|work| work.destination_branch.clone())
            .unwrap_or_else(|| default_branch.to_owned()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generation_preserves_branch_intent_when_new_feature_uid_is_not_linked_yet() {
        let mut work = crate::core::planning_work::Work::new(
            "feature:export".into(),
            "Export feature".into(),
            "Export project data".into(),
            String::new(),
        );
        work.feature_id = Some("F-38".into());
        work.source_branch = Some("release/2.1".into());
        work.destination_branch = Some("integration".into());
        let targets = feature_branch_targets(&[work], "F-38", None, "main");
        assert_eq!(targets.source, "release/2.1");
        assert_eq!(targets.destination, "integration");
    }
}
