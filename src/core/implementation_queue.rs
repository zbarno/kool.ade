//! Durable Auto-mode preferences and dependency-aware queue selection.
use crate::{artifacts::task_docs::TaskDocument, core::implementation::Implementation};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Queue {
    pub auto_mode: bool,
    pub running: bool,
    pub current_ticket: Option<String>,
    pub in_flight: std::collections::BTreeSet<String>,
    pub max_parallel: usize,
    pub blocked: BTreeMap<String, String>,
    pub last_error: String,
    pub recovery_paused: bool,
    pub recovery_attempts: BTreeMap<String, usize>,
}
impl Default for Queue {
    fn default() -> Self {
        Self {
            auto_mode: true,
            running: false,
            current_ticket: None,
            in_flight: Default::default(),
            max_parallel: 3,
            blocked: Default::default(),
            last_error: String::new(),
            recovery_paused: false,
            recovery_attempts: Default::default(),
        }
    }
}
fn directory(repo: &Path) -> anyhow::Result<PathBuf> {
    let result = std::process::Command::new("git")
        .args(["rev-parse", "--path-format=absolute", "--git-common-dir"])
        .current_dir(repo)
        .output()?;
    anyhow::ensure!(result.status.success(), "Cannot locate queue metadata");
    Ok(PathBuf::from(String::from_utf8(result.stdout)?.trim()))
}
impl Queue {
    /// Retry known orchestration failures once, preserving a durable budget
    /// across restarts. Worker-level correction limits still apply.
    pub fn recoverable_tickets(&self, docs: &[TaskDocument]) -> Vec<String> {
        if !self.auto_mode || self.recovery_paused {
            return Vec::new();
        }
        self.blocked
            .iter()
            .filter_map(|(ticket, error)| {
                let doc = docs.iter().find(|doc| &doc.path == ticket)?;
                let recoverable = error.contains("diverged before publication")
                    || (error
                        .contains("No implementation changes relative to the starting commit")
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
        match fs::read(directory(repo)?.join("packet-queue.json")) {
            Ok(bytes) => Ok(serde_json::from_slice(&bytes)?),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(error) => Err(error.into()),
        }
    }
    pub fn save(&self, repo: &Path) -> anyhow::Result<()> {
        let path = directory(repo)?.join("packet-queue.json");
        let temp = path.with_extension(format!("{}.tmp", std::process::id()));
        fs::write(&temp, serde_json::to_vec_pretty(self)?)?;
        fs::rename(temp, path)?;
        Ok(())
    }
    pub fn acquire(repo: &Path) -> anyhow::Result<fs::File> {
        let lock = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(directory(repo)?.join("packet-queue.lock"))?;
        lock.try_lock().map_err(|_| {
            anyhow::anyhow!("Auto queue is already running in another Packet window")
        })?;
        Ok(lock)
    }
}

pub fn next_ticket(
    docs: &[TaskDocument],
    states: &BTreeMap<String, Implementation>,
) -> Result<Option<String>, String> {
    next_ready_ticket(docs, states, &Default::default())
}

pub fn next_ready_ticket(
    docs: &[TaskDocument],
    states: &BTreeMap<String, Implementation>,
    active: &std::collections::BTreeSet<String>,
) -> Result<Option<String>, String> {
    let mut docs = docs
        .iter()
        .filter(|doc| !doc.path.ends_with("/README.md"))
        .collect::<Vec<_>>();
    docs.sort_by_key(|doc| &doc.path);
    let done = |path: &str| {
        states.get(path).is_some_and(|state| {
            state.status == "Done" || state.pr_state.as_deref() == Some("MERGED")
        })
    };
    let mut waiting = Vec::new();
    'tasks: for doc in docs {
        if done(&doc.path) || active.contains(&doc.path) {
            continue;
        }
        if states
            .get(&doc.path)
            .is_some_and(|state| state.pr_url.is_some())
        {
            waiting.push(format!(
                "Waiting for the existing PR for {} to merge",
                doc.title
            ));
            continue;
        }
        let mut in_dependencies = false;
        let mut dependencies = String::new();
        for line in doc.text.lines() {
            if line.starts_with("## ") {
                in_dependencies = line.trim().eq_ignore_ascii_case("## Dependencies");
                continue;
            }
            if in_dependencies {
                dependencies.push_str(line);
                dependencies.push('\n');
            }
        }
        for event in pulldown_cmark::Parser::new(&dependencies) {
            if let pulldown_cmark::Event::Start(pulldown_cmark::Tag::Link { dest_url, .. }) = event
            {
                let dependency = Path::new(&*dest_url);
                if dependency.components().count() != 1 || !dest_url.ends_with(".md") {
                    waiting.push(format!(
                        "Review unsupported dependency link {dest_url} in {}",
                        doc.title
                    ));
                    continue 'tasks;
                }
                let path = Path::new(&doc.path).parent().unwrap().join(dependency);
                if !done(&path.to_string_lossy()) {
                    waiting.push(format!(
                        "{} is waiting for dependency {dest_url}",
                        doc.title
                    ));
                    continue 'tasks;
                }
            }
        }
        return Ok(Some(doc.path.clone()));
    }
    if waiting.is_empty() {
        Ok(None)
    } else {
        Err(waiting.join("\n"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn doc(n: usize, dependencies: &str) -> TaskDocument {
        TaskDocument {
            path: format!("planning/tasks/fixture/{n:03}-task.md"),
            title: format!("Task {n}"),
            text: format!(
                "# Task {n}\n\n## Dependencies\n{dependencies}\n\n## Acceptance criteria\n- Done."
            ),
        }
    }
    fn done(ticket: &str) -> Implementation {
        serde_json::from_value(serde_json::json!({"ticket":ticket,"ticket_text":"","branch":"task","base":"main","base_commit":"base","worktree":"fixture","status":"Done","detail":"","pr_url":null,"verified_head":"head"})).unwrap()
    }
    #[test]
    fn known_orchestration_failures_recover_once_but_external_blockers_do_not() {
        let docs = vec![doc(1, "None"), doc(2, "None")];
        let mut queue = Queue::default();
        queue.blocked.insert(docs[0].path.clone(), "Local main and freshly fetched origin/main diverged before publication; verified work is preserved".into());
        queue.blocked.insert(
            docs[1].path.clone(),
            "Credentials require human intervention".into(),
        );
        queue.recovery_paused = true;
        assert!(queue.recoverable_tickets(&docs).is_empty());
        queue.recovery_paused = false;
        let tickets = queue.recoverable_tickets(&docs);
        assert_eq!(tickets, vec![docs[0].path.clone()]);
        queue.schedule_recovery(&tickets);
        assert!(queue.running);
        queue
            .blocked
            .insert(docs[0].path.clone(), "diverged before publication".into());
        let restored: Queue =
            serde_json::from_str(&serde_json::to_string(&queue).unwrap()).unwrap();
        assert!(restored.recoverable_tickets(&docs).is_empty());
        assert!(restored.blocked.contains_key(&docs[1].path));
    }

    #[test]
    fn queue_advances_in_order_and_waits_for_dependencies() {
        let docs = vec![
            doc(2, "- [Task 001](001-task.md) must be complete."),
            doc(1, "None."),
        ];
        let mut states = BTreeMap::new();
        assert_eq!(
            next_ticket(&docs, &states).unwrap(),
            Some(docs[1].path.clone())
        );
        states.insert(docs[1].path.clone(), done(&docs[1].path));
        assert_eq!(
            next_ticket(&docs, &states).unwrap(),
            Some(docs[0].path.clone())
        );
        states.insert(docs[0].path.clone(), done(&docs[0].path));
        assert_eq!(next_ticket(&docs, &states).unwrap(), None);
        assert!(
            next_ticket(&[doc(1, "- [missing](009-task.md)")], &BTreeMap::new())
                .unwrap_err()
                .contains("waiting")
        );
    }
    #[test]
    fn independent_tasks_skip_blocked_and_running_predecessors() {
        let docs = vec![
            doc(1, "- [missing](009-task.md)"),
            doc(2, "None."),
            doc(3, "None."),
            doc(4, "- [two](002-task.md)"),
        ];
        let states = BTreeMap::new();
        assert_eq!(
            next_ticket(&docs, &states).unwrap(),
            Some(docs[1].path.clone())
        );
        let active = std::collections::BTreeSet::from([docs[1].path.clone()]);
        assert_eq!(
            next_ready_ticket(&docs, &states, &active).unwrap(),
            Some(docs[2].path.clone())
        );
        let active = std::collections::BTreeSet::from([docs[1].path.clone(), docs[2].path.clone()]);
        assert!(
            next_ready_ticket(&docs, &states, &active)
                .unwrap_err()
                .contains("waiting")
        );
        let states = BTreeMap::from([(docs[1].path.clone(), done(&docs[1].path))]);
        assert_eq!(
            next_ready_ticket(&docs, &states, &active).unwrap(),
            Some(docs[3].path.clone())
        );
    }

    #[test]
    fn cycles_and_invalid_dependencies_do_not_starve_independent_work() {
        let docs = vec![
            doc(1, "- [two](002-task.md)"),
            doc(2, "- [one](001-task.md)"),
            doc(3, "- [unsafe](../other.md)"),
            doc(4, "None."),
        ];
        assert_eq!(
            next_ticket(&docs, &BTreeMap::new()).unwrap(),
            Some(docs[3].path.clone())
        );
        let waiting = next_ticket(&docs[..3], &BTreeMap::new()).unwrap_err();
        assert!(waiting.contains("waiting") && waiting.contains("unsupported"));
    }

    #[test]
    fn preferences_and_inflight_ticket_survive_restart_and_lock_excludes_another_window() {
        let root = std::env::temp_dir().join(format!(
            "packet-queue-{}",
            chrono::Utc::now().timestamp_nanos_opt().unwrap()
        ));
        fs::create_dir_all(&root).unwrap();
        assert!(
            std::process::Command::new("git")
                .args(["init", "-q"])
                .current_dir(&root)
                .status()
                .unwrap()
                .success()
        );
        let mut queue = Queue::load(&root).unwrap();
        assert!(queue.auto_mode);
        assert!(!queue.running);
        let lock = Queue::acquire(&root).unwrap();
        assert!(Queue::acquire(&root).is_err());
        queue.running = true;
        queue.current_ticket = Some("planning/tasks/fixture/001-task.md".into());
        queue.in_flight.extend(["one".into(), "two".into()]);
        queue.max_parallel = 4;
        queue.save(&root).unwrap();
        drop(lock);
        let _new_lock = Queue::acquire(&root).unwrap();
        let loaded = Queue::load(&root).unwrap();
        assert!(loaded.running && loaded.auto_mode);
        assert_eq!(loaded.current_ticket, queue.current_ticket);
        assert_eq!(loaded.in_flight, queue.in_flight);
        assert_eq!(loaded.max_parallel, 4);
        drop(_new_lock);
        fs::remove_dir_all(root).unwrap();
    }
}
