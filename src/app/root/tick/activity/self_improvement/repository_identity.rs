use crate::{
    app::session::Project,
    core::{implementation::Implementation, self_improvement::SelfImprovementRunV1},
};

pub(super) fn implementation_matches(
    project: &Project,
    implementation: &Implementation,
    run: &SelfImprovementRunV1,
) -> bool {
    let workspace_uid = project
        .state
        .planning_store
        .project_id
        .hyphenated()
        .to_string();
    implementation
        .repository_id
        .as_deref()
        .is_some_and(|repository_id| {
            project
                .state
                .repositories
                .repositories
                .iter()
                .any(|repository| {
                    repository.id == repository_id
                        && crate::core::project_repos::stable_uid(
                            &workspace_uid,
                            "repository",
                            &repository.id,
                        ) == run.target_repository_uid
                        && (repository.remote.is_empty()
                            || crate::core::self_improvement::canonical_product_remote(
                                &repository.remote,
                            ) == Some(run.canonical_remote.as_str()))
                })
        })
}
