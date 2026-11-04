use super::*;

pub(super) struct Request<'a> {
    pub(super) cache: &'a RepositoryCache,
    pub(super) repo: &'a Path,
    pub(super) source_ref: &'a str,
    pub(super) source_sha: &'a str,
    pub(super) dir: &'a Path,
    pub(super) ticket: &'a str,
    pub(super) repository_id: &'a str,
    pub(super) destination_branch: &'a str,
    pub(super) runner: &'a Runner,
}

pub(super) fn resolve_pinned_source(request: Request<'_>) -> anyhow::Result<(String, String)> {
    let Request {
        cache,
        repo,
        source_ref,
        source_sha,
        dir,
        ticket,
        repository_id,
        destination_branch,
        runner,
    } = request;
    cache.validate_branch(source_ref, runner)?;
    let local_ref = format!("refs/heads/{source_ref}^{{commit}}");
    let local_ref_exists = runner
        .git(repo, &["rev-parse", "--verify", &local_ref])
        .is_ok();
    let remote_ref_exists = if let Some(remote) = cache.fetch_url.as_deref() {
        !runner
            .git(
                &cache.path,
                &[
                    "ls-remote",
                    "--heads",
                    remote,
                    &format!("refs/heads/{source_ref}"),
                ],
            )?
            .is_empty()
    } else {
        false
    };
    anyhow::ensure!(
        local_ref_exists || remote_ref_exists,
        "Selected source branch '{source_ref}' is unavailable in the selected code repository"
    );
    let source_has_pin = runner
        .git(
            repo,
            &["cat-file", "-e", &format!("{source_sha}^{{commit}}")],
        )
        .is_ok();
    let cache_has_pin = runner
        .git(
            &cache.path,
            &["cat-file", "-e", &format!("{source_sha}^{{commit}}")],
        )
        .is_ok();

    if source_has_pin {
        cache.import_local_branch(repo, source_ref, source_sha, runner)?;
    } else if !cache_has_pin {
        let remote = cache.fetch_url.as_deref().ok_or_else(|| {
            anyhow::anyhow!(
                "Approved source commit {source_sha} is unavailable in the selected repository"
            )
        })?;
        runner.git(
            &cache.path,
            &[
                "fetch",
                "--no-tags",
                "--no-write-fetch-head",
                remote,
                source_sha,
            ],
        )?;
    }

    // Keep the approved source SHA fixed when its ref advances. If the
    // approved history and the currently advertised source history diverge,
    // preserve both through the existing verified reconciliation workflow.
    if cache.fetch_url.is_some() && remote_ref_exists {
        let remote = cache.refresh_branch(source_ref, runner)?;
        let is_ancestor = |ancestor: &str, descendant: &str| {
            runner
                .git(
                    &cache.path,
                    &["merge-base", "--is-ancestor", ancestor, descendant],
                )
                .is_ok()
        };
        if !is_ancestor(source_sha, &remote) && !is_ancestor(&remote, source_sha) {
            let common_base = runner
                .merge_base(&cache.path, source_sha, &remote)?
                .ok_or_else(|| {
                    crate::core::implementation::initial_reconciliation::support::user_action(
                        format!(
                            "Approved source {source_ref} and its remote history have no common history (no common commit). Both histories are preserved; review them before implementation."
                        ),
                    )
                })?;
            let required = crate::core::implementation::initial_reconciliation::support::required_baseline_checks_for_commits(
                &cache.path,
                runner,
                &common_base,
                source_sha,
                &remote,
            )?;
            let key = crate::core::implementation::key_for_ticket(ticket);
            crate::core::implementation::initial_reconciliation::support::pin_plan_commits(
                &cache.path,
                runner,
                &key,
                source_sha,
                &remote,
            )?;
            crate::core::implementation::initial_reconciliation::save_clone_plan(
                dir,
                source_ref,
                source_sha,
                &remote,
                &common_base,
                &required,
                crate::core::implementation::initial_reconciliation::CloneRepositoryPlan {
                    repository_id: repository_id.to_owned(),
                    repository_identity: cache.identity.clone(),
                    source_ref: source_ref.to_owned(),
                    destination_branch: destination_branch.to_owned(),
                },
            )?;
        }
    }

    runner.git(
        &cache.path,
        &[
            "update-ref",
            &format!("refs/heads/{source_ref}"),
            source_sha,
        ],
    )?;
    let resolved = cache.resolve_branch(source_ref, runner)?;
    anyhow::ensure!(
        resolved == source_sha,
        "Task repository did not retain its approved source commit"
    );
    Ok((source_sha.to_owned(), source_sha.to_owned()))
}
