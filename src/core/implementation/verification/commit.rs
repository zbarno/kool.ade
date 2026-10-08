use super::*;

pub(super) fn finalize_verified(
    dir: &Path,
    state: &mut Implementation,
    runner: &Runner,
    report: Report,
    head: &str,
) -> anyhow::Result<()> {
    anyhow::ensure!(
        runner.git(&state.task_repository, &["rev-parse", "HEAD"])? == head,
        "Agent changed commit history; refusing a non-atomic task commit"
    );
    initial_reconciliation::support::generated::stage_task(runner, state, dir)?;
    if !runner
        .git(&state.task_repository, &["diff", "--cached", "--name-only"])?
        .is_empty()
    {
        runner.git(
            &state.task_repository,
            &[
                "commit",
                "-m",
                &format!("Implement {}", title(&state.ticket_text)),
            ],
        )?;
    }
    anyhow::ensure!(
        initial_reconciliation::support::generated::clean(runner, state, dir)?,
        "Task repository is not clean after verification and commit; resume to review"
    );
    let changed_paths = runner.git(
        &state.task_repository,
        &[
            "diff",
            "--name-only",
            &format!("{}...HEAD", state.base_commit),
        ],
    )?;
    if changed_paths.is_empty() && !permits_evidence_only_completion(&state.ticket_text) {
        return Err(anyhow::Error::new(status::FailureCause(Failure::new(
            FailureKind::NoImplementationChanges,
            RecoveryDisposition::ExplicitResume,
            "No implementation changes relative to the starting commit; no PR created",
        ))));
    }
    state.verified_head = Some(runner.git(&state.task_repository, &["rev-parse", "HEAD"])?);
    state.task_repository_commits.insert(
        state.task_repository.to_string_lossy().into_owned(),
        state.verified_head.clone().unwrap(),
    );
    state.detail = report.summary.clone();
    crate::artifacts::atomic_write(
        &dir.join("verified-report.json"),
        &serde_json::to_string_pretty(&report)?,
    )?;
    crate::artifacts::atomic_write(&dir.join("pr-body.md"), &pr_body(state, &report))?;
    if changed_paths.is_empty() {
        // An evidence-only ticket completes at its unchanged immutable base.
        state.merged_commit = state.verified_head.clone();
        state.status = ImplementationStatus::Completed;
        save(dir, state)?;
        return Ok(());
    }
    state.status = ImplementationStatus::ReadyToPublish;
    save(dir, state)
}
