use super::*;
use crate::{core::state::PlannerState, persistence::chat_store};

pub(super) fn connect_canonical_with_key(
    canonical: PathBuf,
    store: PlanningStore,
    local_key: String,
) -> Result<Project, AppError> {
    if !gitops::is_work_tree(&canonical) {
        return Err(AppError::InvalidRepo {
            path: canonical.to_string_lossy().into_owned(),
            detail: "no .git directory found — Kool.ad/e plans inside a git working tree. Initialize one first (git init) or choose a different folder.".into(),
        });
    }

    store
        .recover()
        .map_err(|e| AppError::Other(format!("local application storage recovery failed: {e}")))?;
    let mut state =
        PlannerState::load_with_store(&canonical, &store).map_err(|e| AppError::Artifact {
            path: canonical.to_string_lossy().into_owned(),
            detail: e.to_string(),
        })?;
    state.bootstrap_missing().map_err(|e| AppError::Io {
        op: "bootstrap planning artifacts".into(),
        detail: e.to_string(),
    })?;
    let slug = local_key;
    let mut chat = chat_store::load(&slug).0;
    if chat.is_empty() {
        let welcome = crate::app::session::welcome_message(&state.title);
        chat.push(welcome.clone());
        chat_store::append(&slug, &[welcome]).ok();
    }
    let task_documents = crate::artifacts::task_docs::load_board(&store, &state.workflow);
    let archived_tasks = crate::persistence::archived_tasks::load(&slug);
    let cancelled_work =
        crate::persistence::cancelled_work::load(&store).map_err(|error| AppError::Artifact {
            path: canonical.to_string_lossy().into_owned(),
            detail: format!("cancelled work could not be loaded: {error}"),
        })?;
    let recovered_git_intents =
        crate::core::planning_work::recover_unmapped_primary_git_intents(&store)
            .map_err(|error| AppError::Other(format!("Git intent recovery failed: {error}")))?;
    if recovered_git_intents > 0 {
        state.baseline_planning_revision = store
            .revision()
            .map_err(|error| AppError::Other(error.to_string()))?;
    }
    let (mut planning_work, planning_revision) =
        crate::core::planning_work::load_expected(&store, &state.baseline_planning_revision)
            .map_err(|e| crate::error::AppError::Other(e.to_string()))?;
    state.baseline_planning_revision = planning_revision;
    let cancelled_planning = planning_work
        .iter()
        .filter(|work| {
            cancelled_work.contains(&crate::persistence::cancelled_work::planning_id(&work.uid))
        })
        .map(|work| work.uid.clone())
        .collect();
    let reconciled = crate::core::planning_work::reconcile_inactive_excluding(
        &state,
        &mut planning_work,
        None,
        &cancelled_planning,
    );
    let planning_save_error = if !reconciled.is_empty() {
        match crate::core::planning_work::save_expected(
            &store,
            &planning_work,
            &state.baseline_planning_revision,
        ) {
            Ok(revision) => {
                state.baseline_planning_revision = revision;
                match crate::core::planning_work::load(&store) {
                    Ok(work) => {
                        planning_work = work;
                        None
                    }
                    Err(error) => Some(error),
                }
            }
            Err(error) => Some(error),
        }
    } else {
        None
    };
    let mut project = Project {
        specification_trace_navigation: None,
        task_chats: Default::default(),
        activity: Default::default(),
        task_documents,
        archived_tasks,
        cancelled_work,
        state,
        chat_slug: slug,
        chat,
        draft: String::new(),
        active_turn: None,
        task_turns: Default::default(),
        task_turn_user_message_ids: Default::default(),
        task_turn_reply_claims: Default::default(),
        task_live: Default::default(),
        planning_work,
        active_planning_work: None,
        queue: crate::core::implementation_queue::Queue::load(&store.root)
            .map_err(|e| AppError::Other(e.to_string()))?,
        queue_lock: None,
        active_implementations: Default::default(),
        pr_refresh: None,
        reconciliation: Default::default(),
        investigation: None,
        investigation_attempted: Default::default(),
        investigation_cooldown_until: None,
        last_pr_refresh: None,
        implementation_states: Default::default(),
        live_progress: crate::harness::LiveProgress::default(),
        next_question_id: None,
        git: gitops::snapshot(&canonical),
    };
    project.refresh_implementations();
    if recovered_git_intents > 0 {
        project.activity.pending.push(format!(
            "Recovered {recovered_git_intents} legacy work record(s). Originals were backed up under state/recovery/unmapped-primary-git-intent/. Review Git intent before starting execution."
        ));
    }
    if !reconciled.is_empty() {
        project.activity.pending.push(format!(
            "Kool.ad/e reconciled planning work with no active turn: {}.",
            reconciled.join("; ")
        ));
    }
    if let Some(error) = planning_save_error {
        project.activity.pending_planning_work = true;
        project.activity.pending.push(format!(
            "Planning work could not be saved: {error}. Kool.ad/e will retry automatically."
        ));
    }
    for item in &project.state.items {
        if let Some(activity) =
            crate::core::implementation::load_activity(&project.state.planning_store.root, &item.id)
        {
            project.activity.tasks.insert(item.id.clone(), activity);
        }
    }
    for ticket in project.activity.recover_dependency_reviews() {
        project.save_task_activity(&ticket);
    }
    if has_current_board_work(&project) {
        project.activity.pending.push(
            "Workspace connected; review existing board work for progress, user actions, and external blockers."
                .into(),
        );
    }
    Ok(project)
}

fn has_current_board_work(project: &Project) -> bool {
    project
        .state
        .items
        .iter()
        .any(|item| item.status == crate::domain::ItemStatus::Open)
        || project.planning_work.iter().any(|work| {
            work.status != crate::core::planning_work::WorkStatus::Done
                && !project
                    .cancelled_work
                    .contains(&crate::persistence::cancelled_work::planning_id(&work.uid))
        })
        || project.task_documents.iter().any(|doc| {
            !doc.path.ends_with("/README.md")
                && !project.archived_tasks.contains(&doc.path)
                && !project.task_cancelled(&doc.path)
                && project
                    .implementation_states
                    .get(&doc.path)
                    .is_none_or(|state| {
                        state.status != crate::core::implementation::ImplementationStatus::Completed
                    })
        })
        || !project.queue.blocked.is_empty()
}
