use super::*;
use crate::core::implementation::repository_cache::RepositoryCache;
use std::collections::BTreeSet;

mod commit;
mod prompt;
mod request;
mod requirements;
mod scratch;
mod workspace;
pub(in crate::core::implementation) use scratch::ReportCheckClone;
pub(super) use workspace::prepare_task_workspace;

pub(super) struct VerificationPlan {
    pub(super) commands: Vec<String>,
    pub(super) application_owned: BTreeSet<String>,
}

pub(super) fn plan_commands(
    dir: &Path,
    reported: &[String],
    task_gates: &[String],
    integration_gates: &[String],
) -> anyhow::Result<VerificationPlan> {
    let plan = requirements::commands_to_run(dir, reported, task_gates, integration_gates)?;
    Ok(VerificationPlan {
        commands: plan.commands,
        application_owned: plan.application_owned,
    })
}

mod prepare;
pub(super) use prepare::prepare_verified;
