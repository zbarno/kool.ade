use super::*;

pub(super) fn finalize_verified(
    dir: &Path,
    state: &mut Implementation,
    runner: &Runner,
    report: Report,
    head: &str,
) -> anyhow::Result<()> {
    anyhow::ensure!(
        runner.git(&state.worktree, &["rev-parse", "HEAD"])? == head,
        "Agent changed commit history; refusing a non-atomic task commit"
    );
    runner.git(&state.worktree, &["add", "--all"])?;
    if !runner
        .git(&state.worktree, &["diff", "--cached", "--name-only"])?
        .is_empty()
    {
        runner.git(
            &state.worktree,
            &[
                "commit",
                "-m",
                &format!("Implement {}", title(&state.ticket_text)),
            ],
        )?;
    }
    anyhow::ensure!(
        runner
            .git(&state.worktree, &["status", "--porcelain"])?
            .is_empty(),
        "Worktree is not clean after verification and commit; resume to review"
    );
    let changed_paths = runner.git(
        &state.worktree,
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
    state.verified_head = Some(runner.git(&state.worktree, &["rev-parse", "HEAD"])?);
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
