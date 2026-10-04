use super::*;

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
    let migration_gate = crate::artifacts::migration::acquire_project_state_gate(planning_root)?;
    anyhow::ensure!(
        target_repository(planning_root, ticket)?.canonicalize()? == repo.canonicalize()?,
        "Implementation checkout does not match the task repository manifest"
    );
    let runner = Runner {
        gh: gh.into(),
        deadline: Instant::now() + crate::core::turn::configured_turn_timeout(),
        cancel,
        progress,
    };
    let (text, task_uid, metadata) = read_ticket_and_identity(planning_root, ticket)?;
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
    let mut state = if dir.join("state.json").exists() {
        let mut state = read_state_file(&dir.join("state.json"))?;
        anyhow::ensure!(
            (state.ticket == ticket
                || task_uid
                    .as_deref()
                    .is_some_and(|uid| state.task_uid.as_deref() == Some(uid)))
                && state.ticket_text == text
                && state
                    .task_uid
                    .as_deref()
                    .is_none_or(|uid| task_uid.as_deref() == Some(uid)),
            "Ticket changed since implementation started. Review the existing worktree before starting a revised ticket."
        );
        state.ticket = ticket.to_owned();
        if task_uid.is_some() {
            state.task_uid = task_uid.clone();
        }
        save(&dir, &state)?;
        state
    } else {
        // A reconciliation plan is durable before task state. If the process
        // stopped between those writes, resume its immutable snapshots instead
        // of fetching and constructing a different task base.
        let saved_plan = crate::core::implementation::initial_reconciliation::load_plan(&dir)?;
        let (base, head) = if let Some(plan) = saved_plan {
            crate::core::implementation::initial_reconciliation::support::validate_pinned_commits(
                repo,
                &runner,
                &key(ticket),
                &plan,
            )?;
            let head = plan
                .verified_commit
                .clone()
                .unwrap_or_else(|| plan.remote_commit.clone());
            (plan.base, head)
        } else {
            let base = if publication_mode == PublicationMode::AutoPublish {
                publication::default_branch(repo, &runner)?
            } else {
                runner.git(repo, &["symbolic-ref", "--short", "HEAD"])?
            };
            runner.update(format!("Fetching latest origin/{base}…"));
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
                    &format!("+refs/heads/{base}:{remote_ref}"),
                ],
            )?;
            let local = runner.git(repo, &["rev-parse", "HEAD"])?;
            let remote = runner.git(repo, &["rev-parse", &remote_ref])?;
            runner.git(repo, &["cat-file", "-e", &format!("{local}^{{commit}}")])?;
            runner.git(repo, &["cat-file", "-e", &format!("{remote}^{{commit}}")])?;
            let common_base = runner.merge_base(repo, &local, &remote)?;
            let head = match common_base {
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
                        &runner,
                        &common_base,
                        &local,
                        &remote,
                    )?;
                    crate::core::implementation::initial_reconciliation::save_plan(
                        &dir,
                        &base,
                        &local,
                        &remote,
                        &common_base,
                        &required_verification,
                    )?;
                    crate::core::implementation::initial_reconciliation::support::pin_plan_commits(
                        repo,
                        &runner,
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
                                "Local {base} and freshly fetched origin/{base} have no common history to reconcile automatically. Both versions are preserved; review the branch histories before implementing."
                            ),
                        ),
                    );
                }
            };
            (base, head)
        };
        let root = repo
            .parent()
            .ok_or_else(|| anyhow::anyhow!("Repository has no parent"))?
            .join(".koolade-worktrees")
            .join(crate::persistence::project_slug(&repo.canonicalize()?));
        fs::create_dir_all(&root)?;
        let dependency_context = completed_dependency_context(planning_root, ticket, &text)?;
        Implementation {
            ticket: ticket.into(),
            task_uid: task_uid.clone(),
            ticket_text: text,
            approved_specification: Path::new(ticket).parent().and_then(|parent| {
                fs::read_to_string(planning_root.join(parent).join("specification.md")).ok()
            }),
            approved_product_context: scoped_product_context(planning_root, ticket)?,
            completed_dependency_context: dependency_context,
            branch: format!("koolade/{}", key(ticket)),
            base,
            base_commit: head,
            worktree: root.join(key(ticket)),
            status: ImplementationStatus::Preparing,
            detail: String::new(),
            pr_url: None,
            verified_head: None,
            auto_merge: publication_mode == PublicationMode::AutoPublish,
            merged_commit: None,
            pr_state: None,
            pr_checked_at: None,
            pr_check_attempted_at: None,
            pr_check_error: None,
            independent_check: None,
            cleanup: Default::default(),
        }
    };
    if state.pr_url.is_none() && state.merged_commit.is_none() {
        state.auto_merge = publication_mode == PublicationMode::AutoPublish;
    }
    if state.status == ImplementationStatus::Completed {
        return Ok(state);
    }
    save(&dir, &state)?;
    drop(migration_gate);
    let result = runner
        .check_storage(&dir)
        .and_then(|_| runner.check_storage(&state.worktree))
        .and_then(|_| {
            lifecycle::execute(
                repo,
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
        deadline: Instant::now() + Duration::from_secs(120),
        cancel: runner.cancel.clone(),
        progress: runner.progress.clone(),
    };
    cleanup::run(repo, &dir, &mut state, &cleanup_runner);
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
