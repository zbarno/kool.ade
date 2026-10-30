use super::*;

pub(crate) fn arbitrate_code_review_finding(
    planning_store: &PlanningStore,
    state_root: &Path,
    ticket: &str,
    context: &crate::core::implementation::code_review::CodeReviewArbitrationContext,
    harness: &dyn AiHarness,
    cancel: Arc<AtomicBool>,
    progress: Sender<LiveProgress>,
) -> anyhow::Result<crate::core::implementation::code_review::ManagerCodeReviewFindings> {
    let _migration_gate = crate::artifacts::locks::acquire_project_state_gate(state_root)?;
    let state_dir = state_dir_for_task(state_root, ticket, Some(&context.task_uid))?;
    let lock = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(state_dir.join("run.lock"))?;
    lock.try_lock().map_err(|_| {
        anyhow::anyhow!(
            "The task is active in another Kool.ad/e instance; arbitration was not started"
        )
    })?;
    let state = read_state_file(&state_dir.join("state.json"))?;
    anyhow::ensure!(
        state.task_uid.as_deref() == Some(context.task_uid.as_str()),
        "Code-review arbitration task identity does not match its saved implementation state"
    );
    task_repository::validate_task_path(state_root, &state)?;
    let runner = Runner {
        gh: "gh".into(),
        runtime_config_source: Some(state_root.to_path_buf()),
        deadline: Instant::now() + crate::core::turn::configured_turn_timeout(),
        cancel,
        progress,
        authorization_verifier: None,
    };
    crate::core::implementation::code_review::arbitrate_persisted_finding(
        planning_store,
        state_root,
        ticket,
        harness,
        &runner,
        context,
    )
}
