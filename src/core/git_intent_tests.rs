use super::*;

fn approved() -> GitIntentV1 {
    GitIntentV1::approve(
        "api".into(),
        "release/2.0".into(),
        "a".repeat(40),
        "main".into(),
        PublicationModeV1::CreatePullRequest,
        None,
    )
    .unwrap()
}

#[test]
fn approved_revision_binds_repo_refs_sha_mode_and_inheritance() {
    let intent = approved();
    intent.validate().unwrap();
    let mutations: [fn(&mut GitIntentV1); 5] = [
        |intent: &mut GitIntentV1| intent.repository_id = "root".into(),
        |intent: &mut GitIntentV1| intent.source_ref = "main".into(),
        |intent: &mut GitIntentV1| intent.source_sha = "b".repeat(40),
        |intent: &mut GitIntentV1| intent.destination_ref = "develop".into(),
        |intent: &mut GitIntentV1| intent.publication_mode = PublicationModeV1::HoldForReview,
    ];
    for mutate in mutations {
        let mut changed = intent.clone();
        mutate(&mut changed);
        assert!(changed.validate().is_err());
    }
}

#[test]
fn intent_rejects_unvalidated_refs_shas_and_repository_ids() {
    for (repository, source, sha, destination) in [
        ("../api", "main", "a".repeat(40), "main"),
        ("api", "bad branch", "a".repeat(40), "main"),
        ("api", "path//branch", "a".repeat(40), "main"),
        ("api", "path/.hidden", "a".repeat(40), "main"),
        ("api", "release.lock", "a".repeat(40), "main"),
        ("api", "main", "a".repeat(40), "@"),
        ("api", "main", "abc".into(), "main"),
        ("api", "main", "a".repeat(40), "bad branch"),
    ] {
        assert!(
            GitIntentV1::approve(
                repository.into(),
                source.into(),
                sha,
                destination.into(),
                PublicationModeV1::HoldForReview,
                None,
            )
            .is_err()
        );
    }
}
