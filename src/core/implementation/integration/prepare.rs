use super::super::*;
use crate::core::implementation::repository_cache::RepositoryCache;
mod repository;

pub(super) fn prepare_integration(
    repo: &Path,
    dir: &Path,
    state: &Implementation,
    integration_base: &str,
    harness: &dyn AiHarness,
    runner: &Runner,
    policy: &ExecutionPolicy<'_>,
) -> anyhow::Result<Implementation> {
    anyhow::ensure!(
        state.task_repository_kind == TaskRepositoryKind::Clone,
        "Legacy tasks must migrate before integration"
    );
    let commit_identity =
        crate::core::implementation::repository_cache::read_task_git_identity(dir)?;
    let integration_dir = dir.join(format!("integration-{integration_base}"));
    fs::create_dir_all(&integration_dir)?;
    crate::core::implementation::repository_cache::save_task_git_identity(
        &integration_dir,
        &commit_identity,
    )?;
    let task_key = task_repository::allocation_key(state);
    let mut integration: Implementation = if integration_dir.join("state.json").exists() {
        read_state_file(&integration_dir.join("state.json"))?
    } else {
        let mut record = state.clone();
        record.branch = format!(
            "koolade/integration/{}/{}",
            task_key,
            &integration_base[..12]
        );
        record.task_repository = state.task_repository.with_file_name(format!(
            "{}-integration-{}",
            task_key,
            &integration_base[..12]
        ));
        if !record.task_repositories.contains(&record.task_repository) {
            record
                .task_repositories
                .push(record.task_repository.clone());
        }
        record.base_commit = integration_base.to_owned();
        record.task_repository_ready = false;
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
    repository::ensure_integration_repository(
        &integration_dir,
        &mut integration,
        integration_base,
        &commit_identity,
        runner,
    )?;
    if integration.verified_head.is_none() {
        let task_head = state
            .verified_head
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Task is not verified"))?;
        let cache = RepositoryCache::from_saved_state(&integration, runner)?;
        let source_ref = RepositoryCache::task_commit_ref(&task_key, task_head);
        let destination_ref = format!("refs/koolade-task-input/{}/{}", task_key, task_head);
        let cache_path = cache
            .path
            .to_str()
            .ok_or_else(|| anyhow::anyhow!("Non-UTF8 repository cache path"))?;
        let refspec = format!("+{source_ref}:{destination_ref}");
        runner.git(
            &integration.task_repository,
            &[
                "fetch",
                "--no-tags",
                "--no-write-fetch-head",
                cache_path,
                &refspec,
            ],
        )?;
        anyhow::ensure!(
            runner.git(
                &integration.task_repository,
                &["rev-parse", &format!("{destination_ref}^{{commit}}")],
            )? == *task_head,
            "Integration clone did not receive the verified task commit"
        );
        let merge_source = destination_ref;
        let unmerged = runner.git(
            &integration.task_repository,
            &["diff", "--name-only", "--diff-filter=U"],
        )?;
        let mut check_error = None;
        if !unmerged.is_empty() {
            check_error =
                Some("Resolve the pending squash-merge conflicts before verification".into());
        } else if !runner
            .git(&integration.task_repository, &["status", "--porcelain"])?
            .is_empty()
        {
            check_error = Some(
                "Integration clone contains saved edits from an earlier attempt; review and verify them before committing".into(),
            );
        } else if let Err(error) = runner.git(
            &integration.task_repository,
            &["merge", "--squash", &merge_source],
        ) {
            let unmerged = unresolved_paths(runner, &integration.task_repository)?;
            anyhow::ensure!(!unmerged.is_empty(), "Cannot integrate task: {error}");
            integration.detail.push_str(&format!(
                "\nMerge conflicts remain in {}. Both task and destination clones are preserved.",
                super::change_set::display_paths(&unmerged)
            ));
        }
        let report: Report = serde_json::from_slice(&fs::read(dir.join("verified-report.json"))?)?;
        let mut evidence = Vec::new();
        if check_error.is_none()
            && unresolved_paths(runner, &integration.task_repository)?.is_empty()
        {
            for command in &report.verification {
                runner.update(format!("Verifying integrated task: {command}"));
                let result = runner.verify(&integration.task_repository, command);
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
                    .git(&integration.task_repository, &["diff", "--check"])
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
            let verification_result = verification::prepare_verified(
                repo,
                &integration_dir,
                &mut integration,
                harness,
                runner,
                None,
                policy.accrual.as_ref(),
            );
            let unresolved = unresolved_paths(runner, &integration.task_repository)?;
            if !unresolved.is_empty() {
                return Err(anyhow::Error::new(super::super::status::FailureCause(
                    Failure::new(
                        FailureKind::ChangeConflict,
                        RecoveryDisposition::UserAction,
                        format!(
                            "Integration could not safely reconcile these changed paths: {}. The task clone and integration clone are preserved for review. Previous attempt: {}",
                            super::change_set::display_paths(&unresolved),
                            verification_result
                                .err()
                                .map(|error| format!("{error:#}"))
                                .unwrap_or_else(|| "conflicts remain after verification".into())
                        ),
                    ),
                )));
            }
            verification_result?;
        } else {
            runner.git(&integration.task_repository, &["add", "--all"])?;
            if !runner
                .git(
                    &integration.task_repository,
                    &["diff", "--cached", "--name-only"],
                )?
                .is_empty()
            {
                runner.git(
                    &integration.task_repository,
                    &[
                        "commit",
                        "-m",
                        &format!("Implement {}", title(&state.ticket_text)),
                    ],
                )?;
            }
            integration.verified_head =
                Some(runner.git(&integration.task_repository, &["rev-parse", "HEAD"])?);
            integration.task_repository_commits.insert(
                integration.task_repository.to_string_lossy().into_owned(),
                integration.verified_head.clone().unwrap(),
            );
            save(&integration_dir, &integration)?;
        }
    }
    anyhow::ensure!(
        runner
            .git(&integration.task_repository, &["status", "--porcelain"])?
            .is_empty()
            && integration.verified_head.as_deref()
                == Some(
                    runner
                        .git(&integration.task_repository, &["rev-parse", "HEAD"])?
                        .as_str()
                ),
        "Integrated result changed after verification"
    );
    Ok(integration)
}

fn unresolved_paths(runner: &Runner, repository: &Path) -> anyhow::Result<Vec<PathBuf>> {
    let output = runner.git(
        repository,
        &["diff", "--name-only", "--diff-filter=U", "-z"],
    )?;
    Ok(output
        .split('\0')
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
        .collect())
}
