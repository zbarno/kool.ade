#[cfg(test)]
use super::*;
#[cfg(test)]
use crate::{app::session::Project, core::published_specs::PublishedSpecReader};

#[cfg(test)]
pub(crate) fn simulate_merged_feature(
    project: &mut Project,
    feature_uid: &str,
    snapshot_hash: &str,
) -> anyhow::Result<()> {
    validate_identity(feature_uid, snapshot_hash)?;
    let (_, pending_path) = paths(feature_uid, snapshot_hash);
    let pending: PendingFeaturePublication =
        serde_json::from_slice(&project.state.planning_store.read(&pending_path)?)?;
    let pin = project
        .state
        .published_spec_pin
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("test feature has no pinned documentation baseline"))?;
    let reader = PublishedSpecReader::new(
        &project.state.repo_root,
        pin.repository_id.clone(),
        pin.reference.clone(),
    )?;
    anyhow::ensure!(
        pending.feature_uid == feature_uid && pending.feature_snapshot_sha256 == snapshot_hash,
        "test merge identity does not match the pending feature"
    );
    let markdown_change = pending
        .changes
        .iter()
        .find(|change| change.path == document_path(feature_uid))
        .ok_or_else(|| anyhow::anyhow!("test feature document is missing"))?;
    let (_, markdown) = project
        .state
        .active_features
        .iter()
        .find(|(id, body)| id == &pending.feature_id && body == &markdown_change.content)
        .ok_or_else(|| anyhow::anyhow!("test feature changed before simulated merge"))?;
    validate_pending_changes(
        &pending,
        &pending.feature_id,
        feature_uid,
        snapshot_hash,
        markdown,
        &reader,
        pin,
    )?;
    let snapshot = reviewed_snapshot(project, &pending.feature_id, feature_uid, markdown)?;
    anyhow::ensure!(
        snapshot.content_hash == snapshot_hash,
        "test snapshot identity changed"
    );
    for change in &pending.changes {
        let path = project.state.repo_root.join(&change.path);
        std::fs::create_dir_all(path.parent().expect("publication path parent"))?;
        std::fs::write(path, &change.content)?;
    }
    let mut add = std::process::Command::new("git");
    add.arg("-C")
        .arg(&project.state.repo_root)
        .args(["add", "--"]);
    for change in &pending.changes {
        add.arg(&change.path);
    }
    anyhow::ensure!(
        add.status()?.success(),
        "could not stage simulated feature merge"
    );
    let commit = std::process::Command::new("git")
        .arg("-C")
        .arg(&project.state.repo_root)
        .args([
            "-c",
            "user.name=Kool.ad/e Test",
            "-c",
            "user.email=koolade-test@example.invalid",
            "commit",
            "-m",
            "Merge reviewed feature publication fixture",
        ])
        .status()?;
    anyhow::ensure!(commit.success(), "could not commit simulated feature merge");
    crate::test_support::set_test_origin_default(&project.state.repo_root);
    let pin = PublishedSpecReader::for_primary_repository(&project.state.repo_root)?.pin()?;
    project.state.published_spec_pin = Some(pin.clone());
    project.state.planning_store.published_spec_pin = Some(pin);
    project.state.planning_store.remove(&pending_path)?;
    // The simulated merge makes the accepted proposal the published source.
    // Clear the private draft pointer so current_document resolves the new pin
    // instead of rejecting the now-stale proposal as based on the old SHA.
    project
        .state
        .planning_store
        .remove("state/spec_drafts/current.json")?;
    project.state.baseline_planning_revision = project.state.planning_store.revision()?;
    Ok(())
}
