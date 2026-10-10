use super::*;
mod checks;

pub(in crate::core::implementation) fn prepare_verified(
    repo: &Path,
    dir: &Path,
    state: &mut Implementation,
    harness: &dyn AiHarness,
    runner: &Runner,
    user_context: Option<&str>,
    accrual: Option<&crate::core::time_accrual::AgentSpan>,
) -> anyhow::Result<()> {
    let (clean, head) = workspace::prepare_workspace(repo, dir, state, runner)?;
    let already_verified = requirements::already_verified(dir, state, clean, &head)?;
    if already_verified && state.pr_url.is_some() {
        return Ok(());
    }
    if !already_verified {
        // Report and verification corrections each have a limit of three; all share the original deadline.
        // Prior-run evidence is context, never part of this run's retry accounting.
        let prior_detail = state.detail.clone();
        if !prior_detail.is_empty() {
            crate::artifacts::atomic_write(
                &dir.join(format!(
                    "{}-resume-context.txt",
                    chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
                )),
                &prior_detail,
            )?;
        }
        let prior_failure = resume_failure_context(&prior_detail);
        let mut feedback = String::new();
        state.detail =
            "Starting a fresh attempt budget; previous work and evidence are preserved.".into();
        let mut previous_response = String::new();
        let specification = state.approved_specification.clone().unwrap_or_default();
        let specification = if specification_matches_task(&state.ticket_text, &specification) {
            specification
        } else {
            "The saved feature specification names a different feature. Use the ticket and its approved product context; inspect the planning artifacts before making changes.".into()
        };
        let mut attempt = 0;
        let mut report_corrections = 0;
        let mut verification_corrections = 0;
        let mut harness_failures = 0;
        let mut healing_attempts = 0;
        let report = loop {
            runner.remaining()?;
            attempt += 1;
            task_repository::validate_clone_path(state)?;
            RepositoryCache::verify_task_repository(&state.task_repository, runner)?;
            anyhow::ensure!(
                runner.git(&state.task_repository, &["symbolic-ref", "--short", "HEAD"])?
                    == state.branch,
                "Implementation changed branches; refusing to continue"
            );
            state.status = ImplementationStatus::Implementing;
            save(dir, state)?;
            runner.update(format!(
                "Implementing {} (attempt {attempt})…",
                state.ticket
            ));
            let status = runner.git(&state.task_repository, &["status", "--short"])?;
            let log = runner.git(&state.task_repository, &["log", "-5", "--oneline"])?;
            let stamp = chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default();
            let history_evidence = history_preflight_context(
                runner,
                &state.task_repository,
                &state.base_commit,
                &state.ticket_text,
            )?;
            // This is a snapshot of the immutable base/history contract. A
            // fresh timestamped copy on every correction adds no evidence.
            let history_evidence_path = dir.join("history-preflight.txt");
            if fs::read_to_string(&history_evidence_path).ok().as_deref()
                != Some(history_evidence.as_str())
            {
                crate::artifacts::atomic_write(&history_evidence_path, &history_evidence)?;
            }
            let (prompt, report_path) = prompt::build(prompt::Context {
                state,
                status: &status,
                log: &log,
                specification: &specification,
                history_path: &history_evidence_path,
                history_evidence: &history_evidence,
                user_context,
                prior_failure: &prior_failure,
                feedback: &feedback,
                previous_response: &previous_response,
                dir,
                stamp,
            });
            let request = request::build(state, runner, prompt, attempt, verification_corrections)?;
            // F7 accrual (AD-4): charge exactly the agent process lifetime.
            // The guard settles the workspace interval on every escape route
            // (Ok, Err-return, budget expiry, cancellation); the normal
            // fall-through settles it here so report parsing, verify-command
            // execution, and publication never join the bill.
            let span_guard = accrual.and_then(crate::core::time_accrual::span_begin);
            let outcome = match harness.execute(&request) {
                Ok(outcome) => outcome,
                Err(error) => {
                    runner.remaining()?;
                    let detail = error.detail();
                    crate::artifacts::atomic_write(&dir.join(format!("{stamp}-harness-error.txt")), &detail)
                        .map_err(|write_error| anyhow::anyhow!("Harness failed: {detail}\nCould not save diagnostics at {}: {write_error}. Check available disk space and permissions before resuming.", dir.display()))?;
                    if let Ok(report) = fs::read_to_string(&report_path) {
                        crate::harness::HarnessOutcome {
                            final_text: report,
                            envelope: None,
                            stderr_tail: detail,
                        }
                    } else {
                        harness_failures += 1;
                        feedback
                            .push_str(&format!("\nHarness failure {harness_failures}: {detail}\n"));
                        state.detail = feedback.clone();
                        save(dir, state)?;
                        if harness_failures >= 3 {
                            feedback.push_str("\nSELF-REPAIR REQUIRED: repeated harness failures. Diagnose their cause using the saved diagnostics, repair preventable causes in the task repository, and add a regression check. Do not repeat the same failed approach. Return the complete JSON report in your final response so Kool.ad/e can save it.\n");
                        }
                        anyhow::ensure!(
                            harness_failures <= 5,
                            "Harness recovery exhausted after {harness_failures} failures in this run. Latest failure: {detail}. Full diagnostics are preserved in {}",
                            dir.display()
                        );
                        runner.update(format!("Recovering harness failure {harness_failures}; existing work preserved…"));
                        continue;
                    }
                }
            };
            if let Some(handle) = span_guard {
                handle.close(crate::core::time_accrual::StopReason::Completed);
            }
            runner.remaining()?;
            runner.remaining()?;
            let final_is_report =
                crate::harness::pi_extract::extract_json_object(&outcome.final_text)
                    .is_some_and(|json| parse_report(&json).is_ok());
            let report_text = if final_is_report {
                outcome.final_text.clone()
            } else {
                fs::read_to_string(&report_path)
                    .ok()
                    .filter(|text| !text.trim().is_empty())
                    .unwrap_or_else(|| outcome.final_text.clone())
            };
            if final_is_report && !report_path.exists() {
                crate::artifacts::atomic_write(&report_path, &report_text)?;
            }
            if outcome.final_text != report_text || !final_is_report {
                crate::artifacts::atomic_write(
                    &dir.join(format!("{stamp}-response.txt")),
                    &outcome.final_text,
                )?;
            }
            let parsed = crate::harness::pi_extract::extract_json_object(&report_text)
                .ok_or_else(|| anyhow::anyhow!("No complete JSON implementation report. Return JSON with status, blocker_disposition, summary, acceptance_criteria, verification, and remaining."))
                .and_then(|json| parse_report(&json));
            // A blocked report is a recovery checkpoint, not an immediate
            // terminal state. Feed its evidence and remaining work through the
            // same bounded correction loop used for report and verification
            // failures. The shared deadline and healing limit still prevent an
            // unrecoverable external dependency from looping forever.
            let blocked_report = parsed
                .as_ref()
                .is_ok_and(|report| report.status == ReportStatus::Blocked);
            let mut failure = None;
            let mut verification_failure = false;
            match parsed {
                Err(error) => failure = Some(error.to_string()),
                Ok(mut report) => {
                    if external_blocker(&report) {
                        let detail = external_blocker_detail(&report, &report_path);
                        state.detail = detail.clone();
                        save(dir, state)?;
                        return Err(anyhow::Error::new(status::FailureCause(Failure::new(
                            FailureKind::ExternalPrerequisite,
                            RecoveryDisposition::UserAction,
                            detail,
                        ))));
                    }
                    if let Err(error) = validate_report(&report, &state.ticket_text) {
                        failure = Some(error.to_string());
                    } else {
                        state.status = ImplementationStatus::Verifying;
                        save(dir, state)?;
                        let task_gates =
                            initial_reconciliation::support::required_task_checks_at_commit(
                                &state.task_repository,
                                runner,
                                &state.base_commit,
                            )?;
                        let plan = plan_commands(dir, &report.verification, &task_gates, &[])?;
                        initial_reconciliation::support::generated::quarantine_untrusted_ignored(
                            runner, state, dir,
                        )?;
                        let mut evidence = Vec::new();
                        if let Some(error) =
                            checks::run(runner, state, dir, &plan, stamp, &mut evidence)?
                        {
                            verification_failure = true;
                            failure = Some(error);
                        }
                        anyhow::ensure!(
                            runner.git(
                                &state.task_repository,
                                &["symbolic-ref", "--short", "HEAD"]
                            )? == state.branch,
                            "Implementation changed branches; refusing to publish"
                        );
                        if failure.is_none()
                            && let Err(error) =
                                runner.git(&state.task_repository, &["diff", "--check"])
                        {
                            verification_failure = true;
                            failure = Some(format!("git diff --check failed: {error}"));
                        }
                        if failure.is_none() {
                            report.verification = plan.commands;
                            break report;
                        }
                    }
                }
            }
            let failure = failure.expect("unsuccessful attempt must have a failure");
            let phase = if verification_failure {
                "verification"
            } else {
                "report"
            };
            feedback.push_str(&format!("\nAttempt {attempt} ({phase}): {failure}\n"));
            if blocked_report {
                feedback.push_str("AUTOMATIC BLOCKER RECOVERY REQUIRED: treat the blocked report as a checkpoint, preserve its evidence and completed work, and execute every remaining remediation available from this task repository. Diagnose and repair local tooling, scripts, tests, or implementation defects before reporting blocked again. Do not weaken acceptance criteria or fabricate evidence.\n");
                runner.update(format!(
                    "Recovering reported blocker in the preserved task repository (attempt {attempt})…"
                ));
            }
            state.detail = feedback.clone();
            save(dir, state)?;
            crate::artifacts::atomic_write(&dir.join(format!("{stamp}-correction.txt")), &failure)?;
            runner.remaining()?;
            let corrections = if verification_failure {
                &mut verification_corrections
            } else {
                &mut report_corrections
            };
            *corrections += 1;
            if *corrections > 3 {
                anyhow::ensure!(
                    healing_attempts < 2,
                    "Automatic correction limit and self-repair attempts exhausted for {phase} in this run ({attempt} attempts). Latest failure: {failure}\nFull correction evidence is preserved in {}. Resume implementation starts a fresh attempt budget.",
                    dir.display()
                );
                healing_attempts += 1;
                feedback.push_str("\nSELF-REPAIR REQUIRED: ordinary retries are exhausted. Diagnose and fix the root cause in this task repository, add a regression check that reproduces the failure, and rerun the complete verification. Preserve existing task work and checks. Kool.ad/e will commit the verified repair atomically with this task.\n");
                runner.update(format!(
                    "Diagnosing root cause and self-repairing ({healing_attempts}/2)…"
                ));
            }
            previous_response.clear();
            append_tail(&mut previous_response, &outcome.final_text);
            runner.update(format!(
                "Automatically correcting attempt {attempt}: {feedback}"
            ));
        };
        return commit::finalize_verified(dir, state, runner, report, &head);
    }
    Ok(())
}
