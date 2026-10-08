use crate::app::session::Project;
use crate::core::gitops;
use crate::core::state::PlannerState;
use crate::error::AppError;
use crate::persistence::{chat_store, project_slug};
use std::ffi::OsString;
use std::path::PathBuf;
/// Normalize user-typed paths (`~` expansion), then load-or-bootstrap.
pub fn attempt_connect(raw: &str) -> Result<Project, AppError> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Err(AppError::InvalidRepo {
            path: raw.to_string(),
            detail: "Please enter the path to a git repository (working tree).".into(),
        });
    }
    let expanded = expand_home(raw);
    let canonical = std::fs::canonicalize(&expanded).map_err(|e| AppError::InvalidRepo {
        path: raw.to_string(),
        detail: format!("could not open that path: {e}"),
    })?;
    if !canonical.is_dir() {
        return Err(AppError::InvalidRepo {
            path: canonical.to_string_lossy().into_owned(),
            detail: "path is not a directory".into(),
        });
    }
    if !gitops::is_work_tree(&canonical) {
        return Err(AppError::InvalidRepo {
            path: canonical.to_string_lossy().into_owned(),
            detail: "no .git directory found — Kool.ad/e plans inside a git working tree. Initialize one first (git init) or choose a different folder.".into(),
        });
    }

    crate::artifacts::transaction::recover(&canonical)
        .map_err(|e| AppError::Other(format!("planning transaction recovery failed: {e:#}")))?;
    crate::artifacts::migration::run(&canonical).map_err(|e| AppError::Artifact {
        path: canonical.to_string_lossy().into_owned(),
        detail: format!("project artifact migration failed: {e:#}"),
    })?;
    let mut state = PlannerState::load(&canonical).map_err(|e| AppError::Artifact {
        path: canonical.to_string_lossy().into_owned(),
        detail: e.to_string(),
    })?;
    let created = state.bootstrap_missing().map_err(|e| AppError::Io {
        op: "bootstrap planning artifacts".into(),
        detail: e.to_string(),
    })?;
    let paths = created;
    if !paths.is_empty() {
        gitops::commit(&canonical, "Kool.ad/e: bootstrap project artifacts", &paths).map_err(
            |e| {
                AppError::Other(format!(
                    "bootstrapped artifacts are preserved but checkpoint failed: {e}"
                ))
            },
        )?;
    }
    let slug = project_slug(&canonical);
    let mut chat = chat_store::load(&slug).0;
    if chat.is_empty() {
        let welcome = crate::app::session::welcome_message(&state.title);
        chat.push(welcome.clone());
        chat_store::append(&slug, &[welcome]).ok();
    }
    let task_documents = crate::artifacts::task_docs::load_board(&canonical, &state.workflow);
    let archived_tasks = crate::persistence::archived_tasks::load(&slug);
    let cancelled_work = crate::persistence::cancelled_work::load(&canonical).map_err(|error| {
        AppError::Artifact {
            path: canonical.to_string_lossy().into_owned(),
            detail: format!("cancelled work could not be loaded: {error}"),
        }
    })?;
    let mut planning_work = crate::core::planning_work::load(&canonical)
        .map_err(|e| crate::error::AppError::Other(e.to_string()))?;
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
    let linked = crate::core::planning_work::link_feature_identities(&state, &mut planning_work);
    let planning_save_error = if linked || !reconciled.is_empty() {
        crate::core::planning_work::save(&canonical, &planning_work).err()
    } else {
        None
    };
    let mut project = Project {
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
        task_live: Default::default(),
        planning_work,
        active_planning_work: None,
        queue: crate::core::implementation_queue::Queue::load(&canonical)
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
        if let Some(activity) = crate::core::implementation::load_activity(&canonical, &item.id) {
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

fn expand_home(raw: &str) -> OsString {
    if let Some(rest) = raw.strip_prefix("~/")
        && let Some(home) = std::env::var_os("HOME")
    {
        return PathBuf::from(home).join(rest).into_os_string();
    }
    raw.into()
}
