//! Runtime path projection over UID-keyed durable Auto-queue state.
mod store;

use crate::{
    artifacts::task_docs::TaskDocument,
    core::implementation::{Failure, FailureKind, RecoveryDisposition},
};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};
use store::TaskQueueState;

#[derive(Debug, Clone)]
pub struct Queue {
    /// Automatically prepare read-only project recommendations from activity.
    pub auto_plan: bool,
    /// Continue eligible, explicitly approved implementation tasks.
    pub auto_build: bool,
    /// Publish verified work without waiting for a user action.
    pub auto_publish: bool,
    /// Wait for independent provider checks before updating a remote default branch.
    pub require_independent_checks: bool,
    pub running: bool,
    pub current_ticket: Option<String>,
    pub in_flight: BTreeSet<String>,
    pub max_parallel: usize,
    /// Ready task paths parked only because every execution slot is occupied.
    /// Derived at runtime and deliberately excluded from the durable queue.
    pub waiting_for_capacity: BTreeSet<String>,
    pub blocked: BTreeMap<String, Failure>,
    pub last_error: String,
    /// In-memory stale remote claim offered for an explicit recovery action.
    pub stale_claim: Option<(String, crate::core::task_claim::ClaimRecord)>,
    pub recovery_paused: bool,
    pub recovery_attempts: BTreeMap<String, usize>,
    task_uids: BTreeMap<String, String>,
    current_paths: BTreeMap<String, String>,
    stable_tasks: BTreeMap<String, TaskQueueState>,
    identity_dirty: bool,
}

impl Default for Queue {
    fn default() -> Self {
        Self {
            auto_plan: true,
            auto_build: true,
            auto_publish: false,
            require_independent_checks: false,
            running: false,
            current_ticket: None,
            in_flight: BTreeSet::new(),
            max_parallel: 3,
            waiting_for_capacity: BTreeSet::new(),
            blocked: BTreeMap::new(),
            last_error: String::new(),
            stale_claim: None,
            recovery_paused: false,
            recovery_attempts: BTreeMap::new(),
            task_uids: BTreeMap::new(),
            current_paths: BTreeMap::new(),
            stable_tasks: BTreeMap::new(),
            identity_dirty: false,
        }
    }
}

impl Queue {
    fn put_runtime_state(&mut self, path: &str, state: &TaskQueueState) {
        if state.current {
            self.current_ticket = Some(path.to_owned());
        }
        if state.in_flight {
            self.in_flight.insert(path.to_owned());
        }
        if let Some(failure) = &state.blocked {
            self.blocked.insert(path.to_owned(), failure.clone());
        }
        if state.recovery_attempts > 0 {
            self.recovery_attempts
                .insert(path.to_owned(), state.recovery_attempts);
        }
    }

    /// Re-key task state against the current board. Paths remain a runtime/UI
    /// projection; the durable queue state uses task UIDs.
    pub fn bind_task_documents(&mut self, docs: &[TaskDocument]) -> Result<bool, String> {
        let mut current_paths = BTreeMap::new();
        let mut reused_paths = Vec::new();
        for doc in docs.iter().filter(|doc| !doc.path.ends_with("/README.md")) {
            let Some(identity) = &doc.identity else {
                continue;
            };
            if let Some(previous) = current_paths.insert(identity.uid.clone(), doc.path.clone())
                && previous != doc.path
            {
                return Err(format!(
                    "Task identity {} appears at both {previous} and {}",
                    identity.uid, doc.path
                ));
            }
            if self
                .task_uids
                .get(&doc.path)
                .is_some_and(|old| old != &identity.uid)
            {
                reused_paths.push((doc.path.clone(), self.task_uids[&doc.path].clone()));
            }
        }
        let mut changed = false;
        for (path, old_uid) in reused_paths {
            let orphan = format!("@unlinked:{old_uid}");
            if self.current_ticket.as_deref() == Some(path.as_str()) {
                self.current_ticket = Some(orphan.clone());
            }
            if self.in_flight.remove(&path) {
                self.in_flight.insert(orphan.clone());
            }
            if let Some(failure) = self.blocked.remove(&path) {
                self.blocked.entry(orphan.clone()).or_insert(failure);
            }
            if let Some(attempts) = self.recovery_attempts.remove(&path) {
                self.recovery_attempts
                    .entry(orphan.clone())
                    .and_modify(|saved| *saved = (*saved).max(attempts))
                    .or_insert(attempts);
            }
            self.task_uids.remove(&path);
            self.task_uids.insert(orphan, old_uid.clone());
            if let Some(state) = self.stable_tasks.get_mut(&old_uid)
                && state.path_hint == path
            {
                state.path_hint = format!("@unlinked:{old_uid}");
            }
            self.last_error = format!(
                "Saved queue state for an older task at {path} remains under its previous identity and was not attached to the new task."
            );
            changed = true;
        }
        for (uid, path) in &current_paths {
            let mut sources = self
                .task_uids
                .iter()
                .filter_map(|(old_path, old_uid)| (old_uid == uid).then_some(old_path.clone()))
                .collect::<BTreeSet<_>>();
            sources.insert(path.clone());
            let has_state = sources.iter().any(|source| {
                self.current_ticket.as_deref() == Some(source.as_str())
                    || self.in_flight.contains(source)
                    || self.blocked.contains_key(source)
                    || self.recovery_attempts.contains_key(source)
            });
            if has_state && !self.stable_tasks.contains_key(uid) {
                changed = true;
            }
            if self
                .current_ticket
                .as_ref()
                .is_some_and(|ticket| sources.contains(ticket) && ticket != path)
            {
                self.current_ticket = Some(path.clone());
                changed = true;
            }
            if sources.iter().any(|source| self.in_flight.contains(source)) {
                for source in &sources {
                    self.in_flight.remove(source);
                }
                self.in_flight.insert(path.clone());
                changed |= sources.iter().any(|source| source != path);
            }
            let mut attempts = 0;
            for source in &sources {
                attempts = attempts.max(self.recovery_attempts.remove(source).unwrap_or_default());
            }
            if attempts > 0 {
                self.recovery_attempts.insert(path.clone(), attempts);
            }
            let failures = sources
                .iter()
                .filter_map(|source| self.blocked.remove(source))
                .collect::<Vec<_>>();
            if !failures.is_empty() {
                self.blocked
                    .insert(path.clone(), combine_failures(failures));
                changed |= sources.iter().any(|source| source != path);
            }
            self.task_uids.insert(path.clone(), uid.clone());
            if self
                .stable_tasks
                .get(uid)
                .is_some_and(|state| state.path_hint != *path)
            {
                changed = true;
            }
        }
        self.current_paths = current_paths;
        self.identity_dirty |= changed;
        Ok(self.identity_dirty)
    }

    pub fn recoverable_tickets(&self, docs: &[TaskDocument]) -> Vec<String> {
        if !self.auto_build || self.recovery_paused {
            return Vec::new();
        }
        self.blocked
            .iter()
            .filter_map(|(ticket, failure)| {
                let doc = docs.iter().find(|doc| &doc.path == ticket)?;
                let recoverable = failure.recovery == RecoveryDisposition::AutomaticRetry
                    || (failure.kind == FailureKind::NoImplementationChanges
                        && failure.recovery == RecoveryDisposition::ExplicitResume
                        && crate::core::implementation::permits_evidence_only_completion(
                            &doc.text,
                        ));
                (recoverable && self.recovery_attempts.get(ticket).copied().unwrap_or(0) < 1)
                    .then(|| ticket.clone())
            })
            .collect()
    }

    pub fn schedule_recovery(&mut self, tickets: &[String]) {
        for ticket in tickets {
            self.blocked.remove(ticket);
            *self.recovery_attempts.entry(ticket.clone()).or_default() += 1;
        }
        if !tickets.is_empty() {
            self.running = true;
            self.last_error.clear();
        }
    }

    pub fn load(repo: &Path) -> anyhow::Result<Self> {
        let _gate = crate::artifacts::migration::acquire_project_state_gate(repo)?;
        let path = directory(repo)?.join("koolade-queue.json");
        match fs::read(&path) {
            Ok(bytes) => {
                let (queue, migrated) = Self::decode_persisted(&bytes)?;
                if migrated {
                    crate::artifacts::atomic_write(&path, &serde_json::to_string_pretty(&queue)?)?;
                }
                Ok(queue)
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(error) => Err(error.into()),
        }
    }

    pub(crate) fn decode_persisted(bytes: &[u8]) -> anyhow::Result<(Self, bool)> {
        let value: serde_json::Value = serde_json::from_slice(bytes)?;
        store::decode_value(value)
    }

    pub fn save(&mut self, repo: &Path) -> anyhow::Result<()> {
        let _gate = crate::artifacts::migration::acquire_project_state_gate(repo)?;
        let path = directory(repo)?.join("koolade-queue.json");
        let data = store::persisted(self);
        crate::artifacts::atomic_write(&path, &serde_json::to_string_pretty(&data)?)?;
        self.stable_tasks = data.tasks.clone();
        self.identity_dirty = false;
        Ok(())
    }

    pub fn acquire(repo: &Path) -> anyhow::Result<fs::File> {
        let lock = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(directory(repo)?.join("koolade-queue.lock"))?;
        lock.try_lock().map_err(|_| {
            anyhow::anyhow!("Auto queue is already running in another Kool.ad/e window")
        })?;
        Ok(lock)
    }
}

fn combine_failures(failures: Vec<Failure>) -> Failure {
    let first = failures[0].clone();
    if failures.iter().all(|failure| {
        failure.kind == first.kind
            && failure.recovery == first.recovery
            && failure.message == first.message
    }) {
        return first;
    }
    Failure::new(
        FailureKind::Other,
        RecoveryDisposition::UserAction,
        failures
            .into_iter()
            .map(|failure| failure.message)
            .collect::<Vec<_>>()
            .join("\n"),
    )
}

pub(super) fn directory(repo: &Path) -> anyhow::Result<PathBuf> {
    let result = std::process::Command::new("git")
        .args(["rev-parse", "--path-format=absolute", "--git-common-dir"])
        .current_dir(repo)
        .output()?;
    anyhow::ensure!(result.status.success(), "Cannot locate queue metadata");
    Ok(PathBuf::from(String::from_utf8(result.stdout)?.trim()))
}
