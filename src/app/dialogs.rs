//! Secondary dialogs: reference-doc import and stakeholder/identity
//! settings. Business effects (files, commits) run on SAVE only.

use egui::{Layout, RichText, TextEdit};

use crate::app::session::Project;
use crate::artifacts::{atomic_write, config_io, imports_io, CONFIG_FILE, IMPORTS_DIR};
use crate::core::gitops;
use crate::domain::stakeholder::{CategoryOwners, Stakeholders};
use crate::domain::user::CurrentUser;
use crate::error::AppError;
use crate::ui::theme;

// ---------------------------------------------------------------------------
// Import dialog
// ---------------------------------------------------------------------------

pub struct DlgImport {
    pub paths: String, // one per line (files or folders)
    pub feedback: Option<(bool, String)>, // (ok, message)
}

impl DlgImport {
    pub fn new() -> Self {
        Self { paths: String::new(), feedback: None }
    }

    /// Stage the listed paths into planning/imports/, then checkpoint.
    pub fn apply(&mut self, proj: &mut Project) -> Result<usize, AppError> {
        let mut staged = 0usize;
        for line in self.paths.lines().map(str::trim).filter(|l| !l.is_empty()) {
            let expanded = expand_tilde(line);
            let src = std::path::Path::new(&expanded);
            if !src.exists() {
                return Err(AppError::InvalidRepo {
                    path: line.to_string(),
                    detail: "file or folder does not exist".into(),
                });
            }
            let _doc = imports_io::import_into_repo(&proj.state.repo_root, src).map_err(|e| {
                AppError::Io {
                    op: format!("import {line}"),
                    detail: e.to_string(),
                }
            })?;
            staged += 1;
        }
        if staged > 0 {
            let _ = gitops::commit(
                &proj.state.repo_root,
                "planner: import reference material",
                &[format!("planning/{IMPORTS_DIR}")],
            );
            proj.refresh_git();
        }
        Ok(staged)
    }
}

// ---------------------------------------------------------------------------
// Settings dialog (current user + stakeholder categories)
// ---------------------------------------------------------------------------

pub struct Row {
    pub category: String,
    pub members: String,
}

pub struct DlgSettings {
    pub user_name: String,
    pub user_groups: String, // csv
    pub rows: Vec<Row>,
    pub feedback: Option<(bool, String)>,
}

impl DlgSettings {
    pub fn from_project(proj: &Project) -> Self {
        let cfg = &proj.state.config;
        let user = cfg.user.as_ref();
        let mut rows = Vec::new();
        for cat in cfg.stakeholders.iter_categories() {
            if let Some(entry) = cfg.stakeholders.find(cat) {
                rows.push(Row {
                    category: cat.to_string(),
                    members: entry.members.join(", "),
                });
            }
        }
        Self {
            user_name: user.map(|u| u.name.clone()).unwrap_or_default(),
            user_groups: user.map(|u| u.groups.join(", ")).unwrap_or_default(),
            rows,
            feedback: None,
        }
    }

    /// Commit the edited roster: rewrite config.md, resync, checkpoint.
    pub fn apply(&mut self, proj: &mut Project) -> Result<String, AppError> {
        let user = CurrentUser::new(self.user_name.trim(), csv_parts(&self.user_groups));
        let mut sk = Stakeholders::new(Vec::new());
        for r in &self.rows {
            let cat = r.category.trim();
            if cat.is_empty() {
                continue;
            }
            sk.upsert(CategoryOwners::new(cat, csv_parts(&r.members)));
        }
        let md = config_io::serialize(&config_io::PlannerConfig {
            user: Some(user),
            stakeholders: sk,
        });
        let dest = crate::artifacts::repo_artifact(&proj.state.repo_root, CONFIG_FILE);
        atomic_write(&dest, &md).map_err(|e| AppError::Io {
            op: "write .planner/config.md".into(),
            detail: e.to_string(),
        })?;
        proj.state.resync().map_err(|e| AppError::Io {
            op: "resync planning state".into(),
            detail: e.to_string(),
        })?;
        let sha = gitops::commit(
            &proj.state.repo_root,
            "settings: update stakeholders and identity",
            &[CONFIG_FILE.to_string()],
        )?;
        proj.refresh_git();
        Ok(sha.chars().take(7).collect())
    }
}

// ---------------------------------------------------------------------------
// Painting (returns frame signals; no hidden global state)
// ---------------------------------------------------------------------------

/// Paint card body; returns (save_pressed, close_pressed).
pub fn paint_import_card(ui: &mut egui::Ui, dlg: &mut DlgImport) -> (bool, bool) {
    ui.label(RichText::new("Add reference material").size(12.5).weak());
    ui.add_space(3.0);
    ui.label(
        RichText::new("Paste file or folder paths (one per line). Existing content under planning/ is ignored.")
            .weak()
            .size(11.0),
    );
    ui.add_space(4.0);
    ui.add_sized(
        egui::vec2(ui.available_width(), 116.0),
        TextEdit::multiline(&mut dlg.paths)
            .hint_text("/path/to/prd.pdf\n/path/to/architecture-notes/")
            .font(egui::FontId::monospace(12.0))
            .desired_width(f32::INFINITY)
            .desired_rows(5),
    );
    footers(ui, &dlg.feedback)
}

/// Paint card body; returns (save_pressed, close_pressed).
pub fn paint_settings_card(ui: &mut egui::Ui, dlg: &mut DlgSettings) -> (bool, bool) {
    ui.label(RichText::new("Who am I?").size(13.0).strong().color(theme::TEXT));
    ui.add_space(5.0);
    ui.horizontal(|ui| {
        ui.label(RichText::new("Name").size(12.0).weak());
        ui.add_sized(
            egui::vec2(260.0, 26.0),
            TextEdit::singleline(&mut dlg.user_name)
                .font(egui::FontId::proportional(12.5))
                .desired_width(240.0),
        );
    });
    ui.horizontal(|ui| {
        ui.label(RichText::new("Teams").size(12.0).weak());
        ui.add_sized(
            egui::vec2(340.0, 26.0),
            TextEdit::singleline(&mut dlg.user_groups)
                .hint_text("Platform, QA  (comma separated)")
                .font(egui::FontId::proportional(12.5))
                .desired_width(320.0),
        );
    });
    ui.add_space(10.0);
    ui.label(
        RichText::new("Categories & owners — drives question routing")
            .size(13.0)
            .strong()
            .color(theme::TEXT),
    );
    ui.add_space(4.0);
    let mut removed: Vec<usize> = Vec::new();
    egui::ScrollArea::vertical()
        .max_height(170.0)
        .show(ui, |ui| {
            for (i, r) in dlg.rows.iter_mut().enumerate() {
                ui.horizontal(|ui| {
                    ui.add_sized(
                        egui::vec2(180.0, 24.0),
                        TextEdit::singleline(&mut r.category)
                            .font(egui::FontId::proportional(12.0))
                            .desired_width(160.0),
                    );
                    ui.add_sized(
                        egui::vec2(280.0, 24.0),
                        TextEdit::singleline(&mut r.members)
                            .hint_text("members, comma, separated")
                            .font(egui::FontId::proportional(12.0))
                            .desired_width(260.0),
                    );
                    if ui.small_button("\u{2715}").clicked() {
                        removed.push(i);
                    }
                });
            }
        });
    for idx in removed.iter().rev() {
        if *idx < dlg.rows.len() {
            dlg.rows.remove(*idx);
        }
    }
    if ui.button("+ add category").clicked() {
        dlg.rows.push(Row {
            category: String::new(),
            members: String::new(),
        });
    }
    ui.add_space(6.0);
    footers(ui, &dlg.feedback)
}

fn footers(ui: &mut egui::Ui, feedback: &Option<(bool, String)>) -> (bool, bool) {
    if let Some((ok, msg)) = feedback {
        ui.add_space(8.0);
        ui.label(RichText::new(msg).size(11.5).color(if *ok { theme::SUCCESS } else { theme::DANGER }));
    }
    let mut save = false;
    let mut close = false;
    ui.add_space(10.0);
    ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
        let save_btn = ui.add(
            egui::Button::new(RichText::new("Save").strong().color(theme::BG))
                .fill(theme::ACCENT_SOFT)
                .corner_radius(6.0),
        );
        if save_btn.clicked() {
            save = true;
        }
        if ui.button(RichText::new("Close").weak()).clicked() {
            close = true;
        }
        ui.add_space(4.0);
    });
    (save, close)
}

fn csv_parts(csv: &str) -> Vec<String> {
    csv.split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

fn expand_tilde(raw: &str) -> String {
    if let Some(rest) = raw.strip_prefix("~/") {
        if let Ok(home) = std::env::var("HOME") {
            return format!("{home}/{rest}");
        }
    }
    raw.to_string()
}
