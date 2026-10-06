use super::*;

mod guide;
mod ownership;
mod painter;
mod repository_names;

#[cfg(test)]
pub(super) use guide::GuideLine;
pub(super) use guide::{GuideLineKind, ProbeReport, ProbeView, drain_probe, harness_guide_lines};
pub use painter::{paint_import_card, paint_settings_card};
pub use repository_names::RepositoryNameRow;

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
    pub repositories: Vec<RepositoryNameRow>,
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
            repositories: repository_names::rows(&proj.state.repositories),
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
        let manifest_changed = repository_names::persist(proj, &mut self.repositories)?;
        let previously_synthesized = crate::core::ownership::synthesize_for_state(&proj.state);
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
        let previous_config = match std::fs::read_to_string(&dest) {
            Ok(config) => Some(config),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => {
                return Err(AppError::Io {
                    op: "read previous workspace settings".into(),
                    detail: error.to_string(),
                });
            }
        };
        atomic_write(&dest, &md).map_err(|e| AppError::Io {
            op: "write .koolade-packet/config/project.md".into(),
            detail: e.to_string(),
        })?;
        if let Err(error) = proj.state.resync() {
            return Err(ownership::rollback_config_update(
                proj,
                &dest,
                previous_config.as_deref(),
                &error.to_string(),
            ));
        }
        let mut changed_paths = vec![CONFIG_FILE.to_string()];
        if manifest_changed {
            changed_paths.push(crate::artifacts::layout::canonical::PROJECT_MANIFEST.into());
        }
        if ownership::persist_assigned_resolutions(
            proj,
            previously_synthesized,
            previous_config,
            &dest,
        )? {
            changed_paths.push(crate::artifacts::layout::canonical::OPEN_ITEMS.into());
            changed_paths.push(crate::artifacts::layout::canonical::RESOLVED_ITEMS.into());
        }
        let sha = gitops::commit(
            &proj.state.repo_root,
            "settings: update workspace settings",
            &changed_paths,
        )?;
        proj.refresh_git();
        Ok(sha.chars().take(7).collect())
    }
}

// ---------------------------------------------------------------------------
// Painting (returns frame signals; no hidden global state)
// ---------------------------------------------------------------------------

/// Suggestions come from the current seat, its teams and existing category owners.
pub(super) fn owner_choices(dlg: &DlgSettings) -> Vec<String> {
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

pub(super) fn set_owner_selected(members: &mut String, owner: &str, selected: bool) {
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
    ui.separator();
    ui.label(RichText::new("OpenAI Codex CLI").size(12.0).strong());
    ui.label(
        RichText::new(format!(
            "Set {}=codex before launching Kool.ad/e to use Codex. Optional model: {}.",
            crate::harness::CODEX_HARNESS_ENV,
            crate::harness::codex_harness::CODEX_MODEL_ENV
        ))
        .size(11.0)
        .weak(),
    );
}
