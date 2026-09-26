use super::*;

pub(super) fn auto_publish(
    repo: &Path,
    dir: &Path,
    state: &mut Implementation,
    harness: &dyn AiHarness,
    runner: &Runner,
    auto_publish_gate: Option<&AtomicBool>,
    require_independent_checks: bool,
) -> anyhow::Result<()> {
    let publish_lock = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(common(repo)?.join("packet-auto-publish.lock"))?;
    state.status = ImplementationStatus::WaitingToMerge;
    save(dir, state)?;
    runner.update("Verified; waiting for the project integration lock…");
    while publish_lock.try_lock().is_err() {
        runner.remaining()?;
        if !publication::auto_publish_enabled(auto_publish_gate) {
            return publication::hold_for_review(dir, state);
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    state.base = publication::default_branch(repo, runner)?;
    let mut last_error = String::new();
    for _ in 0..3 {
        runner.remaining()?;
        if !publication::auto_publish_enabled(auto_publish_gate) {
            return publication::hold_for_review(dir, state);
        }
        runner.update(format!(
            "Integrating and verifying against latest origin/{}…",
            state.base
        ));
        let remote_ref = format!("refs/packet-auto-bases/{}", key(&state.ticket));
        if let Err(error) = runner.git(
            repo,
            &[
                "fetch",
                "--no-tags",
                "--no-write-fetch-head",
                "origin",
                &format!("+refs/heads/{}:{remote_ref}", state.base),
            ],
        ) {
            last_error = error.to_string();
            continue;
        }
        let remote = runner.git(repo, &["rev-parse", &remote_ref])?;
        // Recover a crash or lost push response without another merge or agent call.
        if let Some(commit) = state.merged_commit.clone()
            && runner
                .git(repo, &["merge-base", "--is-ancestor", &commit, &remote])
                .is_ok()
        {
            if require_independent_checks
                && !checks_gate::independent_check_passed(state.independent_check.as_ref(), &commit)
            {
                checks_gate::wait_for_independent_checks(
                    repo,
                    dir,
                    state,
                    &state.worktree.clone(),
                    &commit,
                    runner,
                    auto_publish_gate,
                )?;
            }
            return publication::finish_auto_publish(repo, dir, state, runner);
        }
        let local = runner.git(repo, &["rev-parse", "HEAD"])?;
        // Integrate on fetched remote truth when histories diverge. The task's
        // verified branch is squash-merged below, with conflicts repaired and
        // verification rerun in isolation. Never rewrite the user's checkout.
        let integration_base = if runner
            .git(repo, &["merge-base", "--is-ancestor", &remote, &local])
            .is_ok()
        {
            local
        } else {
            remote.clone()
        };
        let integration_dir = dir.join(format!("integration-{integration_base}"));
        fs::create_dir_all(&integration_dir)?;
        let mut integration: Implementation = if integration_dir.join("state.json").exists() {
            read_state_file(&integration_dir.join("state.json"))?
        } else {
            let mut record = state.clone();
            record.branch = format!(
                "packet/integration/{}/{}",
                key(&state.ticket),
                &integration_base[..12]
            );
            record.worktree = state.worktree.with_file_name(format!(
                "{}-integration-{}",
                key(&state.ticket),
                &integration_base[..12]
            ));
            record.base_commit = integration_base.clone();
            record.verified_head = None;
            record.merged_commit = None;
            record.auto_merge = false;
            record.pr_url = None;
            record.pr_state = None;
            record.status = ImplementationStatus::Preparing;
            record.detail = "Integrate the verified task with the latest default branch. Resolve any merge conflicts preserving both intended behaviors. Repair failures and verify the integrated result.".into();
            save(&integration_dir, &record)?;
            record
        };
        if !integration.worktree.exists() {
            let path = integration
                .worktree
                .to_str()
                .ok_or_else(|| anyhow::anyhow!("Invalid integration path"))?;
            if runner
                .git(
                    repo,
                    &[
                        "show-ref",
                        "--verify",
                        &format!("refs/heads/{}", integration.branch),
                    ],
                )
                .is_ok()
            {
                runner.git(repo, &["worktree", "add", path, &integration.branch])?;
            } else {
                runner.git(
                    repo,
                    &[
                        "worktree",
                        "add",
                        "-b",
                        &integration.branch,
                        path,
                        &integration_base,
                    ],
                )?;
            }
        }
        anyhow::ensure!(
            common(&integration.worktree)?.canonicalize()? == common(repo)?.canonicalize()?
                && runner.git(&integration.worktree, &["symbolic-ref", "--short", "HEAD"])?
                    == integration.branch,
            "Integration worktree identity changed; refusing to modify it"
        );
        if integration.verified_head.is_none() {
            let task_head = state
                .verified_head
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("Task is not verified"))?;
            let unmerged = runner.git(
                &integration.worktree,
                &["diff", "--name-only", "--diff-filter=U"],
            )?;
            if unmerged.is_empty()
                && runner
                    .git(&integration.worktree, &["status", "--porcelain"])?
                    .is_empty()
                && let Err(error) =
                    runner.git(&integration.worktree, &["merge", "--squash", task_head])
            {
                anyhow::ensure!(
                    !runner
                        .git(
                            &integration.worktree,
                            &["diff", "--name-only", "--diff-filter=U"]
                        )?
                        .is_empty(),
                    "Cannot integrate task: {error}"
                );
                integration
                    .detail
                    .push_str(&format!("\nMerge failed: {error}"));
            }
            let report: Report =
                serde_json::from_slice(&fs::read(dir.join("verified-report.json"))?)?;
            let mut check_error = None;
            let mut evidence = Vec::new();
            if runner
                .git(
                    &integration.worktree,
                    &["diff", "--name-only", "--diff-filter=U"],
                )?
                .is_empty()
            {
                for command in &report.verification {
                    runner.update(format!("Verifying integrated task: {command}"));
                    let result = runner.verify(&integration.worktree, command);
                    evidence.push(serde_json::json!({
                        "command": command,
                        "output": result.as_ref().ok().map(|text| crate::error::redact_secrets(text)),
                        "error": result.as_ref().err().map(ToString::to_string).map(|text| crate::error::redact_secrets(&text)),
                    }));
                    if let Err(error) = result {
                        check_error = Some(error.to_string());
                        break;
                    }
                }
                if check_error.is_none() {
                    check_error = runner
                        .git(&integration.worktree, &["diff", "--check"])
                        .err()
                        .map(|e| e.to_string());
                }
            } else {
                check_error =
                    Some("Resolve the pending squash-merge conflicts before verification".into());
            }
            crate::artifacts::atomic_write_bytes(
                &integration_dir.join(format!(
                    "{}-verification.json",
                    chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
                )),
                &serde_json::to_vec_pretty(&evidence)?,
            )?;
            if let Some(error) = check_error {
                integration
                    .detail
                    .push_str(&format!("\nIntegration verification failure: {error}"));
                save(&integration_dir, &integration)?;
                verification::prepare_verified(
                    repo,
                    &integration_dir,
                    &mut integration,
                    harness,
                    runner,
                    None,
                )?;
            } else {
                runner.git(&integration.worktree, &["add", "--all"])?;
                if !runner
                    .git(&integration.worktree, &["diff", "--cached", "--name-only"])?
                    .is_empty()
                {
                    runner.git(
                        &integration.worktree,
                        &[
                            "commit",
                            "-m",
                            &format!("Implement {}", title(&state.ticket_text)),
                        ],
                    )?;
                }
                integration.verified_head =
                    Some(runner.git(&integration.worktree, &["rev-parse", "HEAD"])?);
                save(&integration_dir, &integration)?;
            }
        }
        anyhow::ensure!(
            runner
                .git(&integration.worktree, &["status", "--porcelain"])?
                .is_empty()
                && integration.verified_head.as_deref()
                    == Some(
                        runner
                            .git(&integration.worktree, &["rev-parse", "HEAD"])?
                            .as_str()
                    ),
            "Integrated result changed after verification"
        );
        state.merged_commit = integration.verified_head.clone();
        state.status = ImplementationStatus::Publishing;
        save(dir, state)?;
        if !publication::auto_publish_enabled(auto_publish_gate) {
            return publication::hold_for_review(dir, state);
        }
        if require_independent_checks {
            let verified_commit = state
                .merged_commit
                .clone()
                .ok_or_else(|| anyhow::anyhow!("Integrated commit is missing"))?;
            checks_gate::wait_for_independent_checks(
                repo,
                dir,
                state,
                &integration.worktree,
                &verified_commit,
                runner,
                auto_publish_gate,
            )?;
        }
        let target = format!(
            "{}:refs/heads/{}",
            state.merged_commit.as_ref().unwrap(),
            state.base
        );
        match runner.git(&integration.worktree, &["push", "origin", &target]) {
            Ok(_) => {
                return publication::finish_auto_publish(repo, dir, state, runner);
            }
            Err(error) => {
                last_error = error.to_string();
                runner.update(
                    "Default branch changed or push failed; fetching and retrying integration…",
                );
            }
        }
    }
    anyhow::bail!(
        "Auto publication could not complete after retries; verified work is preserved: {last_error}"
    )
}
