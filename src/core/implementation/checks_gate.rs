use super::*;
use crate::core::implementation::repository_cache::RepositoryCache;

pub(super) fn independent_check_passed(check: Option<&IndependentCheck>, commit: &str) -> bool {
    check.is_some_and(|check| {
        check.commit == commit && check.status == IndependentCheckStatus::Passed
    })
}

pub(super) struct CheckRequest<'a> {
    pub provider: &'a dyn checks::Provider,
    pub repository: &'a str,
    pub candidate_ref: &'a str,
    pub repository_path: &'a Path,
    pub commit: &'a str,
}

pub(super) fn wait_for_independent_checks(
    dir: &Path,
    state: &mut Implementation,
    repository_path: &Path,
    commit: &str,
    runner: &Runner,
    auto_publish_gate: Option<&AtomicBool>,
    claim_lease: Option<&crate::core::task_claim::ClaimLeaseHandle>,
) -> anyhow::Result<()> {
    let (base_remote, push_identity, effective_push) =
        if state.task_repository_kind == TaskRepositoryKind::Clone {
            let cache = RepositoryCache::from_saved_state(state, runner)?;
            cache.validate_task_remote(repository_path, runner)?;
            let base = cache
                .origin_url
                .clone()
                .ok_or_else(|| anyhow::anyhow!("Saved task has no trusted Git origin"))?;
            let push_identity = state
                .push_repository
                .clone()
                .or_else(|| cache.push_identity_url.clone())
                .unwrap_or_else(|| base.clone());
            let effective_push = cache
                .push_url
                .clone()
                .or_else(|| cache.origin_url.clone())
                .ok_or_else(|| anyhow::anyhow!("Saved task has no effective push destination"))?;
            (base, push_identity, effective_push)
        } else {
            let base = runner.git(repository_path, &["config", "--get", "remote.origin.url"])?;
            let push_identity = publication::push_identity_remote(repository_path, runner)?;
            let effective_push = publication::effective_push_remote(repository_path, runner)?;
            (base, push_identity, effective_push)
        };
    publication::validate_target_remotes(&base_remote, &push_identity, &effective_push).map_err(
        |error| {
            crate::core::implementation::initial_reconciliation::support::user_action(
                error.to_string(),
            )
        },
    )?;
    let Some(provider) = checks::for_remote(&push_identity) else {
        state.independent_check = Some(IndependentCheck {
            provider: "Unsupported provider".into(),
            commit: commit.into(),
            candidate_ref: String::new(),
            status: IndependentCheckStatus::Unavailable,
            checked_at: Some(chrono::Utc::now().to_rfc3339()),
            detail: Some(
                "Independent checks are required, but Kool.ad/e does not support the task's Git push destination yet."
                    .into(),
            ),
        });
        state.status = ImplementationStatus::WaitingForIndependentChecks;
        save(dir, state)?;
        return Err(failure(
            FailureKind::ExternalPrerequisite,
            RecoveryDisposition::UserAction,
            "Independent checks are required, but this Git remote has no supported CI provider",
        ));
    };
    let push_repository = provider.repository(&push_identity).ok_or_else(|| {
        anyhow::anyhow!("Supported CI provider could not identify its repository")
    })?;
    let candidate_ref = checks::candidate_ref(&task_repository::allocation_key(state), commit);
    wait_with_provider(
        CheckRequest {
            provider,
            repository: &push_repository,
            candidate_ref: &candidate_ref,
            repository_path,
            commit,
        },
        dir,
        state,
        runner,
        auto_publish_gate,
        claim_lease,
    )
}

pub(super) fn wait_with_provider(
    request: CheckRequest<'_>,
    dir: &Path,
    state: &mut Implementation,
    runner: &Runner,
    auto_publish_gate: Option<&AtomicBool>,
    claim_lease: Option<&crate::core::task_claim::ClaimLeaseHandle>,
) -> anyhow::Result<()> {
    let CheckRequest {
        provider,
        repository,
        candidate_ref,
        repository_path,
        commit,
    } = request;
    let repository_cache = if state.task_repository_kind == TaskRepositoryKind::Clone {
        let cache = RepositoryCache::from_saved_state(state, runner)?;
        cache.validate_task_remote(repository_path, runner)?;
        Some(cache)
    } else {
        None
    };
    let already_passed = state.independent_check.as_ref().is_some_and(|check| {
        check.commit == commit
            && check.candidate_ref == candidate_ref
            && check.status == IndependentCheckStatus::Passed
    });
    if already_passed {
        return Ok(());
    }
    let context_detail = state.detail.clone();
    state.independent_check = Some(IndependentCheck {
        provider: provider.name().into(),
        commit: commit.into(),
        candidate_ref: candidate_ref.to_owned(),
        status: IndependentCheckStatus::Pending,
        checked_at: Some(chrono::Utc::now().to_rfc3339()),
        detail: Some("Waiting for project checks on the locally verified commit.".into()),
    });
    state.status = ImplementationStatus::WaitingForIndependentChecks;
    state.detail = contextual_detail(
        &context_detail,
        &format!(
            "Waiting for {} to verify the integrated change.",
            provider.name()
        ),
    );
    save(dir, state)?;
    runner.update(format!(
        "Starting {} for the verified integrated change…",
        provider.name()
    ));
    if let Some(claim_lease) = claim_lease {
        publication::fenced_push(claim_lease, repository_path, commit, candidate_ref)?;
    } else if let Some(cache) = repository_cache.as_ref() {
        cache.push_commit(repository_path, commit, candidate_ref, runner)?;
    } else {
        runner.git(
            repository_path,
            &["push", "origin", &format!("{commit}:{candidate_ref}")],
        )?;
    }

    let deadline = Instant::now() + Duration::from_secs(15 * 60);
    loop {
        runner.remaining()?;
        if !publication::auto_publish_enabled(auto_publish_gate) {
            state.independent_check.as_mut().unwrap().detail = Some(
                "Checks are still running. Auto Publish was turned off, so the default branch will not be updated.".into(),
            );
            return publication::hold_for_review(dir, state);
        }
        let check_path = repository_cache
            .as_ref()
            .map_or(repository_path, |cache| cache.path.as_path());
        let result = match provider.check(runner, check_path, repository, commit) {
            Ok(result) => result,
            Err(error) => {
                let detail = format!("Could not read {} results: {error:#}", provider.name());
                let check = state.independent_check.as_mut().unwrap();
                check.status = IndependentCheckStatus::Unavailable;
                check.checked_at = Some(chrono::Utc::now().to_rfc3339());
                check.detail = Some(detail.clone());
                state.detail = contextual_detail(&context_detail, &detail);
                save(dir, state)?;
                return Err(failure(
                    FailureKind::ExternalPrerequisite,
                    RecoveryDisposition::UserAction,
                    "Independent checks are unavailable; verified work is preserved",
                ));
            }
        };
        let check = state.independent_check.as_mut().unwrap();
        check.status = checks::persisted_status(&result);
        check.checked_at = Some(chrono::Utc::now().to_rfc3339());
        match result {
            checks::ResultState::Passed => {
                check.detail = Some("All project workflows passed for this exact commit.".into());
                if !publication::auto_publish_enabled(auto_publish_gate) {
                    state.independent_check.as_mut().unwrap().detail = Some(
                        "Checks passed for this commit, but Auto Publish was turned off while they ran. The default branch was not updated.".into(),
                    );
                    save(dir, state)?;
                    return publication::hold_for_review(dir, state);
                }
                state.status = ImplementationStatus::Publishing;
                state.detail = contextual_detail(
                    &context_detail,
                    "Project checks passed. Publishing the verified change.",
                );
                save(dir, state)?;
                return Ok(());
            }
            checks::ResultState::Failed(detail) => {
                check.detail = Some(detail.clone());
                state.detail =
                    contextual_detail(&context_detail, &format!("Project checks failed: {detail}"));
                save(dir, state)?;
                return Err(failure(
                    FailureKind::Verification,
                    RecoveryDisposition::ExplicitResume,
                    format!("Project checks failed; the default branch was not updated. {detail}"),
                ));
            }
            checks::ResultState::Pending => {
                check.detail = Some("Waiting for all workflows to finish.".into());
                save(dir, state)?;
                if Instant::now() >= deadline {
                    let detail =
                        "No complete successful workflow result arrived within 15 minutes.";
                    let check = state.independent_check.as_mut().unwrap();
                    check.status = IndependentCheckStatus::Unavailable;
                    check.checked_at = Some(chrono::Utc::now().to_rfc3339());
                    check.detail = Some(detail.into());
                    state.detail = contextual_detail(&context_detail, detail);
                    save(dir, state)?;
                    return Err(failure(
                        FailureKind::ExternalPrerequisite,
                        RecoveryDisposition::UserAction,
                        "Independent checks timed out; the default branch was not updated",
                    ));
                }
                runner.update("Waiting for project checks to finish…");
                std::thread::sleep(Duration::from_secs(5));
            }
        }
    }
}

fn failure(
    kind: FailureKind,
    recovery: RecoveryDisposition,
    message: impl Into<String>,
) -> anyhow::Error {
    anyhow::Error::new(status::FailureCause(Failure::new(kind, recovery, message)))
}

fn contextual_detail(context: &str, status: &str) -> String {
    let context = context.trim();
    if context.is_empty() {
        status.to_owned()
    } else if context.lines().any(|line| line.trim() == status.trim()) {
        context.to_owned()
    } else {
        format!("{context}\n{status}")
    }
}
