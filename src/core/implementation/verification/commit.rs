use super::*;
use sha2::Digest as _;

const MAX_DIFF_BYTES: u64 = 48 * 1024;

pub(super) fn finalize_verified(
    planning_store: Option<&crate::artifacts::planning_store::PlanningStore>,
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
    if let Some(planning_store) = planning_store
        && let Some(task_uid) = state.task_uid.as_deref()
    {
        let diff_path = dir.join("code-review-subject.patch");
        runner.git_to_file_bounded(
            &state.task_repository,
            &["diff", "--binary", &format!("{}...HEAD", state.base_commit)],
            &diff_path,
            MAX_DIFF_BYTES,
        )?;
        let diff_sha256 = format!("{:x}", sha2::Sha256::digest(std::fs::read(diff_path)?));
        super::super::lifecycle::record_implementation_finished(
            planning_store,
            task_uid,
            &diff_sha256,
        )?;
    }
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
