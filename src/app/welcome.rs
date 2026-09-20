//! First-launch / connect screen: pick a git repository, validate it,
//! bootstrap planning artifacts if absent, and hydrate the ~/.packet chat.

use std::ffi::OsString;
use std::path::PathBuf;

use egui::{Frame, RichText, TextEdit};

use crate::app::session::Project;
use crate::core::gitops;
use crate::core::state::PlannerState;
use crate::error::AppError;
use crate::persistence::{chat_store, project_slug};
use crate::ui::theme;

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
            detail: "no .git directory found — Packet plans inside a git working tree. Initialize one first (git init) or choose a different folder.".into(),
        });
    }

    crate::artifacts::transaction::recover(&canonical)
        .map_err(|e| AppError::Other(format!("planning transaction recovery failed: {e:#}")))?;
    let mut state = PlannerState::load(&canonical).map_err(|e| AppError::Artifact {
        path: canonical.to_string_lossy().into_owned(),
        detail: e.to_string(),
    })?;
    let created = state.bootstrap_missing().map_err(|e| AppError::Io {
        op: "bootstrap planning artifacts".into(),
        detail: e.to_string(),
    })?;
    let legacy_path = canonical.join(crate::artifacts::SPEC_FILE);
    let legacy = std::fs::read_to_string(&legacy_path).unwrap_or_default();
    let _guard = crate::core::writer_gate::acquire();
    let migrated = crate::artifacts::product_docs::migrate(&canonical, &legacy).map_err(|e| {
        AppError::Artifact {
            path: canonical.to_string_lossy().into_owned(),
            detail: format!("product specification migration failed: {e:#}"),
        }
    })?;
    let mut paths: Vec<String> = created.iter().map(|p| p.to_string()).collect();
    paths.extend(migrated);
    paths.sort();
    paths.dedup();
    if !paths.is_empty() {
        gitops::commit(&canonical, "planner: migrate product specification", &paths).map_err(
            |e| {
                AppError::Other(format!(
                    "migration files are preserved but checkpoint failed: {e}"
                ))
            },
        )?;
    }
    state.resync().map_err(|e| AppError::Artifact {
        path: canonical.to_string_lossy().into_owned(),
        detail: format!("cannot reload migrated product: {e:#}"),
    })?;
    drop(_guard);
    let slug = project_slug(&canonical);
    let mut chat = chat_store::load(&slug).0;
    if chat.is_empty() {
        let welcome = crate::app::session::welcome_message(&state.title);
        chat.push(welcome.clone());
        chat_store::append(&slug, &[welcome]).ok();
    }
    let task_documents = crate::artifacts::task_docs::load_latest(&canonical, &state.workflow);
    let archived_tasks = crate::persistence::archived_tasks::load(&slug);
    let mut project = Project {
        task_chats: Default::default(),
        activity: Default::default(),
        task_documents,
        archived_tasks,
        state,
        chat_slug: slug,
        chat,
        draft: String::new(),
        active_turn: None,
        queue: crate::core::implementation_queue::Queue::load(&canonical)
            .map_err(|e| AppError::Other(e.to_string()))?,
        queue_lock: None,
        active_implementations: Default::default(),
        pr_refresh: None,
        reconciliation: None,
        reconciliation_attempted: Default::default(),
        reconciliation_error: None,
        reconciliation_cooldown_until: None,
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
    for item in &project.state.items {
        if let Some(activity) = crate::core::implementation::load_activity(&canonical, &item.id) {
            project.activity.tasks.insert(item.id.clone(), activity);
        }
    }
    Ok(project)
}

fn expand_home(raw: &str) -> OsString {
    if let Some(rest) = raw.strip_prefix("~/") {
        if let Some(home) = std::env::var_os("HOME") {
            return PathBuf::from(home).join(rest).into_os_string();
        }
    }
    raw.into()
}

/// Paint the centered card on the connect screen.
pub fn paint(card_ui: &mut egui::Ui, path: &mut String, error: Option<&str>) -> bool {
    card_ui.set_width(460.0);
    card_ui.add_space(12.0);
    card_ui.label(
        RichText::new("Packet")
            .strong()
            .size(36.0)
            .extra_letter_spacing(1.0)
            .color(theme::TEXT),
    );
    card_ui.label(
        RichText::new("Great products start with a clear idea.")
            .weak()
            .size(13.0),
    );
    card_ui.add_space(28.0);
    card_ui.label(
        RichText::new("Open your workspace")
            .size(12.5)
            .color(theme::TEXT_DIM),
    );
    card_ui.add_space(4.0);
    card_ui.add_sized(
        egui::vec2(card_ui.available_width(), 42.0),
        TextEdit::singleline(path)
            .hint_text("/path/to/my/project")
            .desired_width(f32::INFINITY)
            .font(egui::FontId::monospace(12.5)),
    );
    card_ui.add_space(6.0);
    let submit = card_ui.add_sized(
        egui::vec2(card_ui.available_width(), 44.0),
        egui::Button::new(
            RichText::new("Open workspace")
                .strong()
                .size(13.0)
                .color(theme::BG),
        )
        .fill(theme::TEXT)
        .corner_radius(6.0),
    );
    if submit.hovered() {
        card_ui.ctx().request_repaint();
    }
    card_ui.add_space(10.0);
    if let Some(e) = error {
        Frame::NONE
            .fill(egui::Color32::from_rgb(58, 24, 24))
            .corner_radius(6.0)
            .inner_margin(egui::Margin::symmetric(10, 8))
            .show(card_ui, |ui| {
                ui.label(RichText::new(e).color(theme::DANGER).size(12.5));
            });
        card_ui.add_space(6.0);
    }
    card_ui.add_space(4.0);
    card_ui.label(
        RichText::new(
            "A conversation on the left. A living specification on the right.\nYour decisions, captured and versioned in your repository.",
        )
        .weak()
        .size(11.0),
    );
    // Return true when Enter or click should submit.
    let entered =
        card_ui.input(|i| i.key_pressed(egui::Key::Enter)) && card_ui.input(|i| !i.modifiers.ctrl);
    submit.clicked() || (entered && !path.trim().is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn connecting_missing_path_errors_friendly() {
        match attempt_connect("") {
            Ok(_) => panic!("expected connection error"),
            Err(err) => assert!(matches!(err, AppError::InvalidRepo { .. })),
        }
    }

    #[test]
    fn connecting_nonexistent_path_errors_friendly() {
        match attempt_connect("/no/such/dir-xyz-123") {
            Ok(_) => panic!("expected connection error"),
            Err(err) => assert!(matches!(err, AppError::InvalidRepo { .. })),
        }
    }
}
