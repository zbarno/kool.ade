//! Loaded project state (SPECIFICATION.md §17 "Project State").
//!
//! This is the planner's single in-memory mirror of the durable artifacts.
//! Mutations happen only through `core::apply` after validation; the UI
//! reads this. Cloning is cheap (plain strings) and powers safe threading —
//! workers operate on snapshots, never on the UI's live instance.
//!
//! Seated operator (FR-13, D-14): the identity is derived from the connected
//! repository at every `load` (connect and each resync), with the tier order
//! verbatim from the approved specification: `user.name` preferred,
//! `user.email` fallback, the config's Current User block as tertiary
//! source, `(guest)` last resort. `effective_user()` always reflects that
//! derivation — the config block is a fallback override, not the primary
//! declaration.

use std::path::{Path, PathBuf};

use anyhow::anyhow;

use crate::artifacts::CONFIG_FILE;
#[cfg(test)]
use crate::artifacts::OPEN_ITEMS_FILE;
use crate::artifacts::config_io::{self, PlannerConfig};
use crate::artifacts::items_io;
use crate::artifacts::planning_store::PlanningStore;
use crate::artifacts::spec_doc;
use crate::domain::{OpenItem, ResolvedIdentity};

/// Snapshot of one connected project.
#[derive(Debug, Clone)]
pub struct PlannerState {
    pub repo_root: PathBuf,
    /// Shared planning data root; `repo_root` remains the code workspace.
    pub planning_store: PlanningStore,
    /// Repository display name (directory name).
    pub title: String,
    /// Current specification Markdown, `None` before it existed.
    pub spec_text: Option<String>,
    pub active_feature: Option<(String, String)>,
    /// All feature deltas that have not reached Implemented or Abandoned.
    /// `active_feature` remains the current workflow focus for compatibility.
    pub active_features: Vec<(String, String)>,
    pub repositories: crate::core::project_repos::ProjectManifest,
    /// Open-item queue (sorted per `items_io::sort_queue`).
    pub items: Vec<OpenItem>,
    /// Completed planning items remain available on the board with their outcomes.
    pub resolved_items: Vec<OpenItem>,
    /// Stakeholder/current-user configuration.
    pub config: PlannerConfig,
    /// Seated operator per FR-13 (git user.name → git user.email → config
    /// Current User block → `(guest)`), recomputed on every `load`/`resync`.
    pub identity: ResolvedIdentity,
    /// Serialized forms AT LOAD TIME — the write-diff baselines so a no-op
    /// turn never re-touches files.
    pub baseline_spec: Option<String>,
    pub baseline_items_md: String,
    pub baseline_planning_revision: String,
    pub workflow: crate::core::workflow::Workflow,
}

impl PlannerState {
    /// Read every artifact from disk into memory (strict: corrupt files are
    /// reported rather than guessed around — §16).
    pub fn load(repo: &Path) -> anyhow::Result<Self> {
        let store = PlanningStore::legacy_embedded(uuid::Uuid::nil(), repo);
        Self::load_with_store(repo, &store)
    }

    /// Load planning artifacts from the injected store while keeping Git
    /// identity and code-workspace operations on `repo`.
    pub fn load_with_store(repo: &Path, store: &PlanningStore) -> anyhow::Result<Self> {
        #[cfg(test)]
        crate::artifacts::product_docs::migrate_legacy_change_fixtures(store)?;
        let starting_revision = store.revision()?;
        crate::artifacts::product_docs::validate_change_metadata(store)?;
        let spec = spec_doc::load(store)?;
        let items_text = match store.read(crate::artifacts::planning_store::paths::OPEN_ITEMS) {
            Ok(bytes) => String::from_utf8(bytes)?,
            Err(crate::artifacts::planning_store::StoreError::Io { source, .. })
                if source.kind() == std::io::ErrorKind::NotFound =>
            {
                items_io::serialize(&[])
            }
            Err(error) => return Err(error.into()),
        };
        let items = items_io::parse(&items_text).map_err(|e| {
            anyhow!("open-items.md is unreadable to the planner: {e} (restore it with git checkout if needed)")
        })?;
        let baseline_items_md = items_io::serialize(&items);
        let config_text = match store.read(crate::artifacts::planning_store::paths::PROJECT_CONFIG)
        {
            Ok(bytes) => String::from_utf8(bytes)?,
            Err(crate::artifacts::planning_store::StoreError::Io { source, .. })
                if source.kind() == std::io::ErrorKind::NotFound =>
            {
                String::new()
            }
            Err(error) => return Err(error.into()),
        };
        let config = config_io::parse(&config_text)
            .map_err(|e| anyhow!("{CONFIG_FILE} failed to parse: {e}"))?;
        // FR-13: derive the seated operator from the connected repository's
        // git config, THEN the config block, THEN the guest. Probe failures
        // are None by design (§9: degraded but functional, never blocked).
        let git_name = crate::core::gitops::read_config(repo, "user.name");
        let git_email = crate::core::gitops::read_config(repo, "user.email");
        let identity = crate::domain::resolve_identity(
            git_name.as_deref(),
            git_email.as_deref(),
            config.user.as_ref(),
        );
        let title = repo
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "project".into());
        let workflow = crate::artifacts::task_docs::load_workflow(store)?;
        let active_features = crate::artifacts::product_docs::active_features(store);
        let active_feature =
            crate::artifacts::product_docs::active_feature_for_workflow(store, &workflow);
        let repositories =
            crate::core::project_repos::ProjectManifest::load_with_code_root(store, repo)?;
        let resolved_items =
            match store.read(crate::artifacts::planning_store::paths::RESOLVED_ITEMS) {
                Ok(bytes) => serde_json::from_slice(&bytes)?,
                Err(crate::artifacts::planning_store::StoreError::Io { source, .. })
                    if source.kind() == std::io::ErrorKind::NotFound =>
                {
                    Vec::new()
                }
                Err(error) => return Err(error.into()),
            };
        let ending_revision = store.revision()?;
        anyhow::ensure!(
            starting_revision == ending_revision,
            "planning store changed while project state was loading (expected revision {starting_revision}, found {ending_revision})"
        );
        Ok(Self {
            repo_root: repo.to_path_buf(),
            planning_store: store.clone(),
            title,
            baseline_spec: spec.clone(),
            spec_text: spec,
            active_feature,
            active_features,
            repositories,
            items,
            resolved_items,
            baseline_items_md,
            baseline_planning_revision: starting_revision,
            config,
            identity,
            workflow,
        })
    }

    /// Create any missing planning artifacts. Returns the relpaths created.
    pub fn bootstrap_missing(&mut self) -> anyhow::Result<Vec<String>> {
        let (mut created, product_revision) =
            crate::artifacts::migration::bootstrap_product_with_store_expected(
                &self.repo_root,
                &self.planning_store,
                &self.title,
                &self.baseline_planning_revision,
            )?;
        let spec_now = spec_doc::load(&self.planning_store)?.unwrap_or_default();
        self.baseline_spec = Some(spec_now.clone());
        self.spec_text = Some(spec_now);

        let mut seeded_config = None;
        let items_missing = matches!(
            self.planning_store
                .read(crate::artifacts::planning_store::paths::OPEN_ITEMS),
            Err(crate::artifacts::planning_store::StoreError::Io { source, .. })
                if source.kind() == std::io::ErrorKind::NotFound
        );
        let mut changes = Vec::new();
        if items_missing {
            changes.push((
                crate::artifacts::planning_store::paths::OPEN_ITEMS.to_owned(),
                self.baseline_items_md.as_bytes().to_vec(),
            ));
        }
        let manifest_missing = matches!(
            self.planning_store
                .read(crate::artifacts::planning_store::paths::PROJECT_MANIFEST),
            Err(crate::artifacts::planning_store::StoreError::Io { source, .. })
                if source.kind() == std::io::ErrorKind::NotFound
        );
        if manifest_missing {
            self.repositories.validate()?;
            changes.push((
                crate::artifacts::planning_store::paths::PROJECT_MANIFEST.to_owned(),
                serde_json::to_vec_pretty(&self.repositories)?,
            ));
        }
        let config_missing = matches!(
            self.planning_store
                .read(crate::artifacts::planning_store::paths::PROJECT_CONFIG),
            Err(crate::artifacts::planning_store::StoreError::Io { source, .. })
                if source.kind() == std::io::ErrorKind::NotFound
        );
        if config_missing {
            let seeded = seed_initial_config(&self.config);
            changes.push((
                crate::artifacts::planning_store::paths::PROJECT_CONFIG.to_owned(),
                seeded.as_bytes().to_vec(),
            ));
            let re_parsed = config_io::parse(&seeded).map_err(|e| anyhow::anyhow!("{e}"))?;
            seeded_config = Some(re_parsed);
        }
        let (paths, revision) = self
            .planning_store
            .transaction_with_revision(&changes, Some(&product_revision))?;
        created.extend(paths.iter().map(|path| self.planning_store.git_path(path)));
        self.baseline_planning_revision = revision;
        if let Some(config) = seeded_config {
            self.config = config;
        }
        Ok(created)
    }

    /// Content-addressed comparison of an in-memory snapshot against a fresh
    /// disk read. Returns the names of the planning surfaces that drifted
    /// (empty = still safe to apply snapshot-based writes). Writers call this
    /// INSIDE the [`crate::core::writer_gate`] section, immediately before
    /// applying, so a stale snapshot is refused rather than allowed to
    /// clobber a rival writer's newer commit.
    pub fn drift_report(previous: &Self, current: &Self) -> Vec<&'static str> {
        let mut changed = Vec::new();
        if previous.spec_text != current.spec_text {
            changed.push("specification");
        }
        if previous.items != current.items {
            changed.push("open items");
        }
        if previous.workflow != current.workflow {
            changed.push("workflow");
        }
        if previous.repositories != current.repositories {
            changed.push("repositories");
        }
        if previous.active_feature != current.active_feature {
            changed.push("active feature");
        }
        if previous.active_features != current.active_features {
            changed.push("active features");
        }
        if previous.resolved_items != current.resolved_items {
            changed.push("resolved items");
        }
        if previous.config != current.config {
            changed.push("configuration");
        }
        changed
    }

    /// Re-read artifacts from disk (e.g. user clicked Refresh after an
    /// external edit). Keeps baselines consistent with what is now on disk.
    pub fn resync(&mut self) -> anyhow::Result<()> {
        *self = Self::load_with_store(&self.repo_root, &self.planning_store)?;
        Ok(())
    }

    pub fn planning_contract(&self) -> Option<&str> {
        self.active_feature
            .as_ref()
            .map(|(_, body)| body.as_str())
            .or(self.spec_text.as_deref())
    }

    /// Effective current user: the seated operator (FR-13 — git-derived at
    /// load; the config block only acts when git yields nothing), else a
    /// neutral guest that can still be served `General` questions.
    pub fn effective_user(&self) -> crate::domain::CurrentUser {
        self.identity.user.clone()
    }
}

/// Seed a fresh config with the starter categories (§7) so the team sees
/// exactly where to write owners; no invented members.
fn seed_initial_config(existing: &PlannerConfig) -> String {
    let mut merged = existing.clone();
    if merged.stakeholders.entries.is_empty() {
        merged.stakeholders = crate::domain::Stakeholders::new(
            crate::domain::DEFAULT_CATEGORIES
                .iter()
                .map(|c| crate::domain::CategoryOwners::new((*c).to_string(), Vec::new()))
                .collect(),
        );
    }
    config_io::serialize(&merged)
}

#[cfg(test)]
mod tests;
