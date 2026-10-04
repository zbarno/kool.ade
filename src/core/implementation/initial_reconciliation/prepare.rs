mod auto_verify;

use super::support::*;
use super::*;

pub fn prepare(
    repo: &Path,
    dir: &Path,
    state: &mut Implementation,
    harness: &dyn AiHarness,
    runner: &Runner,
    user_context: Option<&str>,
    accrual: Option<&crate::core::time_accrual::AgentSpan>,
) -> anyhow::Result<()> {
    let path = dir.join(PLAN_FILE);
    if !path.exists() {
        return Ok(());
    }
    let mut plan = read_plan(&path)?;
    if let Some(verified) = plan.verified_commit.as_deref() {
        ensure_combines(repo, runner, &plan, verified)?;
        if state.base_commit != verified {
            state.base_commit = verified.into();
            save(dir, state)?;
        }
        return Ok(());
    }

    let _cache_lock = cache::acquire_lock(repo, &plan, runner)?;
    runner.update("Combining local and shared changes in an isolated worktree…");
    verification::prepare_worktree(repo, state, runner)?;
    validate_worktree(repo, state, runner)?;

    if let Some(cached) = cache::load(repo, &plan, runner)?
        && cache::adopt(repo, state, &plan, &cached, runner)?
    {
        plan.verified_commit = Some(cached.verified_commit.clone());
        plan.verification = cached.verification;
        write_plan(&path, &plan)?;
        state.base_commit = cached.verified_commit;
        save(dir, state)?;
        runner
            .update("Reused the verified shared baseline; starting the requested implementation…");
        return Ok(());
    }

    let head = runner.git(&state.worktree, &["rev-parse", "HEAD"])?;
    let local_in_head = is_ancestor(runner, &state.worktree, &plan.local_commit, &head)?;
    let remote_in_head = is_ancestor(runner, &state.worktree, &plan.remote_commit, &head)?;
    let mut merge_head = current_merge_head(runner, &state.worktree)?;
    let mut merge_in_progress = merge_head.is_some();
    if merge_in_progress {
        anyhow::ensure!(
            head == plan.remote_commit && merge_head.as_deref() == Some(plan.local_commit.as_str()),
            "The saved reconciliation worktree has unexpected merge parents. Its contents are preserved for review."
        );
    }

    if !(local_in_head && remote_in_head) && !merge_in_progress {
        let status = runner.git(&state.worktree, &["status", "--porcelain"])?;
        anyhow::ensure!(
            status.is_empty() && head == plan.remote_commit,
            "The saved reconciliation worktree has unexpected edits or a changed base. Its contents are preserved for review."
        );
        let result = runner.git(
            &state.worktree,
            &[
                "merge",
                "--no-ff",
                "--no-commit",
                "--no-edit",
                &plan.local_commit,
            ],
        );
        let unmerged = unmerged_paths(runner, &state.worktree)?;
        if let Err(error) = result {
            anyhow::ensure!(
                !unmerged.is_empty(),
                "Could not combine local and shared histories: {error}"
            );
        }
        merge_head = current_merge_head(runner, &state.worktree)?;
        merge_in_progress = merge_head.is_some();
        anyhow::ensure!(
            merge_in_progress,
            "Git did not leave a resumable merge in the isolated worktree"
        );
    }

    let mut automatically_verified = auto_verify::clean_disjoint_merge(
        repo,
        &state.worktree,
        runner,
        &plan,
        &head,
        merge_head.as_deref(),
    )?;

    let mut feedback = String::new();
    let mut previous_response = String::new();
    let mut last_failure = String::new();
    for attempt in 1..=MAX_ATTEMPTS {
        runner.remaining()?;
        validate_worktree(repo, state, runner)?;
        let status = runner.git(&state.worktree, &["status", "--short"])?;
        let unmerged = unmerged_paths(runner, &state.worktree)?;
        let diff = runner.git(&state.worktree, &["diff", "--cc"])?;
        let request = PlanningRequest {
            mode: crate::harness::ExecutionMode::Implementation,
            reasoning_level: "medium".into(),
            repo_root: state.worktree.clone(),
            prompt_body: prompt::build(
                &plan,
                &status,
                &unmerged,
                &diff,
                user_context,
                &feedback,
                &previous_response,
            ),
            system_instructions: "You are reconciling two existing project histories before a separate implementation task begins. Read repository instructions. Preserve intended changes from both sides, resolve only integration conflicts, and do not implement the later task. Do not stage, commit, merge, abort, reset, checkout, push, or create worktrees; the supervising application controls Git metadata and will verify and commit the result.".into(),
            timeout: runner.remaining()?,
            progress_tx: runner.progress.clone(),
            cancel: runner.cancel.clone(),
        };
        if automatically_verified {
            runner.update("Checking the clean merge of disjoint branch changes…");
        } else {
            runner.update(format!("Reviewing combined changes (attempt {attempt})…"));
        }
        let span = accrual.and_then(crate::core::time_accrual::span_begin);
        let outcome = if automatically_verified {
            Ok(crate::harness::HarnessOutcome {
                final_text: auto_verify::report().to_string(),
                envelope: None,
                stderr_tail: String::new(),
            })
        } else {
            harness.execute(&request)
        };
        drop(span);
        let response = match outcome {
            Ok(outcome) => outcome.final_text,
            Err(error) => {
                last_failure = error.detail();
                feedback = format!("Reconciliation agent failed: {last_failure}");
                continue;
            }
        };
        previous_response = response.clone();
        let stamp = chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default();
        let report_path = dir.join(format!("base-reconciliation-{stamp}-report.json"));
        crate::artifacts::atomic_write(
            &dir.join(format!("base-reconciliation-{stamp}-response.txt")),
            &response,
        )?;
        let report_json = match crate::harness::pi_extract::extract_json_object(&response) {
            Some(json) => json,
            None => {
                last_failure =
                    "No complete JSON reconciliation report was found in the agent response".into();
                feedback = last_failure.clone();
                continue;
            }
        };
        crate::artifacts::atomic_write(&report_path, &report_json)?;
        let report = match super::report::parse_report(&report_json) {
            Ok(report) => report,
            Err(error) => {
                last_failure = format!("Invalid reconciliation report: {error:#}");
                feedback = last_failure.clone();
                continue;
            }
        };
        if super::report::external_blocker(&report) {
            let detail = super::report::external_blocker_detail(&report, &report_path);
            return Err(user_action(detail));
        }
        if let Err(error) = super::report::validate_report(&report, CONTRACT) {
            last_failure = format!("Reconciliation report needs correction: {error:#}");
            feedback = last_failure.clone();
            continue;
        }

        runner.git(&state.worktree, &["add", "--all"])?;
        let unresolved = unmerged_paths(runner, &state.worktree)?;
        if !unresolved.is_empty() {
            last_failure = format!("Unresolved merge paths remain: {}", unresolved.join(", "));
            feedback = last_failure.clone();
            continue;
        }
        let changed = runner.git(
            &state.worktree,
            &[
                "diff",
                "--cached",
                "--name-only",
                "--diff-filter=ACDMRTUXB",
                "-z",
            ],
        )?;
        let mut commands = plan.required_verification.clone();
        for command in required_baseline_checks(&state.worktree, &changed)? {
            if !commands.contains(&command) {
                commands.push(command);
            }
        }
        for command in &report.verification {
            if !commands.contains(command) {
                commands.push(command.clone());
            }
        }
        let evidence = run_verification(runner, state, dir, stamp, &commands)?;
        if let Some(error) = evidence.error {
            last_failure = error;
            feedback = format!(
                "The combined baseline did not pass its reported verification. Repair the reconciliation and return the full corrected report.\n{}",
                last_failure
            );
            automatically_verified = false;
            continue;
        }
        validate_worktree(repo, state, runner)?;
        merge_head = current_merge_head(runner, &state.worktree)?;
        merge_in_progress = merge_head.is_some();
        runner.git(&state.worktree, &["diff", "--cached", "--check"])?;
        let staged = runner.git(&state.worktree, &["diff", "--cached", "--name-only"])?;
        if merge_in_progress || !staged.is_empty() {
            runner.git(
                &state.worktree,
                &[
                    "-c",
                    "user.name=Kool.ad/e",
                    "-c",
                    "user.email=koolade@localhost",
                    "commit",
                    "-m",
                    &format!(
                        "Reconcile local and origin/{} before implementation",
                        state.base
                    ),
                ],
            )?;
        }
        let combined = runner.git(&state.worktree, &["rev-parse", "HEAD"])?;
        ensure_combines(repo, runner, &plan, &combined)?;
        anyhow::ensure!(
            runner
                .git(&state.worktree, &["status", "--porcelain"])?
                .is_empty(),
            "Reconciled worktree is not clean after its verified commit"
        );
        plan.verified_commit = Some(combined);
        plan.verification = commands;
        write_plan(&path, &plan)?;
        state.base_commit = plan
            .verified_commit
            .as_deref()
            .expect("verified commit was just recorded")
            .into();
        save(dir, state)?;
        if let Some(verified) = plan.verified_commit.as_deref()
            && let Err(error) = cache::store(repo, &plan, verified, &plan.verification, runner)
        {
            runner.remaining()?;
            runner.update(format!(
                "Verified baseline could not be shared with follow-up tasks: {error:#}"
            ));
        }
        runner.update("Combined baseline verified; starting the requested implementation…");
        return Ok(());
    }
    Err(anyhow::Error::new(super::status::FailureCause(
        Failure::new(
            FailureKind::RemoteDiverged,
            RecoveryDisposition::ExplicitResume,
            format!(
                "Automatic reconciliation stopped after {MAX_ATTEMPTS} attempts. Both histories and the isolated worktree are preserved. Latest issue: {last_failure}"
            ),
        ),
    )))
}

fn current_merge_head(runner: &Runner, worktree: &Path) -> anyhow::Result<Option<String>> {
    match runner.git(worktree, &["rev-parse", "--verify", "MERGE_HEAD"]) {
        Ok(commit) => Ok(Some(commit)),
        Err(_) => {
            runner.remaining()?;
            Ok(None)
        }
    }
}
