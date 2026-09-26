use super::*;

mod commit;
mod workspace;

pub(super) fn prepare_verified(
    repo: &Path,
    dir: &Path,
    state: &mut Implementation,
    harness: &dyn AiHarness,
    runner: &Runner,
    user_context: Option<&str>,
) -> anyhow::Result<()> {
    let (clean, head) = workspace::prepare_worktree(repo, state, runner)?;
    let already_verified = clean
        && state.verified_head.as_deref() == Some(head.as_str())
        && (!state.auto_merge || dir.join("verified-report.json").exists());
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
        let specification = state
            .approved_specification
            .clone()
            .or_else(|| {
                Path::new(&state.ticket)
                    .parent()
                    .map(|p| repo.join(p).join("specification.md"))
                    .and_then(|p| fs::read_to_string(p).ok())
            })
            .unwrap_or_default();
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
            anyhow::ensure!(
                common(&state.worktree)?.canonicalize()? == common(repo)?.canonicalize()?,
                "Implementation worktree belongs to a different repository"
            );
            anyhow::ensure!(
                runner.git(&state.worktree, &["symbolic-ref", "--short", "HEAD"])? == state.branch,
                "Implementation changed branches; refusing to continue"
            );
            state.status = ImplementationStatus::Implementing;
            save(dir, state)?;
            runner.update(format!(
                "Implementing {} (attempt {attempt})…",
                state.ticket
            ));
            let status = runner.git(&state.worktree, &["status", "--short"])?;
            let log = runner.git(&state.worktree, &["log", "-5", "--oneline"])?;
            let stamp = chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default();
            let history_evidence = history_preflight_context(
                runner,
                &state.worktree,
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
            let mut prompt = format!(
                "Implement this ticket in the CURRENT working directory, a dedicated Git worktree. This may be a RESUME: inspect git status, existing diffs, commits, untracked files, tests and repository instructions FIRST. Preserve and complete existing work; do not restart, reset, clean, discard or overwrite unrelated changes. Verify prerequisites and dependencies; report blocked if unavailable. Implement only this ticket's scope. Run the required checks and repair failures. Do not change branches, create worktrees, commit, push, create PRs or merge; Packet owns those steps. Do not modify the original checkout.\n\nTICKET PATH: {}\nTICKET CONTENT:\n{}\n\nAPPROVED SPECIFICATION:\n{}\n\nAFFECTED PRODUCT MODULES (FROZEN AT TASK APPROVAL):\n{}\n\nCURRENT STATUS:\n{}\nRECENT COMMITS:\n{}\n\nReturn a complete JSON object with status (complete or blocked), summary, acceptance_criteria (array of objects with criterion copied verbatim from the ticket and concrete evidence), verification (array of runnable POSIX /bin/sh commands; each runs in a NEW shell starting in this worktree, with PACKET_WORKTREE set to its absolute path; no shell variables or cwd changes carry between commands), remaining (array of unresolved work). Complete requires every ticket criterion met, meaningful checks passing, and remaining empty. Use actual commands without placeholder paths. Before changing directories, capture paths or use \"$PACKET_WORKTREE/Cargo.toml\"; $(pwd) after cd refers to the NEW directory. Do not use Bash-only syntax. When testing commands yourself, export PACKET_WORKTREE to this worktree path before invoking /bin/sh. Execute exactly the commands you report using /bin/sh. Assert expected outcomes and preserve command exit failures: capture output to a file, then check it, rather than masking a failed command with a successful pipeline or command substitution. Never claim success from an exit code alone or invent results. Do not include prose outside the JSON.",
                state.ticket,
                state.ticket_text,
                specification,
                state
                    .approved_product_context
                    .as_deref()
                    .unwrap_or("Legacy task: no scoped product snapshot."),
                status,
                log
            );
            prompt.push_str(report::response_contract());
            prompt.insert_str(0, report::feasibility_preflight());
            prompt.push_str(&format!(
                "\n\nMECHANICALLY COLLECTED HISTORY PREFLIGHT (also saved at {}):\n{}\nCompare any ticket-stated exact footprint with these reachable-history facts before editing. Explicitly say whether the expected table describes cumulative feature history or this ticket's changes from its task base.\n",
                history_evidence_path.display(), history_evidence
            ));
            prompt.push_str("Write summary for an operator in at most 400 characters: state the outcome and why work is paused, without test inventories or repeated evidence. Keep detailed proof in acceptance_criteria, verification, and the saved report. Make each remaining entry start with the responsible person or role and a verb; name the artifact and result briefly. Use human_choices for actual alternatives instead of embedding a fixed lettered list in prose. Separate human steps from Packet's follow-up.\n");
            if let Some(input) = user_context.filter(|input| !input.trim().is_empty()) {
                prompt.push_str(&format!("\n\nLATEST SUBMITTED USER RESPONSE FOR THIS TASK:\n{}\nThis is a user-supplied decision or observation, not proof that the ledger was changed or external checks were run. Apply only what it explicitly authorizes; verify any required decision-maker identity, inspect the relevant artifacts, and keep unmet requirements blocked.\n",
                    crate::core::context_build::clip(input, 4000)));
            }
            if let Some(dependencies) = &state.completed_dependency_context {
                prompt.push_str(&format!(
                    "\n\nCOMPLETED DEPENDENCY CONTRACTS:\n{dependencies}"
                ));
            }

            if !prior_failure.is_empty() {
                prompt.push_str(&format!("\n\nPREVIOUS STOP / CORRECTION REQUIRED (prior run, context only):\n{prior_failure}\nThis run has a fresh report, verification, harness, and self-repair budget. Prior attempts do not consume it. Preserve previous work; do not treat previous retry exhaustion as a current blocker. Actual unmet prerequisites and acceptance checks still apply.\n"));
            }
            if !feedback.is_empty() {
                prompt.push_str(&format!("\n\nPREVIOUS STOP / CORRECTION REQUIRED:\n{feedback}\nContinue in this same worktree. Treat this as a correction history: keep earlier fixes and address the newest failure without reintroducing older ones. Inspect and preserve existing work. Correct the report or implementation and rerun affected checks. Copy acceptance criterion text EXACTLY, including any spelling mistakes; do not edit the ticket to satisfy this check. Return the full JSON report, not just the correction. Do not weaken or bypass failing checks. Before repeating recovery, check whether the failure is a fixed contradiction in the frozen base/history; if so, preserve the evidence and report the exact human decision needed instead of repeating machine checks. Report blocked for prerequisites or decisions that require human intervention.\nPrevious response (possibly truncated):\n{previous_response}"));
            }
            let report_path = dir.join(format!("{stamp}-report.json"));
            prompt.push_str(&format!("\n\nRECOVERY REPORT FILE: {}\nAfter verification, atomically write the same complete JSON report to this absolute file (temporary sibling then rename) before your final response. This preserves completion if the CLI loses its final message.\nYou may fix the root cause of encountered failures and add regression coverage in this worktree when necessary. Keep repairs focused, preserve checks, and do not commit them yourself: Packet verifies and commits the task and its recovery fixes together atomically.\n", report_path.display()));
            let request = PlanningRequest { mode: crate::harness::ExecutionMode::Implementation, reasoning_level: "medium".into(), repo_root: state.worktree.clone(), prompt_body: prompt, system_instructions: "You are an implementation agent. Read and follow repository AGENTS.md instructions. Implement, integrate, and verify the whole ticket. Preserve existing work when resuming or correcting a failed report. Return the required JSON report. Report blockers honestly. The application alone manages Git commits, integration, and publication.".into(), timeout: runner.remaining()?, progress_tx: runner.progress.clone(), cancel: runner.cancel.clone() };
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
                            feedback.push_str("\nSELF-REPAIR REQUIRED: repeated harness failures. Diagnose their cause using the saved diagnostics, repair preventable causes in this worktree, and add a regression check. Do not repeat the same failed approach. Write the recovery report file before responding.\n");
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
                Ok(report) => {
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
                        let mut evidence = Vec::new();
                        for command in &report.verification {
                            runner.update(format!("Verifying: {command}"));
                            let result = runner.verify(&state.worktree, command);
                            evidence.push(serde_json::json!({
                                "command": command,
                                "output": result.as_ref().ok().map(|text| crate::error::redact_secrets(text)),
                                "error": result.as_ref().err().map(ToString::to_string).map(|text| crate::error::redact_secrets(&text)),
                            }));
                            crate::artifacts::atomic_write_bytes(
                                &dir.join(format!("{stamp}-verification.json")),
                                &serde_json::to_vec_pretty(&evidence)?,
                            )?;
                            if let Err(error) = result {
                                verification_failure = true;
                                failure = Some(format!(
                                    "Verification command failed: {command}\n{error}"
                                ));
                                break;
                            }
                        }
                        anyhow::ensure!(
                            runner.git(&state.worktree, &["symbolic-ref", "--short", "HEAD"])?
                                == state.branch,
                            "Implementation changed branches; refusing to publish"
                        );
                        if failure.is_none()
                            && let Err(error) = runner.git(&state.worktree, &["diff", "--check"])
                        {
                            verification_failure = true;
                            failure = Some(format!("git diff --check failed: {error}"));
                        }
                        if failure.is_none() {
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
                feedback.push_str("AUTOMATIC BLOCKER RECOVERY REQUIRED: treat the blocked report as a checkpoint, preserve its evidence and completed work, and execute every remaining remediation available from this worktree. Diagnose and repair local tooling, scripts, tests, or implementation defects before reporting blocked again. Do not weaken acceptance criteria or fabricate evidence.\n");
                runner.update(format!(
                    "Recovering reported blocker in the preserved worktree (attempt {attempt})…"
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
                feedback.push_str("\nSELF-REPAIR REQUIRED: ordinary retries are exhausted. Diagnose and fix the root cause in this worktree, add a regression check that reproduces the failure, and rerun the complete verification. Preserve existing task work and checks. Packet will commit the verified repair atomically with this task.\n");
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
