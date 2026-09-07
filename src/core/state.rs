//! Loaded project state (SPECIFICATION.md §17 "Project State").
//!
//! This is the planner's single in-memory mirror of the durable artifacts.
//! Mutations happen only through `core::apply` after validation; the UI
//! reads this. Cloning is cheap (plain strings) and powers safe threading —
//! workers operate on snapshots, never on the UI's live instance.

use std::path::{Path, PathBuf};

use anyhow::anyhow;

use crate::artifacts::config_io::{self, PlannerConfig};
use crate::artifacts::items_io;
use crate::artifacts::spec_doc;
use crate::domain::OpenItem;
use crate::artifacts::{CONFIG_FILE, OPEN_ITEMS_FILE, SPEC_FILE};

/// Snapshot of one connected project.
#[derive(Debug, Clone)]
pub struct PlannerState {
    pub repo_root: PathBuf,
    /// Repository display name (directory name).
    pub title: String,
    /// Current specification Markdown, `None` before it existed.
    pub spec_text: Option<String>,
    /// Open-item queue (sorted per `items_io::sort_queue`).
    pub items: Vec<OpenItem>,
    /// Stakeholder/current-user configuration.
    pub config: PlannerConfig,
    /// Serialized forms AT LOAD TIME — the write-diff baselines so a no-op
    /// turn never re-touches files.
    pub baseline_spec: Option<String>,
    pub baseline_items_md: String,
}

impl PlannerState {
    /// Read every artifact from disk into memory (strict: corrupt files are
    /// reported rather than guessed around — §16).
    pub fn load(repo: &Path) -> anyhow::Result<Self> {
        let spec = spec_doc::load(repo)?;
        let items_text = match crate::artifacts::read_utf8_lossy(&crate::artifacts::repo_artifact(repo, OPEN_ITEMS_FILE)) {
            Ok(t) => t,
            Err(_) => items_io::serialize(&[]),
        };
        let items = items_io::parse(&items_text).map_err(|e| {
            anyhow!("open-items.md is unreadable to the planner: {e} (restore it with git checkout if needed)")
        })?;
        let baseline_items_md = items_io::serialize(&items);
        let config_text = match crate::artifacts::read_utf8_lossy(&crate::artifacts::repo_artifact(repo, CONFIG_FILE)) {
            Ok(t) => t,
            Err(_) => String::new(),
        };
        let config = config_io::parse(&config_text).map_err(|e| {
            anyhow!("{CONFIG_FILE} failed to parse: {e}")
        })?;
        let title = repo
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "project".into());
        Ok(Self {
            repo_root: repo.to_path_buf(),
            title,
            baseline_spec: spec.clone(),
            spec_text: spec,
            items,
            baseline_items_md,
            config,
        })
    }

    /// Create any missing planning artifacts. Returns the relpaths created.
    pub fn bootstrap_missing(&mut self) -> anyhow::Result<Vec<&'static str>> {
        let mut created = Vec::new();
        if spec_doc::ensure(&self.repo_root, &self.title)? {
            created.push(SPEC_FILE);
        }
        let spec_now = spec_doc::load(&self.repo_root)?.unwrap_or_default();
        self.baseline_spec = Some(spec_now.clone());
        self.spec_text = Some(spec_now);

        let items_path = crate::artifacts::repo_artifact(&self.repo_root, OPEN_ITEMS_FILE);
        if !items_path.exists() {
            crate::artifacts::atomic_write(&items_path, &self.baseline_items_md.clone())?;
            created.push(OPEN_ITEMS_FILE);
        }
        let cfg_path = crate::artifacts::repo_artifact(&self.repo_root, CONFIG_FILE);
        if !cfg_path.exists() {
            let seeded = seed_initial_config(&self.config);
            crate::artifacts::atomic_write(&cfg_path, &seeded)?;
            let re_parsed = config_io::parse(&seeded).map_err(|e| anyhow::anyhow!("{e}"))?;
            self.config = re_parsed;
            created.push(CONFIG_FILE);
        }
        Ok(created)
    }

    /// Re-read artifacts from disk (e.g. user clicked Refresh after an
    /// external edit). Keeps baselines consistent with what is now on disk.
    pub fn resync(&mut self) -> anyhow::Result<()> {
        *self = Self::load(&self.repo_root)?;
        Ok(())
    }

    /// Effective current user: configured identity, else a neutral guest that
    /// can still be served `General` questions.
    pub fn effective_user(&self) -> crate::domain::CurrentUser {
        self.config
            .user
            .clone()
            .unwrap_or_else(|| crate::domain::CurrentUser::new("(guest)", Vec::new()))
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
mod tests {
    use super::*;
    use crate::domain::CurrentUser;

    fn mkrepo(prefix: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("packet_state_{prefix}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn bootstraps_skeletons_and_reports_creations() {
        let repo = mkrepo("boot");
        let mut st = PlannerState::load(&repo).unwrap();
        let created = st.bootstrap_missing().unwrap();
        assert_eq!(created, vec![SPEC_FILE, OPEN_ITEMS_FILE, CONFIG_FILE]);
        // Second call creates nothing.
        let created2 = st.bootstrap_missing().unwrap();
        assert!(created2.is_empty());
        let _ = std::fs::remove_dir_all(&repo);
    }

    #[test]
    fn seeded_config_contains_starter_categories() {
        let repo = mkrepo("seed");
        let mut st = PlannerState::load(&repo).unwrap();
        st.config.user = Some(CurrentUser::new("Sam", vec!["QA".into()]));
        st.bootstrap_missing().unwrap();
        let text = crate::artifacts::read_utf8_lossy(&crate::artifacts::repo_artifact(
            &repo,
            CONFIG_FILE,
        ))
        .unwrap();
        assert!(text.contains("Name: Sam"));
        assert!(text.contains("### InfoSec"));
        let _ = std::fs::remove_dir_all(&repo);
    }
}
