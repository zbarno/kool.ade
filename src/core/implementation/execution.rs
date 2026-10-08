use super::*;
mod initial_base;
mod resume_state;
mod source;
mod task_state;
pub(crate) use resume_state::{mark_resume_started, record_failed_attempt};

pub fn run(
    repo: &Path,
    ticket: &str,
    harness: &dyn AiHarness,
    cancel: Arc<AtomicBool>,
    progress: Sender<LiveProgress>,
) -> anyhow::Result<Implementation> {
    run_with_project_options(
        repo,
        repo,
        ticket,
        RunOptions {
            harness,
            cancel,
            progress,
            gh: "gh",
            publication_mode: PublicationMode::HoldForReview,
            require_independent_checks: false,
            user_context: None,
            auto_publish_gate: None,
        },
    )
}
pub(super) fn run_with_project_options(
    planning_root: &Path,
    repo: &Path,
    ticket: &str,
    options: RunOptions<'_>,
) -> anyhow::Result<Implementation> {
    let RunOptions {
        harness,
        cancel,
        progress,
        gh,
        publication_mode,
        require_independent_checks,
        user_context,
        auto_publish_gate,
    } = options;
    let active_progress = progress.clone();
    let migration_gate = crate::artifacts::migration::acquire_project_state_gate(planning_root)?;
    anyhow::ensure!(
        target_repository(planning_root, ticket)?.canonicalize()? == repo.canonicalize()?,
        "Implementation checkout does not match the task repository manifest"
    );
    let runner = Runner {
        gh: gh.into(),
        runtime_config_source: Some(repo.to_path_buf()),
        deadline: Instant::now() + crate::core::turn::configured_turn_timeout(),
        cancel,
        progress,
    };
    let (text, task_uid, metadata) = read_ticket_and_identity(planning_root, ticket)?;
    let telemetry_harness = super::telemetry::CaptureHarness::new(
        harness,
        planning_root,
        repo,
        &text,
        metadata.as_ref(),
        task_uid.as_deref(),
    );
    let harness: &dyn AiHarness = &telemetry_harness;
    let accrual = crate::core::time_accrual::span_for_ticket(
        planning_root,
        &text,
        metadata.as_ref(),
        task_uid.as_deref(),
    );
    let dir = state_dir_for_task(planning_root, ticket, task_uid.as_deref())?;
    fs::create_dir_all(&dir)?;
    let lock = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(dir.join("run.lock"))?;
    lock.try_lock().map_err(|_| {
        anyhow::anyhow!("This ticket is already being implemented in another Kool.ad/e instance")
    })?;
    let _active_telemetry = super::telemetry::ActiveSpan::new(
        planning_root,
        &text,
        metadata.as_ref(),
        task_uid.as_deref(),
        &telemetry_harness.session,
        active_progress,
    );
    let mut state = task_state::load_or_create(task_state::Request {
        planning_root,
        repo,
        ticket,
        text: &text,
        task_uid: task_uid.as_deref(),
        metadata: &metadata,
        publication_mode,
        dir: &dir,
        runner: &runner,
    })?;
    if state.task_repository_kind == TaskRepositoryKind::Clone {
        task_repository::validate_task_path(planning_root, &state)?;
    }
    if state.pr_url.is_none() && state.merged_commit.is_none() {
        state.auto_merge = publication_mode == PublicationMode::AutoPublish;
    }
    if state.status == ImplementationStatus::Completed {
        return Ok(state);
    }
    let lifecycle_repository = if state.task_repository_kind == TaskRepositoryKind::Clone {
        state
            .repository_cache
            .clone()
            .ok_or_else(|| anyhow::anyhow!("Saved repository cache path is missing"))?
    } else {
        repo.to_path_buf()
    };
    save(&dir, &state)?;
    drop(migration_gate);
    let result = runner
        .check_storage(&dir)
        .and_then(|_| runner.check_storage(&state.task_repository))
        .and_then(|_| {
            lifecycle::execute(
                &lifecycle_repository,
                &dir,
                &mut state,
                harness,
                &runner,
                ExecutionPolicy {
                    user_context,
                    publication_mode,
                    require_independent_checks,
                    auto_publish_gate: auto_publish_gate.as_deref(),
                    accrual,
                },
            )
        });
    if let Err(error) = result {
        state.status = if runner.cancel.load(Ordering::SeqCst) {
            ImplementationStatus::Interrupted
        } else {
            ImplementationStatus::Blocked
        };
        state.detail = format!("{error:#}");
        if let Err(save_error) = save(&dir, &state) {
            anyhow::bail!(
                "{}\nCould not persist the failed task state at {}: {save_error:#}. Check available disk space and permissions, then Resume implementation.",
                state.detail,
                dir.display()
            );
        }
        return Err(error);
    }
    // Completion is already durable. Reclamation has its own bounded budget
    // and records failure without turning a published task back into a failure.
    let cleanup_runner = Runner {
        gh: runner.gh.clone(),
        runtime_config_source: runner.runtime_config_source.clone(),
        deadline: Instant::now() + Duration::from_secs(120),
        cancel: runner.cancel.clone(),
        progress: runner.progress.clone(),
    };
    cleanup::run(&lifecycle_repository, &dir, &mut state, &cleanup_runner);
    if let Err(error) = save(&dir, &state) {
        if state.status != ImplementationStatus::Completed {
            return Err(error);
        }
        state.cleanup.error = Some(format!(
            "Could not save cleanup outcome: {error:#}. {}",
            state
                .cleanup
                .error
                .as_deref()
                .unwrap_or("Cleanup will be checked again on the next refresh.")
        ));
    }
    Ok(state)
}

#[cfg(test)]
pub(super) fn persist_source_plan_before_task_state(
    planning_root: &Path,
    repo: &Path,
    ticket: &str,
    runner: &Runner,
) -> anyhow::Result<()> {
    let (text, task_uid, metadata) = read_ticket_and_identity(planning_root, ticket)?;
    let dir = state_dir_for_task(planning_root, ticket, task_uid.as_deref())?;
    fs::create_dir_all(&dir)?;
    source::resolve(source::Request {
        planning_root,
        repo,
        dir: &dir,
        ticket,
        text: &text,
        metadata: metadata.as_ref(),
        publication_mode: PublicationMode::HoldForReview,
        runner,
    })?;
    Ok(())
}

#[cfg(test)]
pub(super) fn prepare_task_repository_before_reconciliation(
    planning_root: &Path,
    repo: &Path,
    ticket: &str,
    runner: &Runner,
) -> anyhow::Result<Implementation> {
    let (text, task_uid, metadata) = read_ticket_and_identity(planning_root, ticket)?;
    let dir = state_dir_for_task(planning_root, ticket, task_uid.as_deref())?;
    fs::create_dir_all(&dir)?;
    let mut state = task_state::load_or_create(task_state::Request {
        planning_root,
        repo,
        ticket,
        text: &text,
        task_uid: task_uid.as_deref(),
        metadata: &metadata,
        publication_mode: PublicationMode::HoldForReview,
        dir: &dir,
        runner,
    })?;
    let workspace_repository = state
        .repository_cache
        .clone()
        .unwrap_or_else(|| repo.to_path_buf());
    crate::core::implementation::verification::prepare_task_workspace(
        &workspace_repository,
        &dir,
        &mut state,
        runner,
    )?;
    Ok(state)
}
