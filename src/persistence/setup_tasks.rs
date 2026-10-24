//! Durable, machine-local bootstrap task observations.
//!
//! These are NOT shared repository planning records. No stored status or
//! model claim can authorize a host mutation or bypass an execution gate.

use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

use serde::{Deserialize, Serialize};

use crate::app::setup_attention::catalog::{
    FindingStatus, MachineSnapshot, SCHEMA_VERSION as SNAPSHOT_VERSION,
};

const STORE_VERSION: u8 = 1;
const MAX_STATE_BYTES: u64 = 2 * 1024 * 1024;
const MAX_TASKS: usize = 128;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    NeedsAttention,
    BlockedByPrerequisite,
    Unverified,
    Unsupported,
    Verified,
    Interrupted,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct LocalTask {
    pub id: String,
    pub title: String,
    pub explanation: String,
    pub next_step: String,
    pub prerequisites: Vec<String>,
    pub state: State,
    pub observed_at_unix_ms: u64,
    pub verified_at_unix_ms: Option<u64>,
    /// This is a bounded audit *observation* ledger, never a replay queue.
    pub events: Vec<TaskEvent>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct TaskEvent {
    pub observed_at_unix_ms: u64,
    pub state: State,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Store {
    pub schema_version: u8,
    pub tasks: BTreeMap<String, LocalTask>,
}

impl Default for Store {
    fn default() -> Self {
        Self {
            schema_version: STORE_VERSION,
            tasks: BTreeMap::new(),
        }
    }
}

impl Store {
    /// Reconcile an independently probed snapshot. Stale/unknown domains
    /// never verify absent tasks. No setup model response is accepted here.
    pub fn reconcile(&mut self, snapshot: &MachineSnapshot) -> anyhow::Result<bool> {
        anyhow::ensure!(
            snapshot.schema_version == SNAPSHOT_VERSION,
            "unsupported machine readiness snapshot version"
        );
        anyhow::ensure!(
            snapshot.findings.len() <= MAX_TASKS,
            "too many setup findings"
        );
        // A late worker result must not roll back a fresher readiness result.
        anyhow::ensure!(
            self.tasks
                .values()
                .all(|task| task.observed_at_unix_ms <= snapshot.checked_at_unix_ms),
            "stale machine setup observation"
        );

        let mut changed = false;
        let mut seen = BTreeSet::new();
        for finding in &snapshot.findings {
            anyhow::ensure!(valid_id(&finding.id), "invalid setup prerequisite ID");
            anyhow::ensure!(
                seen.insert(finding.id.clone()),
                "duplicate setup prerequisite ID"
            );
            let state = match finding.status {
                FindingStatus::NeedsAttention => State::NeedsAttention,
                FindingStatus::BlockedByPrerequisite => State::BlockedByPrerequisite,
                FindingStatus::Unverified => State::Unverified,
                FindingStatus::Unsupported => State::Unsupported,
            };
            if let Some(task) = self.tasks.get_mut(&finding.id) {
                let meaningful = task.state != state
                    || task.title != finding.title
                    || task.explanation != finding.explanation
                    || task.next_step != finding.next_step
                    || task.prerequisites != finding.prerequisites;
                task.state = state;
                task.title.clone_from(&finding.title);
                task.explanation.clone_from(&finding.explanation);
                task.next_step.clone_from(&finding.next_step);
                task.prerequisites.clone_from(&finding.prerequisites);
                task.observed_at_unix_ms = snapshot.checked_at_unix_ms;
                task.verified_at_unix_ms = None;
                if meaningful {
                    record(task, snapshot.checked_at_unix_ms);
                    changed = true;
                }
            } else {
                let mut task = LocalTask {
                    id: finding.id.clone(),
                    title: finding.title.clone(),
                    explanation: finding.explanation.clone(),
                    next_step: finding.next_step.clone(),
                    prerequisites: finding.prerequisites.clone(),
                    state,
                    observed_at_unix_ms: snapshot.checked_at_unix_ms,
                    verified_at_unix_ms: None,
                    events: Vec::new(),
                };
                record(&mut task, snapshot.checked_at_unix_ms);
                self.tasks.insert(finding.id.clone(), task);
                changed = true;
            }
        }

        // An item disappearing is positive evidence only when its domain
        // was independently rechecked without a failed/unknown probe.
        for task in self.tasks.values_mut() {
            if !seen.contains(&task.id)
                && task_has_positive_evidence(&task.id, snapshot)
                && task.state != State::Verified
            {
                task.state = State::Verified;
                task.verified_at_unix_ms = Some(snapshot.checked_at_unix_ms);
                task.observed_at_unix_ms = snapshot.checked_at_unix_ms;
                record(task, snapshot.checked_at_unix_ms);
                changed = true;
            }
        }
        anyhow::ensure!(
            self.tasks.len() <= MAX_TASKS,
            "setup task store reached its size limit"
        );
        Ok(changed)
    }
}

/// Check the exact prerequisite with authoritative *positive* evidence.
/// A different error replacing the old one is not a successful repair.
fn task_has_positive_evidence(id: &str, snapshot: &MachineSnapshot) -> bool {
    let evidence = match id.split(':').collect::<Vec<_>>().as_slice() {
        ["setup", "cli", "install"] => "cli:install".to_owned(),
        ["setup", "cli", "scan"] => "cli:scan".to_owned(),
        ["setup", "cli", "select"] => "cli:select".to_owned(),
        ["setup", "cli", provider, _] => format!("cli:{provider}"),
        ["setup", "docker", _] => "docker".to_owned(),
        ["setup", "images", ..] => "images".to_owned(),
        _ => return false,
    };
    snapshot
        .checked_domains
        .iter()
        .any(|domain| domain == &evidence)
}

fn record(task: &mut LocalTask, at: u64) {
    const MAX_EVENTS: usize = 24;
    task.events.push(TaskEvent {
        observed_at_unix_ms: at,
        state: task.state,
    });
    if task.events.len() > MAX_EVENTS {
        task.events.drain(..task.events.len() - MAX_EVENTS);
    }
}

fn valid_id(id: &str) -> bool {
    id.len() <= 180
        && id.starts_with("setup:")
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b':' | b'-' | b'_'))
}

pub fn path() -> std::path::PathBuf {
    crate::persistence::state_root()
        .join("machine-setup")
        .join("tasks.json")
}

pub fn load() -> anyhow::Result<Store> {
    load_from(&path())
}

fn load_from(path: &Path) -> anyhow::Result<Store> {
    let bytes = match std::fs::metadata(path) {
        Ok(metadata) => {
            anyhow::ensure!(
                metadata.len() <= MAX_STATE_BYTES,
                "setup task state is oversized"
            );
            std::fs::read(path)?
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Store::default()),
        Err(error) => return Err(error.into()),
    };
    let store: Store = serde_json::from_slice(&bytes)?;
    anyhow::ensure!(
        store.schema_version == STORE_VERSION,
        "unsupported setup store schema"
    );
    anyhow::ensure!(store.tasks.len() <= MAX_TASKS, "oversized setup task store");
    for (key, task) in &store.tasks {
        anyhow::ensure!(
            valid_id(key) && key == &task.id,
            "invalid saved setup task ID"
        );
    }
    Ok(store)
}

pub fn save(store: &Store) -> anyhow::Result<()> {
    save_to(&path(), store)
}

fn save_to(path: &Path, store: &Store) -> anyhow::Result<()> {
    anyhow::ensure!(
        store.schema_version == STORE_VERSION,
        "unsupported setup store schema"
    );
    let parent = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("missing setup state directory"))?;
    if let Ok(metadata) = std::fs::symlink_metadata(parent) {
        anyhow::ensure!(
            metadata.is_dir() && !metadata.file_type().is_symlink(),
            "unsafe setup state directory"
        );
    }
    std::fs::create_dir_all(parent)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700))?;
    }
    let payload = serde_json::to_string_pretty(store)?;
    anyhow::ensure!(
        payload.len() <= MAX_STATE_BYTES as usize,
        "setup task state is oversized"
    );
    crate::artifacts::atomic_write(path, &payload)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

/// Reading a corrupt/newer store must fail without overwriting it. No user
/// approval, secret, or command is recorded here, only diagnostic task state.
pub fn reconcile_live(snapshot: &MachineSnapshot) -> anyhow::Result<Store> {
    let mut store = load()?;
    if store.reconcile(snapshot)? {
        save(&store)?;
    }
    Ok(store)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::setup_attention::catalog::{Finding, FindingStatus, HostKind};

    fn snapshot(ids: &[(&str, FindingStatus)], domains: &[&str], at: u64) -> MachineSnapshot {
        MachineSnapshot {
            schema_version: SNAPSHOT_VERSION,
            host: HostKind::Linux,
            checked_at_unix_ms: at,
            findings: ids
                .iter()
                .map(|(id, status)| Finding {
                    id: (*id).into(),
                    title: (*id).into(),
                    explanation: "safe observation".into(),
                    next_step: "retry".into(),
                    status: *status,
                    prerequisites: Vec::new(),
                })
                .collect(),
            checked_domains: domains.iter().map(|s| (*s).into()).collect(),
        }
    }

    #[test]
    fn two_faults_are_distinct_and_repeated_scan_deduplicates() {
        let s = snapshot(
            &[
                ("setup:cli:codex:auth", FindingStatus::NeedsAttention),
                ("setup:docker:rootless", FindingStatus::NeedsAttention),
            ],
            &["cli", "docker"],
            10,
        );
        let mut store = Store::default();
        assert!(store.reconcile(&s).unwrap());
        assert_eq!(store.tasks.len(), 2);
        assert!(!store.reconcile(&s).unwrap());
        assert_eq!(store.tasks["setup:docker:rootless"].events.len(), 1);
    }

    #[test]
    fn unverified_docker_scan_does_not_mark_old_problem_fixed() {
        let mut store = Store::default();
        store
            .reconcile(&snapshot(
                &[("setup:docker:rootless", FindingStatus::NeedsAttention)],
                &["docker"],
                1,
            ))
            .unwrap();
        store.reconcile(&snapshot(&[], &["cli"], 2)).unwrap();
        assert_eq!(
            store.tasks["setup:docker:rootless"].state,
            State::NeedsAttention
        );
        store.reconcile(&snapshot(&[], &["docker"], 3)).unwrap();
        assert_eq!(store.tasks["setup:docker:rootless"].state, State::Verified);
        store
            .reconcile(&snapshot(
                &[("setup:docker:rootless", FindingStatus::NeedsAttention)],
                &["docker"],
                4,
            ))
            .unwrap();
        assert_eq!(
            store.tasks["setup:docker:rootless"].state,
            State::NeedsAttention
        );
    }

    #[test]
    fn a_changed_docker_failure_is_not_a_verified_repair() {
        let mut store = Store::default();
        store
            .reconcile(&snapshot(
                &[("setup:docker:rootless", FindingStatus::NeedsAttention)],
                &[],
                1,
            ))
            .unwrap();
        // A newly stopped engine is a different problem, not proof rootless
        // setup succeeded. Only a healthy rootless engine resolves it.
        store
            .reconcile(&snapshot(
                &[("setup:docker:start", FindingStatus::NeedsAttention)],
                &[],
                2,
            ))
            .unwrap();
        assert_eq!(
            store.tasks["setup:docker:rootless"].state,
            State::NeedsAttention
        );
    }

    #[test]
    fn a_missing_cli_after_auth_failure_is_not_verified_auth() {
        let mut store = Store::default();
        store
            .reconcile(&snapshot(
                &[("setup:cli:codex:auth", FindingStatus::NeedsAttention)],
                &[],
                1,
            ))
            .unwrap();
        store
            .reconcile(&snapshot(
                &[("setup:cli:install", FindingStatus::NeedsAttention)],
                &["cli:scan"],
                2,
            ))
            .unwrap();
        assert_eq!(
            store.tasks["setup:cli:codex:auth"].state,
            State::NeedsAttention
        );
    }

    #[test]
    fn stale_snapshot_cannot_reopen_or_verify_newer_readiness() {
        let mut store = Store::default();
        store
            .reconcile(&snapshot(
                &[("setup:docker:rootless", FindingStatus::NeedsAttention)],
                &[],
                5,
            ))
            .unwrap();
        assert!(store.reconcile(&snapshot(&[], &["docker"], 4)).is_err());
        assert_eq!(
            store.tasks["setup:docker:rootless"].state,
            State::NeedsAttention
        );
    }

    #[test]
    fn persists_and_recovers_independently_of_any_project() {
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("state").join("tasks.json");
        let mut store = Store::default();
        store
            .reconcile(&snapshot(
                &[("setup:cli:install", FindingStatus::NeedsAttention)],
                &["cli"],
                4,
            ))
            .unwrap();
        save_to(&path, &store).unwrap();
        assert_eq!(load_from(&path).unwrap(), store);
    }

    #[test]
    fn refuses_invalid_ids_without_modifying_store() {
        let mut store = Store::default();
        assert!(
            store
                .reconcile(&snapshot(
                    &[("setup:../../secret", FindingStatus::NeedsAttention)],
                    &["cli"],
                    1
                ))
                .is_err()
        );
        assert!(store.tasks.is_empty());
    }
}
