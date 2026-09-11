//! Secondary dialogs: reference-doc import and stakeholder/identity
//! settings. Business effects (files, commits) run on SAVE only.

use egui::{Layout, RichText, TextEdit};

use crate::app::session::Project;
use crate::artifacts::{atomic_write, config_io, imports_io, CONFIG_FILE, IMPORTS_DIR};
use crate::core::gitops;
use crate::domain::stakeholder::{CategoryOwners, Stakeholders};
use crate::domain::user::{CurrentUser, IdentitySource};
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
    /// Provenance line rendered under "Who am I?": the seated (DERIVED)
    /// identity per FR-13, labelled with how the seat was obtained. When the
    /// seat came from the config block or fell through to guest, the line
    /// also states that the fields act as the override.
    pub identity_note: String,
    pub rows: Vec<Row>,
    pub feedback: Option<(bool, String)>,
}

impl DlgSettings {
    /// Seed the card. The identity fields ECHO THE SEATED OPERATOR (the
    /// FR-13 derivation: git user.name → git user.email → config block →
    /// guest), never the raw config declaration — editing them and Saving is
    /// the override path, and a resync re-derives the seat (git still wins),
    /// so a save can never downgrade a git-derived seat.
    pub fn from_project(proj: &Project) -> Self {
        let seated = proj.state.effective_user();
        let label = proj.state.identity.source.label();
        let mut identity_note = format!("Seated as {} — {}", seated.name, label);
        if !matches!(
            proj.state.identity.source,
            IdentitySource::GitUserName | IdentitySource::GitUserEmail
        ) {
            identity_note.push_str("; these fields act as the override");
        }
        // A guest seat has no real identity to echo: fall back to the RAW
        // config block (legacy echo behaviour) — blank for the usual guest
        // trees, and defensively trimmed so even a name-less block left in
        // memory would echo a blank Name and no `Name: (guest)` phantom
        // could ever be persisted. The provenance line tells the operator
        // the seat is unaclaimed.
        let cfg = &proj.state.config;
        let (user_name, user_groups) = match proj.state.identity.source {
            IdentitySource::Guest => (
                cfg.user.as_ref().map(|u| u.name.trim().to_string()).unwrap_or_default(),
                cfg.user.as_ref().map(|u| u.groups.join(", ")).unwrap_or_default(),
            ),
            _ => (seated.name.clone(), seated.groups.join(", ")),
        };
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
            user_name,
            user_groups,
            identity_note,
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
    ui.add_space(3.0);
    ui.label(RichText::new(&dlg.identity_note).size(11.0).weak());
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::session::Project;
    use crate::core::gitops::test_support;
    use crate::core::state::PlannerState;
    use crate::domain::user::IdentitySource;

    fn project_from(root: &std::path::Path) -> Project {
        let state = PlannerState::load(root).unwrap();
        Project {
            state,
            chat_slug: "test-slug".into(),
            chat: Vec::new(),
            draft: String::new(),
            active_implementation: None,
            active_implementation_ticket: None,
            pr_refresh: None,
            last_pr_refresh: None,
            implementation_states: Default::default(),
            active_turn: None,
            live_progress: Default::default(),
            next_question_id: None,
            git: Default::default(),
            task_documents: Vec::new(),
        }
    }

    fn tempdir(tag: &str) -> std::path::PathBuf {
        let p = std::env::temp_dir().join(format!("packet_dlg_{tag}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    /// AC4: opened on a git-identified tree, the card prefills the DERIVED
    /// (git) name — not the config's contradicting 'Bob' — with a provenance
    /// line citing git user.name.
    #[test]
    fn settings_echoes_derived_git_identity_not_the_config_declaration() {
        let _shield = test_support::shield("dlg-echo");
        let root = tempdir("ada");
        let git = |args: &[&str]| -> std::process::Output {
            std::process::Command::new("git")
                .arg("-C")
                .arg(&root)
                .args(args)
                .output()
                .unwrap()
        };
        git(&["init", "-q", "-b", "main"]);
        git(&["config", "user.name", "Ada Lovelace"]);
        git(&["config", "user.email", "ada@example.org"]);
        let planner = root.join(".planner");
        std::fs::create_dir_all(&planner).unwrap();
        std::fs::write(
            planner.join("config.md"),
            "# Planner Configuration\n\n## Current User\nName: Bob\nGroups:\n\n## Stakeholders\n\n### QA\n(no owner configured)\n",
        )
        .unwrap();

        let proj = project_from(&root);
        assert_eq!(proj.state.identity.user.name, "Ada Lovelace");
        assert_eq!(proj.state.identity.source, IdentitySource::GitUserName);
        let dlg = DlgSettings::from_project(&proj);
        // The DERIVED value, not the config's 'Bob'.
        assert_eq!(dlg.user_name, "Ada Lovelace");
        assert!(dlg.identity_note.contains("git user.name"), "note: {}", dlg.identity_note);
        assert!(dlg.identity_note.contains("Ada Lovelace"), "note: {}", dlg.identity_note);
        assert!(!dlg.identity_note.contains("override"), "git seat is not an override: {}", dlg.identity_note);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Config-block seat: the echo follows the block and the note flags the
    /// override character of the fields.
    #[test]
    fn settings_echo_config_block_seat_flags_override() {
        let root = tempdir("dana");
        let planner = root.join(".planner");
        std::fs::create_dir_all(&planner).unwrap();
        std::fs::write(
            planner.join("config.md"),
            "# Planner Configuration\n\n## Current User\nName: Dana\nGroups: Ops, Platform\n\n## Stakeholders\n\n### QA\n(no owner configured)\n",
        )
        .unwrap();
        let proj = project_from(&root);
        let dlg = DlgSettings::from_project(&proj);
        assert_eq!(dlg.user_name, "Dana");
        assert_eq!(dlg.user_groups, "Ops, Platform");
        assert!(dlg.identity_note.contains(".planner/config.md"), "note: {}", dlg.identity_note);
        assert!(dlg.identity_note.contains("override"), "note: {}", dlg.identity_note);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Legacy degradation: git-less, artifact-less tree → guest echo with the
    /// matching source label; the dialog still builds cleanly. A GHOSTED
    /// block (whitespace-only Name:, real Groups) is nulled by the tolerant
    /// parser at load (pre-existing behaviour), so the dialog echoes blanks
    /// and a no-edit Save round-trips exactly what the app believes.
    #[test]
    fn settings_echo_guest_when_nothing_identifies_the_operator() {
        let root = tempdir("guest");
        let proj = project_from(&root);
        let dlg = DlgSettings::from_project(&proj);
        // Fields stay BLANK for a guest seat (legacy echo for an unset
        // block) so a no-edit Save cannot persist a phantom `Name: (guest)`
        // declaration; the provenance line carries the guest notice.
        assert_eq!(dlg.user_name, "");
        assert!(dlg.user_groups.is_empty());
        assert!(dlg.identity_note.contains("guest"), "note: {}", dlg.identity_note);
        assert!(dlg.identity_note.contains("override"), "note: {}", dlg.identity_note);
        let _ = std::fs::remove_dir_all(&root);

        // Ghosted block (whitespace-only Name:, real Groups): the tolerant
        // parser nulls the whole block on load (pre-existing behaviour), so
        // the dialog echoes blanks — and a no-edit Save round-trips exactly
        // what the app believes, losing nothing it holds.
        let ghost = tempdir("ghost");
        let planner = ghost.join(".planner");
        std::fs::create_dir_all(&planner).unwrap();
        std::fs::write(
            planner.join("config.md"),
            "# Planner Configuration\n\n## Current User\nName:    \nGroups: Ops\n\n## Stakeholders\n\n### QA\n(no owner configured)\n",
        )
        .unwrap();
        let proj = project_from(&ghost);
        assert_eq!(proj.state.identity.source, IdentitySource::Guest);
        assert!(proj.state.config.user.is_none(), "tolerant parser nulls ghosted blocks");
        let dlg = DlgSettings::from_project(&proj);
        assert_eq!(dlg.user_name, "");
        assert_eq!(dlg.user_groups, "");
        let _ = std::fs::remove_dir_all(&ghost);
    }

    /// AC4 save round-trip on a DRIFTED tree (git 'Ada Lovelace' vs config
    /// 'Bob'): a no-edit Save writes the echoed derived identity into the
    /// block, re-derives (git still wins — saving can never downgrade a
    /// git-derived seat), checkpoints with the conventional settings subject,
    /// and the reopened dialog echoes the SAME derived identity.
    #[test]
    fn no_edit_save_on_drifted_git_tree_keeps_git_seat_and_sets_checkpoint()
    {
        let _shield = test_support::shield("dlg-save");
        let root = tempdir("adasave");
        let git = |args: &[&str]| -> std::process::Output {
            std::process::Command::new("git")
                .arg("-C")
                .arg(&root)
                .args(args)
                .output()
                .unwrap()
        };
        git(&["init", "-q", "-b", "main"]);
        git(&["config", "user.name", "Ada Lovelace"]);
        git(&["config", "user.email", "ada@example.org"]);
        let planner = root.join(".planner");
        std::fs::create_dir_all(&planner).unwrap();
        std::fs::write(
            planner.join("config.md"),
            "# Planner Configuration\n\n## Current User\nName: Bob\nGroups: Ops\n\n## Stakeholders\n\n### QA\n(no owner configured)\n",
        )
        .unwrap();

        let mut proj = project_from(&root);
        assert_eq!(proj.state.identity.source, IdentitySource::GitUserName);
        let mut dlg = DlgSettings::from_project(&proj);
        // The card prefills the DERIVED name; the operator hits Save as-is.
        assert_eq!(dlg.user_name, "Ada Lovelace");
        let short = dlg.apply(&mut proj).expect("no-edit save must succeed");
        assert_eq!(short.chars().count(), 7);

        // Resync re-derived: the git seat survived the block rewrite.
        assert_eq!(proj.state.identity.user.name, "Ada Lovelace");
        assert_eq!(proj.state.identity.source, IdentitySource::GitUserName);
        assert_eq!(proj.state.effective_user().name, "Ada Lovelace");
        let log =
            String::from_utf8_lossy(&git(&["log", "-1", "--pretty=%s"]).stdout).into_owned();
        assert!(
            log.contains("settings: update stakeholders and identity"),
            "checkpoint subject: {log}"
        );
        // Storing the echoed git name in the block is benign redundancy.
        let block = std::fs::read_to_string(planner.join("config.md")).unwrap();
        assert!(block.contains("Name: Ada Lovelace"), "block: {block}");

        // Reopening the dialog echoes the same derived identity/provenance.
        let again = DlgSettings::from_project(&proj);
        assert_eq!(again.user_name, "Ada Lovelace");
        assert!(again.identity_note.contains("git user.name"), "note: {}", again.identity_note);
        let _ = std::fs::remove_dir_all(&root);
    }
}

fn expand_tilde(raw: &str) -> String {
    if let Some(rest) = raw.strip_prefix("~/") {
        if let Ok(home) = std::env::var("HOME") {
            return format!("{home}/{rest}");
        }
    }
    raw.to_string()
}
