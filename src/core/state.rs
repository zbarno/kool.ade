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

use crate::artifacts::config_io::{self, PlannerConfig};
use crate::artifacts::items_io;
use crate::artifacts::spec_doc;
use crate::artifacts::{CONFIG_FILE, OPEN_ITEMS_FILE};
use crate::domain::{OpenItem, ResolvedIdentity};

/// Snapshot of one connected project.
#[derive(Debug, Clone)]
pub struct PlannerState {
    pub repo_root: PathBuf,
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
    pub workflow: crate::core::workflow::Workflow,
}

impl PlannerState {
    /// Read every artifact from disk into memory (strict: corrupt files are
    /// reported rather than guessed around — §16).
    pub fn load(repo: &Path) -> anyhow::Result<Self> {
        let spec = spec_doc::load(repo)?;
        let items_text = match crate::artifacts::read_utf8_lossy(&crate::artifacts::repo_artifact(
            repo,
            OPEN_ITEMS_FILE,
        )) {
            Ok(t) => t,
            Err(_) => items_io::serialize(&[]),
        };
        let items = items_io::parse(&items_text).map_err(|e| {
            anyhow!("open-items.md is unreadable to the planner: {e} (restore it with git checkout if needed)")
        })?;
        let baseline_items_md = items_io::serialize(&items);
        let config_text =
            crate::artifacts::read_utf8_lossy(&crate::artifacts::repo_artifact(repo, CONFIG_FILE))
                .unwrap_or_default();
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
        Ok(Self {
            repo_root: repo.to_path_buf(),
            title,
            baseline_spec: spec.clone(),
            spec_text: spec,
            active_feature: crate::artifacts::product_docs::active_feature(repo),
            active_features: crate::artifacts::product_docs::active_features(repo),
            repositories: crate::core::project_repos::ProjectManifest::load(repo)?,
            items,
            resolved_items: match std::fs::read(
                crate::artifacts::layout::ArtifactLayout::new(repo).resolved_items(),
            ) {
                Ok(bytes) => serde_json::from_slice(&bytes)?,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Vec::new(),
                Err(error) => return Err(error.into()),
            },
            baseline_items_md,
            config,
            identity,
            workflow: crate::artifacts::task_docs::load_workflow(repo)?,
        })
    }

    /// Create any missing planning artifacts. Returns the relpaths created.
    pub fn bootstrap_missing(&mut self) -> anyhow::Result<Vec<String>> {
        let mut created =
            crate::artifacts::migration::bootstrap_product(&self.repo_root, &self.title)?;
        let spec_now = spec_doc::load(&self.repo_root)?.unwrap_or_default();
        self.baseline_spec = Some(spec_now.clone());
        self.spec_text = Some(spec_now);

        let items_path = crate::artifacts::repo_artifact(&self.repo_root, OPEN_ITEMS_FILE);
        if !items_path.exists() {
            crate::artifacts::atomic_write(&items_path, &self.baseline_items_md.clone())?;
            created.push(OPEN_ITEMS_FILE.to_owned());
        }
        let cfg_path = crate::artifacts::repo_artifact(&self.repo_root, CONFIG_FILE);
        if !cfg_path.exists() {
            let seeded = seed_initial_config(&self.config);
            crate::artifacts::atomic_write(&cfg_path, &seeded)?;
            let re_parsed = config_io::parse(&seeded).map_err(|e| anyhow::anyhow!("{e}"))?;
            self.config = re_parsed;
            created.push(CONFIG_FILE.to_owned());
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
        *self = Self::load(&self.repo_root)?;
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
mod tests {
    use super::*;
    use crate::core::gitops;
    use crate::domain::{CurrentUser, GUEST_NAME, IdentitySource};

    fn mkrepo(prefix: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("packet_state_{prefix}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    /// Git working tree with (optionally) a local identity plus a seeded
    /// Canonical project config's Current User block — mirrors the
    /// `gitops::tests` temp-repo recipe.
    fn git_fixture(prefix: &str, local_name: Option<&str>) -> PathBuf {
        let p = mkrepo(prefix);
        let git = |args: &[&str]| {
            let out = std::process::Command::new("git")
                .arg("-C")
                .arg(&p)
                .args(args)
                .output()
                .unwrap();
            assert!(
                out.status.success(),
                "git {:?} failed: {}",
                args,
                String::from_utf8_lossy(&out.stderr)
            );
        };
        git(&["init", "-q", "-b", "main"]);
        if let Some(name) = local_name {
            git(&["config", "user.name", name]);
        }
        git(&["config", "user.email", "zbarno@gmail.com"]);
        let dest = crate::artifacts::repo_artifact(&p, CONFIG_FILE);
        std::fs::create_dir_all(dest.parent().unwrap()).unwrap();
        std::fs::write(
            &dest,
            "# Planner Configuration\n\n## Current User\nName: Bob\nGroups: Ops\n\n## Stakeholders\n\n### QA\n(no owner configured)\n",
        )
        .unwrap();
        p
    }

    fn config_path(p: &Path) -> PathBuf {
        crate::artifacts::repo_artifact(p, CONFIG_FILE)
    }

    #[test]
    fn bootstraps_skeletons_and_reports_creations() {
        let repo = mkrepo("boot");
        let mut st = PlannerState::load(&repo).unwrap();
        let created = st.bootstrap_missing().unwrap();
        assert!(created.contains(&crate::artifacts::SPEC_FILE.to_owned()));
        assert!(created.contains(&OPEN_ITEMS_FILE.to_owned()));
        assert!(created.contains(&CONFIG_FILE.to_owned()));
        assert_eq!(created.len(), 10);
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
        let text =
            crate::artifacts::read_utf8_lossy(&crate::artifacts::repo_artifact(&repo, CONFIG_FILE))
                .unwrap();
        assert!(text.contains("Name: Sam"));
        assert!(text.contains("### InfoSec"));
        let _ = std::fs::remove_dir_all(&repo);
    }

    /// Full FR-13 descent under a shielded ambient hierarchy (the dev
    /// machine's GLOBAL config carries its own identity and must not leak
    /// into fixture expectations).
    #[test]
    fn git_identity_seats_operator_and_survives_resync_descent() {
        let _shield = gitops::test_support::shield("state-prio");
        let repo = git_fixture("prio", Some("Zachary Barno"));
        let block_before = std::fs::read_to_string(config_path(&repo)).unwrap();

        let mut st = PlannerState::load(&repo).unwrap();
        // git user.name wins over the contradicting config block…
        assert_eq!(st.identity.user.name, "Zachary Barno");
        assert_eq!(st.identity.source, IdentitySource::GitUserName);
        assert_eq!(st.effective_user().name, "Zachary Barno");
        // …while config still contributes the groups git cannot express.
        assert_eq!(st.effective_user().groups, vec!["Ops".to_string()]);
        // …and never writes to the config block on the way.
        assert_eq!(
            std::fs::read_to_string(config_path(&repo)).unwrap(),
            block_before,
            "identity probing must leave config.md byte-intact"
        );

        // Rewriting the block name demotes nothing: git still outranks it.
        let p = config_path(&repo);
        std::fs::write(&p, block_before.replace("Name: Bob", "Name: Carol")).unwrap();
        st.resync().unwrap();
        assert_eq!(st.identity.user.name, "Zachary Barno");
        assert_eq!(st.identity.source, IdentitySource::GitUserName);

        // Unset name (ambient shadowed by the shield) → email fallback.
        let _ = std::process::Command::new("git")
            .arg("-C")
            .arg(&repo)
            .args(["config", "--unset", "user.name"])
            .output()
            .unwrap();
        st.resync().unwrap();
        assert_eq!(st.identity.user.name, "zbarno@gmail.com");
        assert_eq!(st.identity.source, IdentitySource::GitUserEmail);
        assert_eq!(st.effective_user().groups, vec!["Ops".to_string()]);

        // Unset email too → the config block finally seats the operator.
        let _ = std::process::Command::new("git")
            .arg("-C")
            .arg(&repo)
            .args(["config", "--unset", "user.email"])
            .output()
            .unwrap();
        st.resync().unwrap();
        assert_eq!(st.identity.user.name, "Carol");
        assert_eq!(st.identity.source, IdentitySource::ConfigBlock);
        assert_eq!(st.effective_user().groups, vec!["Ops".to_string()]);

        // An external edit of the local git user.name takes effect on the
        // next resync — derivation is not one-shot at connect.
        let _ = std::process::Command::new("git")
            .arg("-C")
            .arg(&repo)
            .args(["config", "user.name", "Mira Chen"])
            .output()
            .unwrap();
        st.resync().unwrap();
        assert_eq!(st.identity.user.name, "Mira Chen");
        assert_eq!(st.identity.source, IdentitySource::GitUserName);
        let _ = std::fs::remove_dir_all(&repo);
    }

    /// Degradation contract (§9): a non-git tree raises NO error from the
    /// identity probes and simply falls to config-else-guest.
    #[test]
    fn gitless_tree_degrades_to_config_block_then_guest() {
        // Config block only (no .git anywhere) → ConfigBlock seat.
        let repo = mkrepo("cfgonly");
        let config = crate::artifacts::layout::ArtifactLayout::new(&repo).config_root();
        std::fs::create_dir_all(&config).unwrap();
        std::fs::write(
            config.join("project.md"),
            "# Planner Configuration\n\n## Current User\nName: Dana\nGroups: Ops, Platform\n\n## Stakeholders\n\n### QA\n(no owner configured)\n",
        ).unwrap();
        let st = PlannerState::load(&repo).unwrap();
        assert_eq!(st.identity.user.name, "Dana");
        assert_eq!(
            st.identity.user.groups,
            vec!["Ops".to_string(), "Platform".to_string()]
        );
        assert_eq!(st.identity.source, IdentitySource::ConfigBlock);
        assert_eq!(st.effective_user().name, "Dana");
        let _ = std::fs::remove_dir_all(&repo);

        // No git, no artifacts → guest, and load STILL SUCCEEDS.
        let bare = mkrepo("guest");
        let st = PlannerState::load(&bare).unwrap();
        assert_eq!(st.identity.user.name, GUEST_NAME);
        assert_eq!(st.identity.source, IdentitySource::Guest);
        assert_eq!(st.effective_user().name, GUEST_NAME);
        let _ = std::fs::remove_dir_all(&bare);
    }

    /// AC2 (literal): `user.name` unset (ambient shadowed by the shield),
    /// `user.email` set, and a config block that lists groups — the EMAIL
    /// seats the operator and the groups come exclusively from the block.
    #[test]
    fn email_fallback_seats_when_name_unset_and_ambient_shadowed() {
        let _shield = gitops::test_support::shield("state-ac2");
        let repo = git_fixture("ac2", None); // local identity: email only
        let _ = std::process::Command::new("git")
            .arg("-C")
            .arg(&repo)
            .args(["config", "user.email", "eve@example.org"])
            .output()
            .unwrap();
        let st = PlannerState::load(&repo).unwrap();
        assert_eq!(st.identity.user.name, "eve@example.org");
        assert_eq!(st.identity.source, IdentitySource::GitUserEmail);
        assert_eq!(st.effective_user().name, "eve@example.org");
        assert_eq!(st.effective_user().groups, vec!["Ops".to_string()]);
        let _ = std::fs::remove_dir_all(&repo);
    }

    /// AC1-shaped fixture: local git identity 'Ada Lovelace' plus a
    /// contradicting 'Bob' block — git wins, block byte-intact.
    #[test]
    fn ada_lovelace_git_beats_bob_config() {
        let _shield = gitops::test_support::shield("state-ada");
        let repo = git_fixture("ada", Some("Ada Lovelace"));
        let _ = std::process::Command::new("git")
            .arg("-C")
            .arg(&repo)
            .args(["config", "user.email", "ada@example.org"])
            .output()
            .unwrap();
        let block = std::fs::read_to_string(config_path(&repo)).unwrap();
        let st = PlannerState::load(&repo).unwrap();
        assert_eq!(st.identity.user.name, "Ada Lovelace");
        assert_eq!(st.identity.source, IdentitySource::GitUserName);
        assert_eq!(st.effective_user().name, "Ada Lovelace");
        assert_eq!(st.config.user.as_ref().unwrap().name, "Bob");
        assert_eq!(std::fs::read_to_string(config_path(&repo)).unwrap(), block);
        let _ = std::fs::remove_dir_all(&repo);
    }
}
