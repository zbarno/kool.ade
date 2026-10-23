use super::{Queue, combine_failures};
use crate::artifacts::task_docs::TaskDocument;
use std::collections::{BTreeMap, BTreeSet};

impl Queue {
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
            if let Some(waiting_since) = self.waiting_since_unix_ms.remove(&path) {
                self.waiting_since_unix_ms
                    .entry(orphan.clone())
                    .or_insert(waiting_since);
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
                    || self.waiting_since_unix_ms.contains_key(source)
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
            let waiting_since = sources
                .iter()
                .filter_map(|source| self.waiting_since_unix_ms.remove(source))
                .min();
            if let Some(waiting_since) = waiting_since
                && self
                    .waiting_since_unix_ms
                    .insert(path.clone(), waiting_since)
                    != Some(waiting_since)
            {
                changed = true;
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
}
