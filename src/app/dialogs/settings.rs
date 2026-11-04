use super::*;

mod painter;
mod project;
mod repository_names;

pub use painter::{paint_import_card, paint_settings_card};
pub use project::DlgProjectSettings;
pub use project::paint_project_settings_card;
pub use repository_names::RepositoryNameRow;

// Local human identity settings
// ---------------------------------------------------------------------------

#[derive(Clone, Default)]
pub struct DlgSettings {
    pub user_name: String,
    /// Provenance line rendered under "Who am I?": the seated (DERIVED)
    /// identity per FR-13, labelled with how the seat was obtained. When the
    /// seat came from the config block or fell through to guest, the line
    /// also identifies this as a local application setting.
    pub identity_note: String,
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
            identity_note.push_str("; stored for this local installation");
        }
        let cfg = &proj.state.config;
        let user_name = if proj.state.identity.source == IdentitySource::Guest {
            cfg.user
                .as_ref()
                .map(|user| user.name.trim().to_string())
                .unwrap_or_default()
        } else {
            seated.name.clone()
        };
        Self {
            user_name,
            identity_note,
            feedback: None,
        }
    }

    /// Save the local human identity setting.
    pub fn apply(&mut self, proj: &mut Project) -> Result<String, AppError> {
        // Writer section: serialize the private settings transaction.
        let _guard = crate::core::writer_gate::acquire();
        let user = CurrentUser::new(self.user_name.trim());
        let md = config_io::serialize(&config_io::PlannerConfig { user: Some(user) });
        let store = proj.state.planning_store.clone();
        let config = config_io::parse(&md).map_err(AppError::Other)?;
        let mut candidate = proj.state.clone();
        candidate.config = config;
        let git_name = gitops::read_config(&candidate.repo_root, "user.name");
        let git_email = gitops::read_config(&candidate.repo_root, "user.email");
        candidate.identity = crate::domain::resolve_identity(
            git_name.as_deref(),
            git_email.as_deref(),
            candidate.config.user.as_ref(),
        );
        let changes = vec![(
            crate::artifacts::planning_store::paths::PROJECT_CONFIG.to_owned(),
            md.into_bytes(),
        )];
        let (paths, revision) = store
            .transaction_with_revision_and_record_revisions(
                &changes,
                Some(&proj.state.baseline_planning_revision),
                &[],
            )
            .map_err(|error| AppError::Other(format!("workspace settings save failed: {error}")))?;
        candidate.baseline_planning_revision.clone_from(&revision);
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
        if paths.is_empty() {
            return Ok(String::new());
        }
        Ok(revision
            .strip_prefix("sha256:")
            .unwrap_or(&revision)
            .chars()
            .take(7)
            .collect())
    }
}
