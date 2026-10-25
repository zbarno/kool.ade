//! Durable, machine-local bootstrap task observations.
//!
//! These are NOT shared repository planning records. No stored status or
//! model claim can authorize a host mutation or bypass an execution gate.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::app::setup_attention::catalog::{
    FindingStatus, MachineSnapshot, SCHEMA_VERSION as SNAPSHOT_VERSION,
};

const STORE_VERSION: u8 = 3;
const MAX_STATE_BYTES: u64 = 2 * 1024 * 1024;
const MAX_TASKS: usize = 128;

mod io;
#[cfg(test)]
mod tests;
mod transaction;
pub use io::{load, path, save};
pub(super) use io::{load_from, save_to};
pub(crate) use transaction::reconcile_at_if;

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
    /// Latest probe completion time observed, including no-op scans.
    #[serde(default)]
    pub last_observed_at_unix_ms: u64,
    /// Orders overlapping scans independently of when their probes finish.
    #[serde(default)]
    pub last_scan_started_at_unix_ms: u64,
    pub tasks: BTreeMap<String, LocalTask>,
}

impl Default for Store {
    fn default() -> Self {
        Self {
            schema_version: STORE_VERSION,
            last_observed_at_unix_ms: 0,
            last_scan_started_at_unix_ms: 0,
            tasks: BTreeMap::new(),
        }
    }
}

impl Store {
    /// Reconcile an independently probed snapshot. Stale/unknown domains
    /// never verify absent tasks. No setup model response is accepted here.
    pub fn reconcile(&mut self, snapshot: &MachineSnapshot) -> anyhow::Result<bool> {
        snapshot.validate()?;
        anyhow::ensure!(
            snapshot.schema_version == SNAPSHOT_VERSION,
            "unsupported machine readiness snapshot version"
        );
        anyhow::ensure!(
            snapshot.findings.len() <= MAX_TASKS,
            "too many setup findings"
        );
        // Order durable results by when probing began, not when a slow scan
        // finished. Completion timestamps can arrive out of order across workers.
        anyhow::ensure!(
            self.last_scan_started_at_unix_ms <= snapshot.scan_started_at_unix_ms,
            "stale machine setup observation"
        );

        // Millisecond timestamps cannot order distinct scans that start in the
        // same tick (including scans from separate processes). Permit a no-op,
        // but never let ambiguous same-time evidence change the durable ledger.
        let previous_at_same_start = (self.last_scan_started_at_unix_ms
            == snapshot.scan_started_at_unix_ms)
            .then(|| self.clone());

        let mut changed = false;
        let mut task_ledger_changed = false;
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
                task.observed_at_unix_ms = snapshot.scan_started_at_unix_ms;
                task.verified_at_unix_ms = None;
                if meaningful {
                    record(task, snapshot.scan_started_at_unix_ms);
                    changed = true;
                    task_ledger_changed = true;
                }
            } else {
                let mut task = LocalTask {
                    id: finding.id.clone(),
                    title: finding.title.clone(),
                    explanation: finding.explanation.clone(),
                    next_step: finding.next_step.clone(),
                    prerequisites: finding.prerequisites.clone(),
                    state,
                    observed_at_unix_ms: snapshot.scan_started_at_unix_ms,
                    verified_at_unix_ms: None,
                    events: Vec::new(),
                };
                record(&mut task, snapshot.scan_started_at_unix_ms);
                self.tasks.insert(finding.id.clone(), task);
                changed = true;
                task_ledger_changed = true;
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
                task.verified_at_unix_ms = Some(snapshot.scan_started_at_unix_ms);
                task.observed_at_unix_ms = snapshot.scan_started_at_unix_ms;
                record(task, snapshot.scan_started_at_unix_ms);
                changed = true;
                task_ledger_changed = true;
            }
        }
        anyhow::ensure!(
            self.tasks.len() <= MAX_TASKS,
            "setup task store reached its size limit"
        );
        if self.last_observed_at_unix_ms < snapshot.checked_at_unix_ms {
            self.last_observed_at_unix_ms = snapshot.checked_at_unix_ms;
            changed = true;
        }
        if self.last_scan_started_at_unix_ms != snapshot.scan_started_at_unix_ms {
            self.last_scan_started_at_unix_ms = snapshot.scan_started_at_unix_ms;
            changed = true;
        }
        if let Some(previous) = previous_at_same_start
            && task_ledger_changed
        {
            *self = previous;
            anyhow::bail!("ambiguous equal-start machine setup observation");
        }
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
        ["setup", "git", "install"] => "git:executable".to_owned(),
        ["setup", "git", "identity"] => "git:identity".to_owned(),
        ["setup", "git", "verify"] => "git".to_owned(),
        [
            "setup",
            "docker",
            "image-unverified" | "image-digest" | "image-architecture",
        ] => {
            return snapshot.checked_domains.iter().any(|domain| {
                matches!(
                    domain.as_str(),
                    "images:general:amd64" | "images:general:arm64"
                )
            });
        }
        [
            "setup",
            "docker",
            "install" | "start" | "permission" | "rootless" | "windows-containers" | "disk"
            | "limits" | "cgroups" | "seccomp" | "api" | "os" | "architecture" | "workspace"
            | "desktop" | "endpoint" | "capabilities",
        ] => "docker".to_owned(),
        ["setup", "images", profile, architecture] => {
            format!("images:{profile}:{architecture}")
        }
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

/// Reading a corrupt/newer store must fail without overwriting it. No user
/// approval, secret, or command is recorded here, only diagnostic task state.
pub fn reconcile_live(snapshot: &MachineSnapshot) -> anyhow::Result<Store> {
    transaction::reconcile_live(snapshot)
}

pub(crate) fn reconcile_live_if(
    snapshot: &MachineSnapshot,
    still_current: impl Fn() -> bool,
) -> anyhow::Result<Option<Store>> {
    transaction::reconcile_live_if(snapshot, still_current)
}
