use crate::{core::state::PlannerState, domain::OpenItem};

pub(super) fn apply_config_and_resolve_ownership(
    state: &PlannerState,
    synthesized: Vec<OpenItem>,
    config: crate::artifacts::config_io::PlannerConfig,
) -> anyhow::Result<(PlannerState, bool)> {
    let mut updated = state.clone();
    updated.config = config;
    let stakeholders = updated.config.stakeholders.clone();
    updated.items.extend(
        synthesized
            .into_iter()
            .filter(|item| stakeholders.owner_exists(&item.category)),
    );
    let resolved = crate::core::ownership::resolve_assigned_gaps(
        &mut updated.items,
        &mut updated.resolved_items,
        &stakeholders,
    );
    crate::artifacts::items_io::sort_queue(&mut updated.items);
    updated.baseline_items_md = crate::artifacts::items_io::serialize(&updated.items);
    let git_name = crate::core::gitops::read_config(&updated.repo_root, "user.name");
    let git_email = crate::core::gitops::read_config(&updated.repo_root, "user.email");
    updated.identity = crate::domain::resolve_identity(
        git_name.as_deref(),
        git_email.as_deref(),
        updated.config.user.as_ref(),
    );
    Ok((updated, !resolved.is_empty()))
}
