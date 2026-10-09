use super::super::support::*;
use super::super::*;
use super::{auto_verify, external_blocker, scope, whitespace};
use std::path::Path;

pub(super) struct Context<'a> {
    pub repo: &'a Path,
    pub dir: &'a Path,
    pub path: &'a Path,
    pub state: &'a mut Implementation,
    pub harness: &'a dyn AiHarness,
    pub runner: &'a Runner,
    pub user_context: Option<&'a str>,
    pub accrual: Option<&'a crate::core::time_accrual::AgentSpan>,
    pub plan: &'a mut Plan,
}

pub(super) fn run(
    context: Context<'_>,
    mut automatically_verified: bool,
    already_in_base: bool,
) -> anyhow::Result<()> {
    let Context {
        repo,
        dir,
        path,
        state,
        harness,
        runner,
        user_context,
        accrual,
        plan,
    } = context;
    let mut feedback = String::new();
    let mut previous_response = String::new();
    let mut last_failure = String::new();
    for attempt in 1..=MAX_ATTEMPTS {
        runner.remaining()?;
        validate_task_repository(state, runner)?;
        let status = runner.git(&state.task_repository, &["status", "--short"])?;
        let unmerged = unmerged_paths(runner, &state.task_repository)?;
        let diff = runner.git(&state.task_repository, &["diff", "--cc"])?;
        let request = PlanningRequest {
            mode: crate::harness::ExecutionMode::Implementation,
            task_id: state.task_uid.clone().or_else(|| Some(state.ticket.clone())),
            reasoning_level: "medium".into(),
            telemetry_phase: Some("reconciliation".into()),
            repo_root: state.task_repository.clone(),
            runtime_config_source: None,
            prompt_body: prompt::build(
                plan,
                &status,
                &unmerged,
                &diff,
                user_context,
                &feedback,
                &previous_response,
            ),
            system_instructions: "You are reconciling two existing project histories before a separate implementation task begins. Read repository instructions. Preserve intended changes from both sides, resolve only integration conflicts, and do not implement the later task. Modify only paths present in the two pinned histories; do not add build configuration, helper files, or other changes to work around verification failures. Report unavailable tools, feeds, caches, or resources as environment_prerequisite. Do not stage, commit, merge, abort, reset, checkout, push, or create linked worktrees; the supervising application controls Git metadata and will verify and commit the result.".into(),
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
                final_text: auto_verify::report(already_in_base).to_string(),
                envelope: None,
                stderr_tail: String::new(),
            })
        } else {
            harness.execute(&request)
        };
        drop(span);
        scope::ensure_pinned_path_scope(
            repo,
            &state.task_repository,
            runner,
            plan,
            dir,
            state,
            false,
        )?;
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
            return Err(external_blocker::error(&report, &report_path));
        }
        if let Err(error) = super::report::validate_reconciliation_report(&report, CONTRACT) {
            last_failure = format!("Reconciliation report needs correction: {error:#}");
            feedback = last_failure.clone();
            continue;
        }

        scope::stage_pinned_changes(repo, runner, state, plan)?;
        let unresolved = unmerged_paths(runner, &state.task_repository)?;
        if !unresolved.is_empty() {
            last_failure = format!("Unresolved merge paths remain: {}", unresolved.join(", "));
            feedback = last_failure.clone();
            continue;
        }
        let changed = runner.git_nul_records(
            &state.task_repository,
            &[
                "diff",
                "--cached",
                "--name-only",
                "--diff-filter=ACDMRTUXB",
                "-z",
            ],
        )?;
        let mut commands = plan.required_verification.clone();
        let changed = changed
            .iter()
            .map(|path| format!("{path}\0"))
            .collect::<String>();
        for command in required_baseline_checks(&state.task_repository, &changed)? {
            if !commands.contains(&command) {
                commands.push(command);
            }
        }
        for command in &report.verification {
            if !report_check_is_covered(&commands, command) && !commands.contains(command) {
                commands.push(command.clone());
            }
        }
        if let Err(error) = whitespace::check(runner, &state.task_repository, plan) {
            last_failure = format!("Reconciliation introduced whitespace errors: {error:#}");
            feedback = last_failure.clone();
            automatically_verified = false;
            continue;
        }
        let evidence = run_verification(runner, state, dir, stamp, &commands)?;
        if let Some(error) = evidence.error {
            if verification_needs_environment(&error) {
                return Err(external_blocker::verification_error(
                    &error,
                    &dir.join(format!("base-reconciliation-{stamp}-verification.json")),
                ));
            }
            last_failure = error;
            feedback = format!(
                "The combined baseline did not pass its reported verification. Repair the reconciliation and return the full corrected report.\n{}",
                last_failure
            );
            automatically_verified = false;
            continue;
        }
        scope::ensure_pinned_path_scope(
            repo,
            &state.task_repository,
            runner,
            plan,
            dir,
            state,
            false,
        )?;
        validate_task_repository(state, runner)?;
        let merge_head = auto_verify::current_merge_head(runner, &state.task_repository)?;
        let merge_in_progress = merge_head.is_some();
        whitespace::check(runner, &state.task_repository, plan)?;
        let staged = runner.git(&state.task_repository, &["diff", "--cached", "--name-only"])?;
        if merge_in_progress || !staged.is_empty() {
            runner.git(
                &state.task_repository,
                &[
                    "-c",
                    "user.name=Kool.ad/e",
                    "-c",
                    "user.email=koolade@localhost",
                    "commit",
                    "-m",
                    &format!(
                        "Reconcile local and origin/{} before implementation",
                        plan.base
                    ),
                ],
            )?;
        }
        let combined = runner.git(&state.task_repository, &["rev-parse", "HEAD"])?;
        ensure_combines(&state.task_repository, runner, plan, &combined)?;
        anyhow::ensure!(
            generated::clean(runner, state, dir)?,
            "Reconciled task repository is not clean after its verified commit"
        );
        plan.verified_commit = Some(combined);
        plan.verification = commands;
        write_plan(path, plan)?;
        state.base_commit = plan
            .verified_commit
            .as_deref()
            .expect("verified commit was just recorded")
            .into();
        save(dir, state)?;
        cache::import_verified_result(repo, state, &state.base_commit, runner)?;
        if let Some(verified) = plan.verified_commit.as_deref()
            && let Err(error) = cache::store(repo, plan, verified, &plan.verification, runner)
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
                "Automatic reconciliation stopped after {MAX_ATTEMPTS} attempts. Both histories and the isolated task repository are preserved. Latest issue: {last_failure}"
            ),
        ),
    )))
}
