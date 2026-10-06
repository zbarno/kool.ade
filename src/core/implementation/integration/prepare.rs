use super::super::*;

pub(super) fn prepare_integration(
    repo: &Path,
    dir: &Path,
    state: &Implementation,
    integration_base: &str,
    harness: &dyn AiHarness,
    runner: &Runner,
    policy: &ExecutionPolicy<'_>,
) -> anyhow::Result<Implementation> {
    let integration_dir = dir.join(format!("integration-{integration_base}"));
    fs::create_dir_all(&integration_dir)?;
    let mut integration: Implementation = if integration_dir.join("state.json").exists() {
        read_state_file(&integration_dir.join("state.json"))?
    } else {
        let mut record = state.clone();
        record.branch = format!(
            "koolade/integration/{}/{}",
            key(&state.ticket),
            &integration_base[..12]
        );
        record.worktree = state.worktree.with_file_name(format!(
            "{}-integration-{}",
            key(&state.ticket),
            &integration_base[..12]
        ));
        record.base_commit = integration_base.to_owned();
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
                    integration_base,
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
            && let Err(error) = runner.git(&integration.worktree, &["merge", "--squash", task_head])
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
        let report: Report = serde_json::from_slice(&fs::read(dir.join("verified-report.json"))?)?;
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
                policy.accrual.as_ref(),
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
    Ok(integration)
}
