use super::Project;

pub(super) fn verify_canonical_target(
    project: &Project,
    run: &crate::core::self_improvement::SelfImprovementRunV1,
) -> Result<(), String> {
    let manifest = crate::core::project_repos::ProjectManifest::load(&project.state.planning_store)
        .map_err(|error| format!("cannot verify self-improvement target repository: {error}"))?;
    let workspace_uid = project
        .state
        .planning_store
        .project_id
        .hyphenated()
        .to_string();
    let repository = manifest.repositories.iter().find(|repository| {
        crate::core::project_repos::stable_uid(&workspace_uid, "repository", &repository.id)
            == run.target_repository_uid
    });
    let Some(repository) = repository else {
        return Err(
            "self-improvement target repository is missing from the current manifest".into(),
        );
    };
    if crate::core::self_improvement::canonical_product_remote(&repository.remote)
        != Some(run.canonical_remote.as_str())
    {
        return Err(
            "self-improvement target repository no longer resolves to the approved canonical product remote".into(),
        );
    }
    Ok(())
}
