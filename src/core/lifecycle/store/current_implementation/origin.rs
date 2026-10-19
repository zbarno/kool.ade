use crate::artifacts::planning_store::{PlanningStore, StoreError};

pub(in crate::core::lifecycle::store) fn validate(
    store: &PlanningStore,
    repository: &std::path::Path,
    repository_id: &str,
    provider_url: &str,
    provider_pr_id: u64,
) -> Result<(), StoreError> {
    let manifest = crate::core::project_repos::ProjectManifest::load(store).map_err(|_| {
        StoreError::MalformedState("registered repository mapping is unavailable".into())
    })?;
    let registered = manifest
        .repositories
        .iter()
        .find(|registered| registered.id == repository_id)
        .filter(|registered| !registered.remote.trim().is_empty())
        .ok_or_else(|| {
            StoreError::MalformedState(
                "target repository is not registered with a provider remote".into(),
            )
        })?;
    let mut origin_command = crate::core::planning_sync::validated_git_command(repository)
        .map_err(|_| {
            StoreError::MalformedState("target repository origin is unavailable".into())
        })?;
    let origin = origin_command
        .args(["remote", "get-url", "origin"])
        .output()
        .map_err(|_| {
            StoreError::MalformedState("target repository origin is unavailable".into())
        })?;
    if !origin.status.success() {
        return Err(StoreError::MalformedState(
            "target repository origin is unavailable".into(),
        ));
    }
    let origin_url = String::from_utf8_lossy(&origin.stdout).trim().to_owned();
    crate::core::reconciliation::merge_verification::validate_pr_repository_binding(
        repository_id,
        provider_url,
        provider_pr_id,
        &registered.remote,
        &origin_url,
    )
    .map_err(|_| {
        StoreError::MalformedState(
            "target repository origin does not match the persisted provider PR".into(),
        )
    })
}
