use super::*;

pub(super) fn independent_check_passed(check: Option<&IndependentCheck>, commit: &str) -> bool {
    check.is_some_and(|check| {
        check.commit == commit && check.status == IndependentCheckStatus::Passed
    })
}

pub(super) struct CheckRequest<'a> {
    pub provider: &'a dyn checks::Provider,
    pub repository: &'a str,
    pub candidate_ref: &'a str,
    pub worktree: &'a Path,
    pub commit: &'a str,
}

pub(super) fn wait_for_independent_checks(
    repo: &Path,
    dir: &Path,
    state: &mut Implementation,
    worktree: &Path,
    commit: &str,
    runner: &Runner,
    auto_publish_gate: Option<&AtomicBool>,
) -> anyhow::Result<()> {
    // Keep the configured hosting identity. `remote get-url` applies
    // insteadOf rewrites that can point at a local mirror or transport alias.
    let remote = runner.git(repo, &["config", "--get", "remote.origin.url"])?;
    let Some(provider) = checks::for_remote(&remote) else {
        state.independent_check = Some(IndependentCheck {
            provider: "Unsupported provider".into(),
            commit: commit.into(),
            candidate_ref: String::new(),
            status: IndependentCheckStatus::Unavailable,
            checked_at: Some(chrono::Utc::now().to_rfc3339()),
            detail: Some(
                "Independent checks are required, but Packet does not support this project's Git hosting provider yet."
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
    let repository = provider.repository(&remote).ok_or_else(|| {
        anyhow::anyhow!("Supported CI provider could not identify its repository")
    })?;
    let candidate_ref = checks::candidate_ref(&key(&state.ticket), commit);
    wait_with_provider(
        CheckRequest {
            provider,
            repository: &repository,
            candidate_ref: &candidate_ref,
            worktree,
            commit,
        },
        dir,
        state,
        runner,
        auto_publish_gate,
    )
}

pub(super) fn wait_with_provider(
    request: CheckRequest<'_>,
    dir: &Path,
    state: &mut Implementation,
    runner: &Runner,
    auto_publish_gate: Option<&AtomicBool>,
) -> anyhow::Result<()> {
    let CheckRequest {
        provider,
        repository,
        candidate_ref,
        worktree,
        commit,
    } = request;
    let already_passed = state.independent_check.as_ref().is_some_and(|check| {
        check.commit == commit
            && check.candidate_ref == candidate_ref
            && check.status == IndependentCheckStatus::Passed
    });
    if already_passed {
        return Ok(());
    }
    state.independent_check = Some(IndependentCheck {
        provider: provider.name().into(),
        commit: commit.into(),
        candidate_ref: candidate_ref.to_owned(),
        status: IndependentCheckStatus::Pending,
        checked_at: Some(chrono::Utc::now().to_rfc3339()),
        detail: Some("Waiting for project checks on the locally verified commit.".into()),
    });
    state.status = ImplementationStatus::WaitingForIndependentChecks;
    state.detail = format!(
        "Waiting for {} to verify the integrated change.",
        provider.name()
    );
    save(dir, state)?;
    runner.update(format!(
        "Starting {} for the verified integrated change…",
        provider.name()
    ));
    runner.git(
        worktree,
        &["push", "origin", &format!("{commit}:{candidate_ref}")],
    )?;

    let deadline = Instant::now() + Duration::from_secs(15 * 60);
    loop {
        runner.remaining()?;
        if !publication::auto_publish_enabled(auto_publish_gate) {
            state.independent_check.as_mut().unwrap().detail = Some(
                "Checks are still running. Auto Publish was turned off, so the default branch will not be updated.".into(),
            );
            return publication::hold_for_review(dir, state);
        }
        let result = match provider.check(runner, worktree, repository, commit) {
            Ok(result) => result,
            Err(error) => {
                let detail = format!("Could not read {} results: {error:#}", provider.name());
                let check = state.independent_check.as_mut().unwrap();
                check.status = IndependentCheckStatus::Unavailable;
                check.checked_at = Some(chrono::Utc::now().to_rfc3339());
                check.detail = Some(detail.clone());
                state.detail = detail;
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
                state.detail = "Project checks passed. Publishing the verified change.".into();
                save(dir, state)?;
                return Ok(());
            }
            checks::ResultState::Failed(detail) => {
                check.detail = Some(detail.clone());
                state.detail = format!("Project checks failed: {detail}");
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
                    state.detail = detail.into();
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
