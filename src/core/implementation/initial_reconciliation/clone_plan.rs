use super::{
    PLAN_FILE, Plan,
    support::{read_plan, write_plan},
};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::core::implementation) struct CloneRepositoryPlan {
    pub(in crate::core::implementation) repository_id: String,
    pub(in crate::core::implementation) repository_identity: String,
    pub(in crate::core::implementation) source_ref: String,
    pub(in crate::core::implementation) destination_branch: String,
}

#[cfg(test)]
pub(in crate::core::implementation) fn save_plan(
    dir: &Path,
    base: &str,
    local_commit: &str,
    remote_commit: &str,
    common_base: &str,
    required_verification: &[String],
) -> anyhow::Result<()> {
    save_plan_with_repository(
        dir,
        base,
        local_commit,
        remote_commit,
        common_base,
        required_verification,
        None,
    )
}

pub(in crate::core::implementation) fn save_clone_plan(
    dir: &Path,
    base: &str,
    local_commit: &str,
    remote_commit: &str,
    common_base: &str,
    required_verification: &[String],
    clone_repository: CloneRepositoryPlan,
) -> anyhow::Result<()> {
    save_plan_with_repository(
        dir,
        base,
        local_commit,
        remote_commit,
        common_base,
        required_verification,
        Some(clone_repository),
    )
}

pub(in crate::core::implementation) fn attach_clone_repository(
    dir: &Path,
    clone_repository: CloneRepositoryPlan,
) -> anyhow::Result<()> {
    let path = dir.join(PLAN_FILE);
    let mut plan = read_plan(&path)?;
    if let Some(existing) = &plan.clone_repository {
        anyhow::ensure!(
            existing == &clone_repository,
            "Saved reconciliation clone identity changed; the snapshot is preserved"
        );
        return Ok(());
    }
    plan.schema_version = 3;
    plan.clone_repository = Some(clone_repository);
    write_plan(&path, &plan)
}

fn save_plan_with_repository(
    dir: &Path,
    base: &str,
    local_commit: &str,
    remote_commit: &str,
    common_base: &str,
    required_verification: &[String],
    clone_repository: Option<CloneRepositoryPlan>,
) -> anyhow::Result<()> {
    let plan = Plan {
        schema_version: 3,
        base: base.into(),
        local_commit: local_commit.into(),
        remote_commit: remote_commit.into(),
        common_base: common_base.into(),
        required_verification: required_verification.to_vec(),
        verified_commit: None,
        verification: Vec::new(),
        clone_repository,
    };
    let path = dir.join(PLAN_FILE);
    if path.exists() {
        let existing = read_plan(&path)?;
        anyhow::ensure!(
            existing.base == plan.base
                && existing.local_commit == plan.local_commit
                && existing.remote_commit == plan.remote_commit
                && existing.common_base == plan.common_base
                && existing.clone_repository == plan.clone_repository,
            "A different starting-point reconciliation is already saved for this task"
        );
        return Ok(());
    }
    write_plan(&path, &plan)
}
