use super::Queue;

const MAX_RECORDED_QUEUE_WAIT_MS: u64 = 7 * 24 * 60 * 60 * 1_000;

impl Queue {
    /// Retain runnable task ages, starting new ages only while all slots are occupied.
    pub fn update_ready_waits(
        &mut self,
        ready: &[String],
        now_unix_ms: u64,
        capacity_full: bool,
    ) -> bool {
        let ready = ready
            .iter()
            .map(String::as_str)
            .collect::<std::collections::BTreeSet<_>>();
        let previous_len = self.waiting_since_unix_ms.len();
        self.waiting_since_unix_ms
            .retain(|ticket, _| ready.contains(ticket.as_str()));
        let mut changed = self.waiting_since_unix_ms.len() != previous_len;
        for ticket in &ready {
            if let Some(started) = self.waiting_since_unix_ms.get_mut(*ticket)
                && *started > now_unix_ms
            {
                *started = now_unix_ms;
                changed = true;
            }
        }
        if capacity_full {
            for ticket in ready {
                match self.waiting_since_unix_ms.entry(ticket.to_owned()) {
                    std::collections::btree_map::Entry::Vacant(entry) => {
                        entry.insert(now_unix_ms);
                        changed = true;
                    }
                    std::collections::btree_map::Entry::Occupied(mut entry)
                        if *entry.get() > now_unix_ms =>
                    {
                        entry.insert(now_unix_ms);
                        changed = true;
                    }
                    std::collections::btree_map::Entry::Occupied(_) => {}
                }
            }
        }
        changed
    }

    /// Oldest queued task wins; stable task identity breaks equal-time ties.
    pub fn oldest_ready_ticket(&self, ready: &[String]) -> Option<String> {
        ready
            .iter()
            .min_by_key(|ticket| {
                let path = ticket.as_str();
                (
                    self.waiting_since_unix_ms
                        .get(path)
                        .copied()
                        .unwrap_or(u64::MAX),
                    self.task_uids.get(path).map(String::as_str).unwrap_or(path),
                )
            })
            .cloned()
    }

    /// Consume the age when a task leaves the queue for execution.
    pub fn take_queue_wait_ms(&mut self, ticket: &str, now_unix_ms: u64) -> Option<u64> {
        self.waiting_since_unix_ms.remove(ticket).map(|started| {
            now_unix_ms
                .saturating_sub(started)
                .min(MAX_RECORDED_QUEUE_WAIT_MS)
        })
    }

    pub fn clear_ready_waits(&mut self) -> bool {
        let mut changed = !self.waiting_since_unix_ms.is_empty();
        self.waiting_since_unix_ms.clear();
        for state in self.stable_tasks.values_mut() {
            if state.waiting_since_unix_ms.take().is_some() {
                changed = true;
            }
        }
        changed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ready_age_is_retained_while_runnable_and_reset_when_blocked() {
        let mut queue = Queue::default();
        let ready = ["task-a".into(), "task-b".into()];
        assert!(!queue.update_ready_waits(&ready, 5, false));
        assert!(queue.update_ready_waits(&ready, 10, true));
        assert!(queue.update_ready_waits(&["task-a".into()], 20, true));
        assert_eq!(queue.take_queue_wait_ms("task-a", 35), Some(25));
        assert_eq!(queue.take_queue_wait_ms("task-a", 40), None);
        assert!(!queue.clear_ready_waits());
    }

    #[test]
    fn clearing_waits_removes_every_saved_queue_age() {
        let mut queue = Queue::default();
        assert!(queue.update_ready_waits(&["task-a".into(), "task-b".into()], 10, true));
        assert!(queue.clear_ready_waits());
        assert_eq!(queue.take_queue_wait_ms("task-a", 20), None);
        assert_eq!(queue.take_queue_wait_ms("task-b", 20), None);
        assert!(!queue.clear_ready_waits());
    }

    #[test]
    fn oldest_waiter_precedes_unqueued_work_and_stable_id_breaks_ties() {
        let mut queue = Queue::default();
        queue.task_uids.insert("path-b".into(), "uid-b".into());
        queue.task_uids.insert("path-a".into(), "uid-a".into());
        queue.waiting_since_unix_ms.insert("path-b".into(), 5);
        queue.waiting_since_unix_ms.insert("path-a".into(), 5);
        assert_eq!(
            queue.oldest_ready_ticket(&["path-b".into(), "path-a".into()]),
            Some("path-a".into())
        );
        assert_eq!(
            queue.oldest_ready_ticket(&["path-new".into(), "path-b".into()]),
            Some("path-b".into())
        );
    }

    #[test]
    fn current_queue_state_restores_and_clears_ready_age() {
        let mut queue = Queue::default();
        let stable_uid = uuid::Uuid::from_u128(1).to_string();
        queue
            .task_uids
            .insert("tasks/oldest.md".into(), stable_uid.clone());
        queue
            .waiting_since_unix_ms
            .insert("tasks/oldest.md".into(), 100);
        let bytes = serde_json::to_vec(&queue).unwrap();
        let mut restored = Queue::decode_persisted(&bytes).unwrap();
        assert!(restored.clear_ready_waits());
        assert_eq!(
            restored.stable_tasks[&stable_uid].waiting_since_unix_ms,
            None
        );

        let unsupported = serde_json::json!({"schemaVersion": 0});
        assert!(Queue::decode_persisted(&serde_json::to_vec(&unsupported).unwrap()).is_err());
    }
}
