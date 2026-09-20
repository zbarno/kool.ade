//! Secondary dialogs: reference-doc import and stakeholder/identity
//! settings. Business effects (files, commits) run on SAVE only.

use egui::{Layout, RichText, TextEdit};

use crate::app::session::Project;
use crate::artifacts::{CONFIG_FILE, IMPORTS_DIR, atomic_write, config_io, imports_io};
use crate::core::gitops;
use crate::domain::stakeholder::{CategoryOwners, Stakeholders};
use crate::domain::user::{CurrentUser, IdentitySource};
use crate::error::AppError;
use crate::ui::theme;

// ---------------------------------------------------------------------------
// Import dialog
// ---------------------------------------------------------------------------

pub struct DlgImport {
    pub paths: String,                    // one per line (files or folders)
    pub feedback: Option<(bool, String)>, // (ok, message)
}

impl DlgImport {
    pub fn new() -> Self {
        Self {
            paths: String::new(),
            feedback: None,
        }
    }

    /// Stage the listed paths into planning/imports/, then checkpoint.
    pub fn apply(&mut self, proj: &mut Project) -> Result<usize, AppError> {
        // Writer section: import writes + checkpoint share the index.
        let _guard = crate::core::writer_gate::acquire();
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
            gitops::commit(
                &proj.state.repo_root,
                "planner: import reference material",
                &[format!("planning/{IMPORTS_DIR}")],
            )
            .map_err(|e| {
                AppError::Other(format!("imports were staged but checkpoint failed: {e}"))
            })?;
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
    /// Per-open background probe (F-16 guide, D-15): `Some` while the
    /// detached thread may still deliver its report; drained by
    /// `paint_harness_guide`, then dropped.
    pub probe_rx: Option<std::sync::mpsc::Receiver<ProbeReport>>,
    /// Live guide state: pending until the probe thread replies.
    pub probe_view: ProbeView,
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
                cfg.user
                    .as_ref()
                    .map(|u| u.name.trim().to_string())
                    .unwrap_or_default(),
                cfg.user
                    .as_ref()
                    .map(|u| u.groups.join(", "))
                    .unwrap_or_default(),
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
            // Per-open, DETACHED probe (D-15): bounded near ~12 s off the UI
            // thread (10 s poll + 2 s settle, NFR-4). The closure is 'static
            // and panic-free, and the send result is ignored, so an
            // abandoned open (dialog closed early) just lets the thread die
            // into a dead channel — no join, no accumulated handles.
            probe_rx: {
                let (tx, rx) = std::sync::mpsc::channel();
                std::thread::spawn(move || {
                    let _ = tx.send(crate::harness::PiHarness::probe_report());
                });
                Some(rx)
            },
            probe_view: ProbeView::Pending,
        }
    }

    /// Commit the edited roster: rewrite config.md, resync, checkpoint.
    pub fn apply(&mut self, proj: &mut Project) -> Result<String, AppError> {
        // Writer section: config write + checkpoint share the index.
        let _guard = crate::core::writer_gate::acquire();
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
// F-16 in-app pi harness setup guide (D-15: rendered-only, in this card)
// ---------------------------------------------------------------------------

/// Type alias so the dialog layer names the harness's display-purpose
/// snapshot without repeating the path.
pub type ProbeReport = crate::harness::pi_harness::ProbeReport;

/// Live discovery state painted in the guide: a per-open background probe,
/// pending until it replies.
#[derive(Clone, Default, Debug, PartialEq)]
pub enum ProbeView {
    /// The probe thread has not replied yet — the card shows the pending
    /// hint and NO detail line.
    #[default]
    Pending,
    /// The detached probe delivered its report.
    Report(ProbeReport),
}

/// Styling class for one rendered guide line (pins the golden-text tests).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum GuideLineKind {
    Title,
    Lead,
    Status,
    Detail,
    Order,
    Rule,
    Step,
}

/// One rendered line of the guide. Lines compose PURELY so the NFR-8
/// golden-text pins can exercise the copy without painting widgets.
#[derive(Debug, PartialEq)]
pub struct GuideLine {
    pub kind: GuideLineKind,
    pub text: String,
}

/// Compose the full "Set up the pi harness" guide for the given live
/// state. Pure: identical inputs yield identical lines. The discovery copy
/// is SINGLE-SOURCED from the harness (`PI_BINARY_ENV`, `COMMON_HOME_SITES`),
/// so the rendered order can never drift from the executed order; when
/// `home` is `None` (HOME unset) the order lines print literal `$HOME`,
/// mirroring `locate_binary` skipping its home sites entirely.
fn harness_guide_lines(view: &ProbeView, home: Option<&str>) -> Vec<GuideLine> {
    let env = crate::harness::pi_harness::PI_BINARY_ENV;
    let home_display = home.unwrap_or("$HOME");
    let mut lines = Vec::new();
    lines.push(GuideLine {
        kind: GuideLineKind::Title,
        text: "Set up the pi harness".into(),
    });
    lines.push(GuideLine {
        kind: GuideLineKind::Lead,
        text: "Packet shells out to a locally installed pi CLI; it downloads and \
               installs nothing itself."
            .into(),
    });
    lines.push(GuideLine {
        kind: GuideLineKind::Status,
        text: match view {
            ProbeView::Pending => "Looking for the pi CLI…".to_string(),
            ProbeView::Report(r) => r.status.clone(),
        },
    });
    if let ProbeView::Report(r) = view {
        // Pending intentionally shows NO detail line: the hint stands alone
        // until the probe actually says something.
        lines.push(GuideLine {
            kind: GuideLineKind::Detail,
            text: if r.ok {
                format!(
                    "Winning binary: {}",
                    r.binary
                        .as_deref()
                        .map(|p| p.display().to_string())
                        .unwrap_or_default()
                )
            } else {
                r.diagnostic.clone()
            },
        });
    }
    lines.push(GuideLine {
        kind: GuideLineKind::Rule,
        text: "Discovery order — first match wins:".into(),
    });
    lines.push(GuideLine {
        kind: GuideLineKind::Order,
        text: format!(
            "1. {env} override: if set and executable it wins outright; \
             a bad value fails fast with no fall-through."
        ),
    });
    lines.push(GuideLine {
        kind: GuideLineKind::Order,
        text: "2. pi in every PATH directory, in PATH order.".into(),
    });
    for (i, site) in crate::harness::pi_harness::COMMON_HOME_SITES
        .iter()
        .enumerate()
    {
        lines.push(GuideLine {
            kind: GuideLineKind::Order,
            text: format!("{}. {home_display}/{site}/pi", i + 3),
        });
    }
    lines.push(GuideLine {
        kind: GuideLineKind::Rule,
        text: "Version policy: no floor, no pinning — any installed pi is \
              accepted; the probed version is display-only (D-13)."
            .into(),
    });
    lines.push(GuideLine {
        kind: GuideLineKind::Lead,
        text: "Install & make discoverable:".into(),
    });
    lines.push(GuideLine {
        kind: GuideLineKind::Step,
        text: "1. Obtain pi via the vendor channel — npm install -g \
               @earendil-works/pi-coding-agent (adjust if the vendor's \
               documented channel differs)."
            .into(),
    });
    lines.push(GuideLine {
        kind: GuideLineKind::Step,
        text: format!(
            "2. Make it reachable via PATH, a home location above, or {env}=/\
             abs/path/to/pi in the launching environment."
        ),
    });
    lines.push(GuideLine {
        kind: GuideLineKind::Step,
        text: "3. Reopen this dialog and confirm the status reads 'pi \
              <version>'."
            .into(),
    });
    lines
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
    ui.label(
        RichText::new("Who am I?")
            .size(13.0)
            .strong()
            .color(theme::TEXT),
    );
    ui.add_space(3.0);
    ui.label(RichText::new(&dlg.identity_note).size(11.0).weak());
    ui.add_space(5.0);
    ui.horizontal(|ui| {
        ui.label(RichText::new("Name").size(12.0).weak());
        ui.add_sized(
            egui::vec2(ui.available_width(), 26.0),
            TextEdit::singleline(&mut dlg.user_name)
                .font(egui::FontId::proportional(12.5))
                .desired_width(240.0),
        );
    });
    ui.horizontal(|ui| {
        ui.label(RichText::new("Teams").size(12.0).weak());
        ui.add_sized(
            egui::vec2(ui.available_width(), 26.0),
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
    ui.label(
        RichText::new("Choose existing people or teams, or enter new owners separated by commas.")
            .size(12.0)
            .weak(),
    );
    let owners = owner_choices(dlg);
    let mut removed: Vec<usize> = Vec::new();
    for (i, row) in dlg.rows.iter_mut().enumerate() {
        ui.push_id(("ownership_row", i), |ui| {
            egui::Frame::group(ui.style()).show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Category");
                    ui.add_sized(
                        [(ui.available_width() - 40.0).max(60.0), 28.0],
                        TextEdit::singleline(&mut row.category).id_salt("category"),
                    );
                    if crate::ui::overlays::close_button(ui)
                        .on_hover_text("Remove category")
                        .clicked()
                    {
                        removed.push(i);
                    }
                });
                ui.label(RichText::new("Owners").size(12.0).weak());
                ui.add(
                    TextEdit::singleline(&mut row.members)
                        .id_salt("owners")
                        .desired_width(f32::INFINITY)
                        .hint_text("Names or teams, separated by commas"),
                );
                egui::ComboBox::from_id_salt("existing_owners")
                    .selected_text("Select existing owners…")
                    .width(240.0_f32.min(ui.available_width()))
                    .show_ui(ui, |ui| {
                        if owners.is_empty() {
                            ui.label("Enter a name or team to make it available here.");
                        }
                        for owner in &owners {
                            let mut selected = csv_parts(&row.members)
                                .iter()
                                .any(|value| value.eq_ignore_ascii_case(owner));
                            if ui.checkbox(&mut selected, owner).changed() {
                                set_owner_selected(&mut row.members, owner, selected);
                            }
                        }
                    });
            });
            ui.add_space(6.0);
        });
    }
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
    ui.add_space(12.0);
    ui.separator();
    ui.collapsing("AI harness setup", |ui| paint_harness_guide(ui, dlg));
    ui.add_space(6.0);
    footers(ui, &dlg.feedback)
}

/// Suggestions come from the current seat, its teams and existing category owners.
fn owner_choices(dlg: &DlgSettings) -> Vec<String> {
    let mut owners = Vec::<String>::new();
    for owner in std::iter::once(dlg.user_name.trim().to_string())
        .chain(csv_parts(&dlg.user_groups))
        .chain(dlg.rows.iter().flat_map(|row| csv_parts(&row.members)))
    {
        if owner.is_empty()
            || matches!(
                owner.to_ascii_lowercase().as_str(),
                "(guest)" | "(owner tbd)" | "-" | "all"
            )
        {
            continue;
        }
        if !owners
            .iter()
            .any(|existing| existing.eq_ignore_ascii_case(&owner))
        {
            owners.push(owner);
        }
    }
    owners.sort_by_key(|owner| owner.to_lowercase());
    owners
}

fn set_owner_selected(members: &mut String, owner: &str, selected: bool) {
    let mut owners = csv_parts(members);
    if selected {
        if !owners
            .iter()
            .any(|existing| existing.eq_ignore_ascii_case(owner))
        {
            owners.push(owner.to_owned());
        }
    } else {
        owners.retain(|existing| !existing.eq_ignore_ascii_case(owner));
    }
    *members = owners.join(", ");
}

/// Drain whatever the detached probe has queued since the last frame; flip
/// `dlg.probe_view` to the newest report and forget a channel whose sender
/// has gone (probe delivered, or died with its superseded open). Borrow-only
/// phase first (the receiver cannot be cloned), then commit the mutations.
fn drain_probe(dlg: &mut DlgSettings) {
    let (incoming, sender_gone) = match dlg.probe_rx.as_ref() {
        Some(rx) => {
            let mut incoming = None;
            let mut gone = false;
            loop {
                match rx.recv_timeout(std::time::Duration::ZERO) {
                    Ok(rep) => incoming = Some(rep),
                    Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                        gone = true;
                        break;
                    }
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => break,
                }
            }
            (incoming, gone)
        }
        None => (None, false),
    };
    if let Some(rep) = incoming {
        dlg.probe_view = ProbeView::Report(rep);
    }
    if sender_gone {
        dlg.probe_rx = None;
    }
}

/// Paint the F-16 "Set up the pi harness" section (private; rendered-only —
/// no text inputs, no file writes, no navigation). Drains the probe (so the
/// pending → resolved flip lands within the repaint cadence, no reopen
/// needed), then composes and styles the golden-pinned line set. Keeps
/// `paint_settings_card`'s `(bool, bool)` footer contract intact.
fn paint_harness_guide(ui: &mut egui::Ui, dlg: &mut DlgSettings) {
    drain_probe(dlg);
    // Mirror `locate_binary`: HOME unset ⇔ home sites skipped in code, so the
    // composer renders literal "$HOME" lines for the reduced search.
    let home_raw = std::env::var("HOME").ok();
    let home = home_raw.as_deref();
    let lines = harness_guide_lines(&dlg.probe_view, home);
    let probe_failed = matches!(&dlg.probe_view, ProbeView::Report(r) if !r.ok);
    for line in &lines {
        let rt = match line.kind {
            GuideLineKind::Title => RichText::new(&line.text)
                .size(13.0)
                .strong()
                .color(theme::TEXT),
            GuideLineKind::Lead | GuideLineKind::Rule => {
                RichText::new(&line.text).size(11.0).weak()
            }
            GuideLineKind::Status => {
                RichText::new(&line.text)
                    .size(12.0)
                    .strong()
                    .color(match &dlg.probe_view {
                        ProbeView::Pending => theme::TEXT_DIM,
                        ProbeView::Report(r) if r.ok => theme::SUCCESS,
                        ProbeView::Report(_) => theme::DANGER,
                    })
            }
            GuideLineKind::Detail if probe_failed => RichText::new(&line.text)
                .size(11.0)
                .weak()
                .color(theme::DANGER),
            GuideLineKind::Detail => RichText::new(&line.text).size(11.0).weak(),
            GuideLineKind::Order => RichText::new(&line.text)
                .monospace()
                .size(11.5)
                .color(theme::TEXT_DIM),
            GuideLineKind::Step => RichText::new(&line.text)
                .monospace()
                .size(11.5)
                .color(theme::TEXT),
        };
        ui.label(rt);
    }
}

/// Placeholder for the first-run editor — ITSELF valid JSON so pasting the
/// hint straight into Save passes the well-formedness probe.
pub const MCP_EXAMPLE_HINT: &str = "{\n  \"mcpServers\": {\n    \"example\": { \"command\": \"your-server\", \"args\": [] }\n  }\n}";

/// F-18 / D-16: the in-app editor for `.planner/mcp.json`. Raw free-text,
/// mono-spaced; the business effect (write/remove + checkpoint) is owned by
/// `crate::artifacts::mcp_io`, which keeps this struct a dumb carrier.
pub struct DlgMcp {
    /// The editor buffer; bound to the file's live bytes on open.
    pub text: String,
    /// True when the field opened from NO existing file — the painter then
    /// shows the exemplar hint as placeholder.
    pub hint_active: bool,
    pub feedback: Option<(bool, String)>,
    /// Sticky, non-blocking warning (orange) — set on a malformed-yet-saved
    /// buffer or an unreadable pre-existing file; survives until Close.
    pub warning: Option<String>,
    /// Keep-open driver owned by `perform_mcp`: after a malformed save the
    /// card MUST stay up showing the warning; Unchanged/Write/Clear closes.
    pub keep_open: bool,
}

impl DlgMcp {
    /// Seed the card from the LIVE file bytes (first-run: absent → hint).
    pub fn from_project(proj: &Project) -> Self {
        let st = crate::artifacts::mcp_io::load_state(&proj.state.repo_root);
        let (text, hint_active, warning) = Self::dialog_fields(&st);
        Self {
            text,
            hint_active,
            feedback: None,
            warning,
            keep_open: true,
        }
    }

    /// Factor the load → field mapping so the pins exercise the REAL
    /// branching (absent / present-ok / present-unreadable) without egui.
    /// Unreadable ⇒ the field starts empty WITH an explicit OVERWRITE
    /// warning: unseen content is never destroyed silently.
    fn dialog_fields(
        st: &crate::artifacts::mcp_io::McpLoadState,
    ) -> (String, bool, Option<String>) {
        if !st.present {
            (String::new(), true, None)
        } else if let Some(content) = &st.content {
            (content.clone(), false, None)
        } else {
            let err = st
                .read_error
                .clone()
                .unwrap_or_else(|| "unknown read error".into());
            (
                String::new(),
                false,
                Some(format!(
                    "Existing .planner/mcp.json could not be read ({err}). The field starts empty — saving will OVERWRITE the file."
                )),
            )
        }
    }

    /// Thin shim onto the module that owns all disk/git effects. No
    /// `PlannerState` resync is owed: mcp.json is never cached in state —
    /// the next turn's `context_build` re-reads the file fresh.
    pub fn apply(
        &mut self,
        proj: &mut Project,
    ) -> Result<crate::artifacts::mcp_io::McpApplyReceipt, AppError> {
        crate::artifacts::mcp_io::apply_save(&proj.state.repo_root, &self.text)
    }
}

/// Paint the MCP card; returns (save_pressed, close_pressed). Height budget
/// (~360 px) fits the 640 px min window — no ScrollArea, unlike the grown
/// settings card.
pub fn paint_mcp_card(ui: &mut egui::Ui, dlg: &mut DlgMcp) -> (bool, bool) {
    ui.label(
        RichText::new("MCP server configuration")
            .size(13.0)
            .strong()
            .color(theme::TEXT),
    );
    ui.add_space(3.0);
    ui.label(
        RichText::new(
            "Raw .planner/mcp.json — advertised verbatim to every pi session. The planner enforces no server schema; blank + Save removes the file (unconfigured)",
        ).weak().size(11.0),
    );
    ui.add_space(4.0);
    // Multiline pattern mirrors the import dialog; the exemplar hint is only
    // offered while the field opened with no file underneath (hint_active).
    let editor = TextEdit::multiline(&mut dlg.text)
        .font(egui::FontId::monospace(12.0))
        .desired_width(f32::INFINITY)
        .desired_rows(10);
    let editor = if dlg.hint_active {
        editor.hint_text(MCP_EXAMPLE_HINT)
    } else {
        editor
    };
    ui.add_sized(egui::vec2(ui.available_width(), 210.0), editor);
    // The verified feed cap: prompts clip past 4,096 chars (context_build).
    // DISK NEVER clips — this line only telegraphs the presentation cutoff.
    ui.label(
        RichText::new(format!(
            "{} chars — prompts clip past 4096",
            dlg.text.chars().count()
        ))
        .size(10.5)
        .weak()
        .color(theme::TEXT_DIM),
    );
    if let Some(warning) = &dlg.warning {
        ui.add_space(6.0);
        ui.label(
            RichText::new(warning)
                .size(11.5)
                .weak()
                .color(theme::WARNING),
        );
    }
    footers(ui, &dlg.feedback)
}

fn footers(ui: &mut egui::Ui, feedback: &Option<(bool, String)>) -> (bool, bool) {
    if let Some((ok, msg)) = feedback {
        ui.add_space(8.0);
        ui.label(RichText::new(msg).size(11.5).color(if *ok {
            theme::SUCCESS
        } else {
            theme::DANGER
        }));
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
            task_chats: Default::default(),
            activity: Default::default(),
            state,
            chat_slug: "test-slug".into(),
            chat: Vec::new(),
            draft: String::new(),
            queue: Default::default(),
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
            active_turn: None,
            live_progress: Default::default(),
            next_question_id: None,
            git: Default::default(),
            task_documents: Vec::new(),
            archived_tasks: Default::default(),
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
        assert!(
            dlg.identity_note.contains("git user.name"),
            "note: {}",
            dlg.identity_note
        );
        assert!(
            dlg.identity_note.contains("Ada Lovelace"),
            "note: {}",
            dlg.identity_note
        );
        assert!(
            !dlg.identity_note.contains("override"),
            "git seat is not an override: {}",
            dlg.identity_note
        );
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
        assert!(
            dlg.identity_note.contains(".planner/config.md"),
            "note: {}",
            dlg.identity_note
        );
        assert!(
            dlg.identity_note.contains("override"),
            "note: {}",
            dlg.identity_note
        );
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
        assert!(
            dlg.identity_note.contains("guest"),
            "note: {}",
            dlg.identity_note
        );
        assert!(
            dlg.identity_note.contains("override"),
            "note: {}",
            dlg.identity_note
        );
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
        assert!(
            proj.state.config.user.is_none(),
            "tolerant parser nulls ghosted blocks"
        );
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
    fn no_edit_save_on_drifted_git_tree_keeps_git_seat_and_sets_checkpoint() {
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
        let log = String::from_utf8_lossy(&git(&["log", "-1", "--pretty=%s"]).stdout).into_owned();
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
        assert!(
            again.identity_note.contains("git user.name"),
            "note: {}",
            again.identity_note
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    // ---- F-16 guide golden-text / probe pins (NFR-8, D-15) -------------

    use GuideLineKind as K;

    fn gl(k: K, t: &str) -> GuideLine {
        GuideLine {
            kind: k,
            text: t.into(),
        }
    }

    fn rep(status: &str, diagnostic: &str, binary: Option<&str>, ok: bool) -> ProbeReport {
        ProbeReport {
            status: status.into(),
            diagnostic: diagnostic.into(),
            binary: binary.map(std::path::PathBuf::from),
            ok,
        }
    }

    /// GOLDEN — found state: exact line count, ordering, and byte-identical
    /// text for every line, including the five discovery lines, the override
    /// naming, the version-policy rule, and the npm step. Any wording or
    /// reorder breakage fails this test (the D-15 render pin).
    #[test]
    fn harness_guide_golden_found_state_pins_every_line() {
        let view = ProbeView::Report(rep("pi 0.84.4", "", Some("/home/op/.local/bin/pi"), true));
        let lines = harness_guide_lines(&view, Some("/home/op"));
        let expected = vec![
            gl(K::Title, "Set up the pi harness"),
            gl(
                K::Lead,
                "Packet shells out to a locally installed pi CLI; it downloads and \
                 installs nothing itself.",
            ),
            gl(K::Status, "pi 0.84.4"),
            gl(K::Detail, "Winning binary: /home/op/.local/bin/pi"),
            gl(K::Rule, "Discovery order — first match wins:"),
            gl(
                K::Order,
                "1. PACKET_PI_BIN override: if set and executable it wins outright; \
                 a bad value fails fast with no fall-through.",
            ),
            gl(K::Order, "2. pi in every PATH directory, in PATH order."),
            gl(K::Order, "3. /home/op/.npm-global/bin/pi"),
            gl(K::Order, "4. /home/op/.local/bin/pi"),
            gl(K::Order, "5. /home/op/.pi/bin/pi"),
            gl(
                K::Rule,
                "Version policy: no floor, no pinning — any installed pi is \
                 accepted; the probed version is display-only (D-13).",
            ),
            gl(K::Lead, "Install & make discoverable:"),
            gl(
                K::Step,
                "1. Obtain pi via the vendor channel — npm install -g \
                 @earendil-works/pi-coding-agent (adjust if the vendor's \
                 documented channel differs).",
            ),
            gl(
                K::Step,
                "2. Make it reachable via PATH, a home location above, or \
                 PACKET_PI_BIN=/abs/path/to/pi in the launching environment.",
            ),
            gl(
                K::Step,
                "3. Reopen this dialog and confirm the status reads 'pi \
                 <version>'.",
            ),
        ];
        assert_eq!(lines, expected, "guide copy drifted from the D-15 pin");
    }

    /// GOLDEN — unavailable state: the status line pins the DANGER-state
    /// string verbatim and the Detail line is the diagnostic itself, with no
    /// “Winning binary” claim (fast-fail semantics preserved in the copy).
    #[test]
    fn harness_guide_golden_unavailable_state_pins_danger_status_and_diagnostic() {
        let view = ProbeView::Report(rep(
            "pi (unavailable: Pi harness not found)",
            "PACKET_PI_BIN=/nonexistent-packet-selftest/pi is not an executable file",
            None,
            false,
        ));
        let lines = harness_guide_lines(&view, Some("/home/op"));
        let status = lines
            .iter()
            .find(|l| l.kind == K::Status)
            .expect("status line present");
        assert_eq!(status.text, "pi (unavailable: Pi harness not found)");
        let detail = lines
            .iter()
            .find(|l| l.kind == K::Detail)
            .expect("detail line present for a report");
        assert_eq!(
            detail.text,
            "PACKET_PI_BIN=/nonexistent-packet-selftest/pi is not an executable file"
        );
        // Fail-fast honesty: no line may promise a PATH rescue for a bad
        // override.
        assert!(
            !lines
                .iter()
                .any(|l| l.text.contains("would have") || l.text.contains("fallback"))
        );
    }

    /// PENDING state + HOME-less composition: the first paint shows the
    /// placeholder status and NO Detail line; with HOME unset the order
    /// lines render literal $HOME (mirroring locate_binary skipping home
    /// sites) and nothing else changes shape.
    #[test]
    fn harness_guide_pending_state_pins_placeholder_and_absent_detail() {
        let lines = harness_guide_lines(&ProbeView::Pending, None);
        let status = lines
            .iter()
            .find(|l| l.kind == K::Status)
            .expect("status line present");
        assert_eq!(status.text, "Looking for the pi CLI\u{2026}");
        assert!(
            !lines.iter().any(|l| l.kind == K::Detail),
            "pending state must not show a detail line: {lines:?}"
        );
        let orders: Vec<&GuideLine> = lines.iter().filter(|l| l.kind == K::Order).collect();
        assert_eq!(orders.len(), 5, "five discovery tiers");
        assert_eq!(orders[2].text, "3. $HOME/.npm-global/bin/pi");
        assert_eq!(orders[3].text, "4. $HOME/.local/bin/pi");
        assert_eq!(orders[4].text, "5. $HOME/.pi/bin/pi");
        assert!(
            orders[0].text.starts_with("1."),
            "env override stays tier 1: {}",
            orders[0].text
        );
        assert!(
            orders[0].text.contains("PACKET_PI_BIN"),
            "{}",
            orders[0].text
        );
        assert_eq!(lines.len(), 14, "pending drops exactly the detail line");
    }

    /// Single-source pin: the composed discovery lines derive FROM the
    /// harness constants — guards against re-hardcoding the order or env
    /// name in the UI copy (reorder/rename in code or copy breaks this).
    #[test]
    fn harness_guide_discovery_lines_single_source_from_harness_constants() {
        use crate::harness::pi_harness::{COMMON_HOME_SITES, PI_BINARY_ENV};
        let view = ProbeView::Report(rep("pi 1.2.3", "", Some("/x/pi"), true));
        let lines = harness_guide_lines(&view, Some("/h"));
        let orders: Vec<&GuideLine> = lines.iter().filter(|l| l.kind == K::Order).collect();
        assert_eq!(orders.len(), 2 + COMMON_HOME_SITES.len());
        assert!(
            orders[0].text.contains(PI_BINARY_ENV),
            "tier 1 must name {}: {}",
            PI_BINARY_ENV,
            orders[0].text
        );
        assert_eq!(
            orders[1].text,
            "2. pi in every PATH directory, in PATH order."
        );
        for (i, site) in COMMON_HOME_SITES.iter().enumerate() {
            assert_eq!(
                orders[i + 2].text,
                format!("{}. /h/{site}/pi", i + 3),
                "site #{i} drifted from COMMON_HOME_SITES"
            );
        }
        // The step offering the override also derives from the constant.
        let steps: Vec<&GuideLine> = lines.iter().filter(|l| l.kind == K::Step).collect();
        assert!(
            steps[1].text.contains(PI_BINARY_ENV),
            "step 2: {}",
            steps[1].text
        );
    }

    /// Runtime leg (headless stand-in for the manual smoke): opening the
    /// dialog spawns the detached probe, and the drain path used by
    /// `paint_harness_guide` observes the pending → report transition within
    /// the probe's ~12 s budget — no dialog reopen, no keyboard input. The
    /// assertions are host-agnostic (hold whether pi is installed or not).
    #[test]
    fn opened_dialog_flips_pending_to_report_within_probe_budget() {
        let root = tempdir("probe-flip");
        let proj = project_from(&root);
        let mut dlg = DlgSettings::from_project(&proj);
        assert!(
            matches!(dlg.probe_view, ProbeView::Pending),
            "fresh open is pending before the probe replies"
        );
        // Pump exactly like the painter: drain, then let the UI cadence
        // elapse. Bound generously past the ~12 s worst-case probe.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(25);
        loop {
            drain_probe(&mut dlg);
            if matches!(dlg.probe_view, ProbeView::Report(_)) {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "probe did not report within budget; view still: {:?}",
                dlg.probe_view
            );
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        match &dlg.probe_view {
            ProbeView::Report(r) => {
                // Shape invariants, identical to the probe_report pins.
                assert!(r.status.starts_with("pi "), "status: {}", r.status);
                if r.ok {
                    assert!(r.binary.is_some(), "ok requires a winning binary");
                    assert!(!r.status.contains("(unavailable"), "status: {}", r.status);
                } else {
                    assert!(r.status.contains("(unavailable"), "status: {}", r.status);
                }
                // On THIS host (pi provisioned per F-16) the live report is
                // the found-state line the operator would see on open.
                if r.ok {
                    println!(
                        "probe-reported live: {} | {}",
                        r.status,
                        r.binary
                            .as_deref()
                            .map(|p| p.display().to_string())
                            .unwrap_or_default()
                    );
                }
            }
            ProbeView::Pending => unreachable!("loop exits only on a report"),
        }
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

#[cfg(test)]
mod mcp_tests {
    //! F-18 dialog-field pins (pure; no egui): the load -> field mapping
    //! decides hint/warning, so unseen content is never destroyed silently.

    use crate::artifacts::mcp_io;

    /// First run: no file -> empty field, exemplar hint ACTIVE, no warning.
    #[test]
    fn mcp_fields_absent_file_opens_empty_with_hint_active() {
        let root =
            std::env::temp_dir().join(format!("packet_dlg_mcpabsent_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let st = mcp_io::load_state(&root);
        assert!(!st.present);
        let (text, hint, warning) = super::DlgMcp::dialog_fields(&st);
        assert_eq!(text, String::new());
        assert!(hint, "first run activates the exemplar hint");
        assert_eq!(warning, None);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Present file: content echoed BYTE-EXACT, hint OFF, no warning.
    #[test]
    fn mcp_fields_present_file_echoes_content_byte_exact() {
        let root =
            std::env::temp_dir().join(format!("packet_dlg_mcppresent_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join(".planner")).unwrap();
        let body = r#"{"k":1}"#;
        std::fs::write(root.join(".planner/mcp.json"), body).unwrap();
        let st = mcp_io::load_state(&root);
        assert!(st.present);
        let (text, hint, warning) = super::DlgMcp::dialog_fields(&st);
        assert_eq!(text, body, "editor seeds the exact stored bytes");
        assert!(!hint, "a real file suppresses the exemplar hint");
        assert_eq!(warning, None);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Present but UNREADABLE: field starts empty (hint off - there WAS a
    /// file) with the sticky warning demanding explicit consent to
    /// OVERWRITE unseen content.
    #[test]
    fn mcp_fields_unreadable_file_warns_explicit_overwrite_consent() {
        let st = mcp_io::McpLoadState {
            present: true,
            content: None,
            read_error: Some("Permission denied (os error 13)".into()),
        };
        let (text, hint, warning) = super::DlgMcp::dialog_fields(&st);
        assert_eq!(text, String::new(), "field must start empty, not guess");
        assert!(!hint, "unreadable is not absent: hint must not imply empty");
        let warning = warning.expect("sticky warning required for unreadable files");
        assert!(
            warning.contains("could not be read"),
            "warns why: {warning}"
        );
        assert!(
            warning.contains("Permission denied"),
            "quotes the error: {warning}"
        );
        assert!(
            warning.contains("OVERWRITE"),
            "explicit consent wording: {warning}"
        );
    }
}

#[cfg(test)]
mod ownership_picker_tests {
    use super::*;
    fn fixture() -> DlgSettings {
        DlgSettings {
            user_name: "Zach".into(),
            user_groups: "Platform, QA".into(),
            identity_note: "Current project identity".into(),
            rows: vec![
                Row {
                    category: "Product".into(),
                    members: String::new(),
                },
                Row {
                    category: "Engineering".into(),
                    members: "Morgan, platform, (owner TBD)".into(),
                },
            ],
            feedback: None,
            probe_rx: None,
            probe_view: ProbeView::Pending,
        }
    }
    #[test]
    fn suggestions_deduplicate_and_selection_preserves_custom_owners() {
        let dlg = fixture();
        assert_eq!(
            owner_choices(&dlg),
            vec!["Morgan", "Platform", "QA", "Zach"]
        );
        let mut members = "Custom team, Morgan".to_string();
        set_owner_selected(&mut members, "morgan", true);
        assert_eq!(members, "Custom team, Morgan");
        set_owner_selected(&mut members, "QA", true);
        set_owner_selected(&mut members, "MORGAN", false);
        assert_eq!(members, "Custom team, QA");
    }
    #[test]
    fn existing_owner_can_be_selected_in_the_modal() {
        let mut dlg = fixture();
        let ctx = egui::Context::default();
        fn frame(
            ctx: &egui::Context,
            dlg: &mut DlgSettings,
            events: Vec<egui::Event>,
        ) -> egui::FullOutput {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1280.0, 900.0),
                    )),
                    events,
                    ..Default::default()
                },
                |ui| {
                    crate::ui::overlays::show_modal(
                        ui,
                        true,
                        "Stakeholders & ownership",
                        660.0,
                        |ui| {
                            paint_settings_card(ui, dlg);
                        },
                    );
                },
            );
            // Egui paints duplicate-ID diagnostics into the frame when IDs collide.
            assert!(!output.shapes.iter().any(|s| matches!(&s.shape, egui::Shape::Text(t) if t.galley.text().contains("use of ScrollArea ID") || t.galley.text().contains("use of widget ID"))));
            output.textures_delta.clear();
            output
        }
        fn position(output: &egui::FullOutput, text: &str) -> egui::Pos2 {
            output
                .shapes
                .iter()
                .find_map(|s| match &s.shape {
                    egui::Shape::Text(t) if t.galley.text() == text => {
                        Some(t.pos + t.galley.mesh_bounds.center().to_vec2())
                    }
                    _ => None,
                })
                .expect(text)
        }
        fn click(ctx: &egui::Context, dlg: &mut DlgSettings, pos: egui::Pos2) {
            for pressed in [true, false] {
                frame(
                    ctx,
                    dlg,
                    vec![
                        egui::Event::PointerMoved(pos),
                        egui::Event::PointerButton {
                            pos,
                            button: egui::PointerButton::Primary,
                            pressed,
                            modifiers: Default::default(),
                        },
                    ],
                );
            }
        }
        frame(&ctx, &mut dlg, vec![]);
        let output = frame(&ctx, &mut dlg, vec![]);
        click(&ctx, &mut dlg, position(&output, "Select existing owners…"));
        frame(&ctx, &mut dlg, vec![]);
        let output = frame(&ctx, &mut dlg, vec![]);
        click(&ctx, &mut dlg, position(&output, "Morgan"));
        assert_eq!(dlg.rows[0].members, "Morgan");
        assert_eq!(dlg.rows[1].members, "Morgan, platform, (owner TBD)");
    }
}
