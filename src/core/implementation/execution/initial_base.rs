use super::*;

pub(super) fn prepare(
    repo: &Path,
    dir: &Path,
    ticket: &str,
    metadata: &Option<crate::artifacts::task_docs::TaskMetadata>,
    publication_mode: PublicationMode,
    runner: &Runner,
) -> anyhow::Result<(String, String, Option<String>)> {
    // A reconciliation plan is durable before task state. If the process
    // stopped between those writes, resume its immutable snapshots instead
    // of fetching and constructing a different task base.
    let explicit_source = metadata
        .as_ref()
        .and_then(|metadata| metadata.source_branch.as_deref());
    let source = explicit_source.map(str::to_owned).unwrap_or(
        if publication_mode == PublicationMode::AutoPublish {
            publication::default_branch(repo, runner)?
        } else {
            runner.git(repo, &["symbolic-ref", "--short", "HEAD"])?
        },
    );
    let destination = metadata
        .as_ref()
        .and_then(|metadata| metadata.destination_branch.clone())
        .unwrap_or_else(|| source.clone());
    let saved_plan = crate::core::implementation::initial_reconciliation::load_plan(dir)?;
    let head = if let Some(plan) = saved_plan {
        anyhow::ensure!(
            plan.base == source,
            "Saved source branch differs from the selected source branch. Review the existing reconciliation snapshot before resuming."
        );
        crate::core::implementation::initial_reconciliation::support::validate_pinned_commits(
            repo,
            runner,
            &key(ticket),
            &plan,
        )?;
        plan.verified_commit
            .clone()
            .unwrap_or_else(|| plan.remote_commit.clone())
    } else {
        let local_ref = format!("refs/heads/{source}");
        let local_source = runner.git(repo, &["rev-parse", "--verify", &local_ref]);
        let advertised = if explicit_source.is_some() {
            Some(runner.git(repo, &["ls-remote", "--heads", "origin", &local_ref])?)
        } else {
            None
        };
        let (local, remote) = if explicit_source.is_some()
            && advertised.as_deref().is_some_and(str::is_empty)
        {
            let Some(local) = local_source.ok() else {
                return Err(
                    crate::core::implementation::initial_reconciliation::support::user_action(
                        format!(
                            "Selected source branch '{source}' no longer exists locally or on origin. Restore it or select an existing source branch."
                        ),
                    ),
                );
            };
            (local.clone(), local)
        } else {
            runner.update(format!("Fetching latest origin/{source}…"));
            // Fetch an explicit branch and resolve its immutable commit. Do not pull
            // into the user's checkout, which may contain unrelated drafts.
            let remote_ref = format!("refs/koolade-bases/{}", key(ticket));
            runner.git(
                repo,
                &[
                    "fetch",
                    "--no-tags",
                    "--no-write-fetch-head",
                    "origin",
                    &format!("+refs/heads/{source}:{remote_ref}"),
                ],
            )
            .map_err(|error| {
                if explicit_source.is_some() {
                    crate::core::implementation::initial_reconciliation::support::user_action(
                        format!("Selected source branch '{source}' is unavailable on origin. Refresh the repository and select an existing source branch. {error}"),
                    )
                } else {
                    error
                }
            })?;
            let remote = runner.git(repo, &["rev-parse", &remote_ref])?;
            let local = if explicit_source.is_some() {
                local_source.unwrap_or_else(|_| remote.clone())
            } else {
                runner.git(repo, &["rev-parse", "HEAD"])?
            };
            (local, remote)
        };
        runner.git(repo, &["cat-file", "-e", &format!("{local}^{{commit}}")])?;
        runner.git(repo, &["cat-file", "-e", &format!("{remote}^{{commit}}")])?;
        let common_base = runner.merge_base(repo, &local, &remote)?;
        match common_base {
            Some(common_base) if common_base == local => remote.clone(),
            Some(common_base) if common_base == remote => local.clone(),
            Some(_common_base) if publication_mode == PublicationMode::AutoPublish => {
                // Auto workers start from current remote truth, leaving divergent
                // local development history intact in the operator's checkout.
                remote.clone()
            }
            Some(common_base) => {
                let required_verification = crate::core::implementation::initial_reconciliation::support::required_baseline_checks_for_commits(
                    repo,
                    runner,
                    &common_base,
                    &local,
                    &remote,
                )?;
                crate::core::implementation::initial_reconciliation::save_plan(
                    dir,
                    &source,
                    &local,
                    &remote,
                    &common_base,
                    &required_verification,
                )?;
                crate::core::implementation::initial_reconciliation::support::pin_plan_commits(
                    repo,
                    runner,
                    &key(ticket),
                    &local,
                    &remote,
                )?;
                remote.clone()
            }
            None if publication_mode == PublicationMode::AutoPublish => remote.clone(),
            None => {
                return Err(
                    crate::core::implementation::initial_reconciliation::support::user_action(
                        format!(
                            "Local {source} and freshly fetched origin/{source} have no common history to reconcile automatically. Both versions are preserved; review the branch histories before implementing."
                        ),
                    ),
                );
            }
        }
    };
    Ok((destination, head, explicit_source.map(str::to_owned)))
}
