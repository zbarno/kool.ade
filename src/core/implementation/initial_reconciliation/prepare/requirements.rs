use super::super::*;

pub(super) fn refresh(
    repo: &Path,
    runner: &Runner,
    plan_path: &Path,
    plan: &mut Plan,
) -> anyhow::Result<()> {
    let required = support::required_baseline_checks_for_commits(
        repo,
        runner,
        &plan.common_base,
        &plan.local_commit,
        &plan.remote_commit,
    )?;
    if plan.required_verification != required {
        plan.required_verification = required;
        support::write_plan(plan_path, plan)?;
    }
    Ok(())
}
