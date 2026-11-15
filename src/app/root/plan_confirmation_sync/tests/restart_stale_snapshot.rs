use super::*;

fn git_head(root: &std::path::Path) -> String {
    let output = std::process::Command::new("git")
        .args(["-C"])
        .arg(root)
        .args(["rev-parse", "HEAD"])
        .output()
        .unwrap();
    assert!(output.status.success());
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

/// Production boundary: reconnect reloads a durable plan-confirmation card,
/// then the plan-sync seam refreshes its immutable snapshot after the source
/// feature and Git HEAD change. Reducer tests cannot prove private-store reload
/// and connection wiring. Uses local Git/private files only; setup is one repo.
#[test]
fn reconnect_refreshes_stale_plan_snapshot_before_an_action_is_reused() {
    let repo = repo("restart-stale-plan-snapshot");
    let feature_uid = feature_uid();
    let old_body = spec_body(&feature_uid);
    let feature_path = repo
        .root
        .join("docs/koolade/features")
        .join(format!("{feature_uid}.md"));
    std::fs::create_dir_all(feature_path.parent().unwrap()).unwrap();
    std::fs::write(&feature_path, &old_body).unwrap();
    git(&repo.root, &["add", feature_path.to_str().unwrap()]);
    git(&repo.root, &["commit", "-qm", "seed synthetic feature"]);

    let home = tempfile::tempdir().unwrap();
    let _home = crate::test_support::KooladeHomeGuard::set(home.path());
    let mut first = crate::app::welcome::attempt_connect(repo.root.to_str().unwrap()).unwrap();
    let mut root = intent_root(&feature_uid);
    root.task_intent_uid = Some(root.uid.clone());
    first.planning_work.push(root);
    let first_effects = sync(&mut first);
    assert_eq!(first_effects.evaluations_wanted.len(), 1);
    let old_payload = the_card(&first).plan_confirmation.as_ref().unwrap();
    let old_hash = old_payload.snapshot_content_hash.clone();
    let old_spec_revision = old_payload.snapshot_spec_revision.clone();
    drop(first);

    let new_body = old_body.replace(
        "A saved view can be reopened.",
        "A saved view can be reopened after a device restart.",
    );
    assert_ne!(new_body, old_body);
    std::fs::write(&feature_path, &new_body).unwrap();
    git(&repo.root, &["add", feature_path.to_str().unwrap()]);
    git(
        &repo.root,
        &["commit", "-qm", "advance the feature snapshot"],
    );
    let advanced_head = git_head(&repo.root);

    let mut restarted = crate::app::welcome::attempt_connect(repo.root.to_str().unwrap()).unwrap();
    restarted.planning_work =
        crate::core::planning_work::load(&restarted.state.planning_store).unwrap();
    assert_eq!(
        the_card(&restarted)
            .plan_confirmation
            .as_ref()
            .unwrap()
            .snapshot_content_hash,
        old_hash,
        "connection reload exposes the stale durable snapshot for reconciliation"
    );
    let effects = sync(&mut restarted);
    let card = the_card(&restarted);
    let payload = card.plan_confirmation.as_ref().unwrap();
    let identity = crate::domain::ArtifactIdentity::from_markdown(&new_body)
        .unwrap()
        .unwrap();
    let snapshot = crate::core::lifecycle::plan_snapshot::from_feature_with_author(
        &restarted.state.planning_store,
        "FEAT-IT",
        &identity,
        &new_body,
        &crate::core::workflow::feature_contract(&new_body),
    )
    .unwrap();
    assert_ne!(payload.snapshot_content_hash, old_hash);
    assert_ne!(payload.snapshot_spec_revision, old_spec_revision);
    assert_eq!(payload.snapshot_content_hash, snapshot.content_hash);
    assert_eq!(payload.snapshot_spec_revision, snapshot.spec_revision);
    assert_eq!(card.status, WorkStatus::NeedsAttention);
    assert_eq!(
        effects.evaluations_wanted.len(),
        1,
        "sync did not claim evaluation: {:?}; pending: {:?}",
        effects.announcements,
        restarted.activity.pending
    );
    assert_eq!(git_head(&repo.root), advanced_head);
    remove_planning_store_for(&repo.root);
}
