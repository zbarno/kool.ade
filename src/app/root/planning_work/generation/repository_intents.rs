pub(super) fn feature_git_intents(
    works: &[crate::core::planning_work::Work],
    feature_id: &str,
    feature_uid: Option<&str>,
) -> anyhow::Result<Option<std::collections::BTreeMap<String, crate::core::git_intent::GitIntentV1>>>
{
    let candidates = works
        .iter()
        .filter(|work| {
            !matches!(
                work.kind,
                crate::core::planning_work::WorkKind::PlanReviewDisposition
                    | crate::core::planning_work::WorkKind::CodeReviewDisposition
                    | crate::core::planning_work::WorkKind::QaFindingDisposition
            ) && crate::core::planning_work::belongs_to_feature(work, feature_id, feature_uid)
        })
        .collect::<Vec<_>>();
    let by_uid = candidates
        .iter()
        .map(|work| (work.uid.as_str(), *work))
        .collect::<std::collections::HashMap<_, _>>();
    let mut intents =
        std::collections::BTreeMap::<String, (usize, crate::core::git_intent::GitIntentV1)>::new();
    for work in candidates {
        let depth = work_depth(work, &by_uid)?;
        for (repository_id, intent) in &work.git_intents {
            match intents.get(repository_id) {
                Some((existing_depth, existing)) if *existing_depth > depth => {}
                Some((existing_depth, existing)) if *existing_depth == depth => {
                    anyhow::ensure!(
                        existing == intent,
                        "Conflicting approved Git intents for repository {repository_id} at the same task level"
                    );
                }
                _ => {
                    intents.insert(repository_id.clone(), (depth, intent.clone()));
                }
            }
        }
    }
    if intents.is_empty() {
        return Ok(None);
    }
    // The workflow stores a feature-level effective map. Child lineage stays
    // on each local Work approval and is not carried into that feature scope.
    let projected = intents
        .into_iter()
        .map(|(repository_id, (_, intent))| {
            let intent = crate::core::git_intent::GitIntentV1::approve(
                repository_id.clone(),
                intent.source_ref,
                intent.source_sha,
                intent.destination_ref,
                intent.publication_mode,
                None,
            )?;
            Ok((repository_id, intent))
        })
        .collect::<anyhow::Result<std::collections::BTreeMap<_, _>>>()?;
    Ok(Some(projected))
}

fn work_depth(
    work: &crate::core::planning_work::Work,
    by_uid: &std::collections::HashMap<&str, &crate::core::planning_work::Work>,
) -> anyhow::Result<usize> {
    let mut depth = 0;
    let mut parent = work.parent_uid.as_deref();
    let mut visited = std::collections::HashSet::new();
    while let Some(uid) = parent {
        anyhow::ensure!(
            visited.insert(uid),
            "Planning task parent cycle prevents Git intent selection"
        );
        let Some(parent_work) = by_uid.get(uid) else {
            break;
        };
        depth += 1;
        parent = parent_work.parent_uid.as_deref();
    }
    Ok(depth)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn feature_intents_require_feature_identity_and_reject_repository_conflicts() {
        let mut work = crate::core::planning_work::Work::new(
            "feature:F38".into(),
            "Export feature".into(),
            "Export project data".into(),
            String::new(),
        );
        work.feature_id = Some("F38".into());
        work.feature_uid = Some("feature-uid".into());
        let intent = crate::core::git_intent::GitIntentV1::approve(
            "root".into(),
            "main".into(),
            "a".repeat(40),
            "main".into(),
            crate::core::git_intent::PublicationModeV1::HoldForReview,
            None,
        )
        .unwrap();
        work.git_intents.insert("root".into(), intent.clone());
        work.git_intent = Some(intent);
        work.source_branch = Some("main".into());
        work.destination_branch = Some("main".into());
        assert!(
            feature_git_intents(&[work.clone()], "F38", None)
                .unwrap()
                .is_none()
        );
        assert!(
            feature_git_intents(&[work.clone()], "F38", Some("feature-uid"))
                .unwrap()
                .is_some()
        );
        let mut conflicting = work.clone();
        let different = crate::core::git_intent::GitIntentV1::approve(
            "root".into(),
            "develop".into(),
            "b".repeat(40),
            "main".into(),
            crate::core::git_intent::PublicationModeV1::HoldForReview,
            None,
        )
        .unwrap();
        conflicting.git_intents.insert("root".into(), different);
        assert!(feature_git_intents(&[work, conflicting], "F38", Some("feature-uid")).is_err());
    }

    #[test]
    fn child_repository_override_keeps_unmodified_parent_intents() {
        let mut parent = crate::core::planning_work::Work::new(
            "feature:F39".into(),
            "Multi-repository feature".into(),
            "Use two repositories".into(),
            String::new(),
        );
        parent.feature_id = Some("F39".into());
        parent.feature_uid = Some("feature-uid".into());
        let root_intent =
            crate::test_support::approved_git_intent("root", "develop", &"a".repeat(40), "develop");
        let api_intent =
            crate::test_support::approved_git_intent("api", "release/2.0", &"b".repeat(40), "main");
        parent.git_intents = std::collections::BTreeMap::from([
            ("root".into(), root_intent.clone()),
            ("api".into(), api_intent.clone()),
        ]);
        let mut child = crate::core::planning_work::Work::new(
            "feature-child".into(),
            "Override API target".into(),
            "Use a release branch".into(),
            String::new(),
        );
        child.parent_uid = Some(parent.uid.clone());
        child.feature_uid = Some("feature-uid".into());
        let inherited_root = crate::core::git_intent::GitIntentV1::approve(
            "root".into(),
            root_intent.source_ref.clone(),
            root_intent.source_sha.clone(),
            root_intent.destination_ref.clone(),
            root_intent.publication_mode,
            Some(parent.uid.clone()),
        )
        .unwrap();
        let override_intent =
            crate::test_support::approved_git_intent("api", "release/2.1", &"c".repeat(40), "main");
        child
            .git_intents
            .insert("api".into(), override_intent.clone());
        child.git_intents.insert("root".into(), inherited_root);

        let effective = feature_git_intents(&[parent, child], "F39", Some("feature-uid"))
            .unwrap()
            .unwrap();
        assert_eq!(effective.get("root"), Some(&root_intent));
        assert_eq!(effective.get("api"), Some(&override_intent));
        assert!(effective["root"].inherited_from_task_uid.is_none());
    }
}
