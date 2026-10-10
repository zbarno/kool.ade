use super::*;

mod ownership;
mod painter;
mod project;
mod repository_names;

pub use painter::{paint_import_card, paint_settings_card};
pub use project::DlgProjectSettings;
pub use project::paint_project_settings_card;
pub use repository_names::RepositoryNameRow;

// Settings dialog (current user + stakeholder categories)
// ---------------------------------------------------------------------------

#[derive(Clone, Default)]
pub struct Row {
    pub category: String,
    pub members: String,
}

#[derive(Clone, Default)]
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
        }
    }

    /// Commit the edited roster: rewrite config.md, resync, checkpoint.
    pub fn apply(&mut self, proj: &mut Project) -> Result<String, AppError> {
        // Writer section: config write + checkpoint share the index.
        let _guard = crate::core::writer_gate::acquire();
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
        let store = proj.state.planning_store.clone();
        let config = config_io::parse(&md).map_err(AppError::Other)?;
        let (mut candidate, resolutions_changed) = ownership::apply_config_and_resolve_ownership(
            &proj.state,
            previously_synthesized,
            config,
        )
        .map_err(|error| AppError::Other(error.to_string()))?;
        let mut changes = vec![(
            crate::artifacts::planning_store::paths::PROJECT_CONFIG.to_owned(),
            md.into_bytes(),
        )];
        let mut record_checks = Vec::new();
        if resolutions_changed {
            let (item_changes, checks, _) = crate::artifacts::items_io::record_changes(
                &store,
                &candidate.items,
                &candidate.resolved_items,
            )
            .map_err(|error| AppError::Other(error.to_string()))?;
            changes.extend(item_changes);
            record_checks = checks;
        }
        let (paths, revision) = store
            .transaction_with_revision_and_record_revisions(
                &changes,
                Some(&proj.state.baseline_planning_revision),
                &record_checks,
            )
            .map_err(|error| AppError::Other(format!("workspace settings save failed: {error}")))?;
        candidate.baseline_planning_revision = revision;
        let (items, resolved_items, _) =
            crate::artifacts::items_io::load_store(&store).map_err(|error| {
                AppError::Other(format!(
                    "settings saved but items could not reload: {error}"
                ))
            })?;
        candidate.items = items;
        candidate.resolved_items = resolved_items;
        candidate.baseline_items_md = crate::artifacts::items_io::serialize(&candidate.items);
        proj.state = candidate;
        let changed_paths = paths
            .iter()
            .map(|path| store.git_path(path))
            .collect::<Vec<_>>();
        if changed_paths.is_empty() {
            return Ok(String::new());
        }
        let sha = gitops::commit(
            &store.git_root(),
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
