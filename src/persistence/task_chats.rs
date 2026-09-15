//! One durable conversation per board identity, separate from Main Chat.
use crate::domain::ChatMessage;
use std::collections::BTreeMap;

#[derive(Default)]
pub struct TaskChats {
    pub messages: BTreeMap<String, Vec<ChatMessage>>,
    pub drafts: BTreeMap<String, String>,
    pub active: Option<String>,
    pub error: Option<String>,
    pending: Vec<(String, Vec<ChatMessage>)>,
    loaded: bool,
    refreshed: Option<std::time::Instant>,
}

type Histories = BTreeMap<String, Vec<ChatMessage>>;

fn read(slug: &str) -> Result<Histories, String> {
    match std::fs::read(super::project_dir(slug).join("task-conversations.json")) {
        Ok(bytes) => serde_json::from_slice(&bytes).map_err(|e| e.to_string()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(BTreeMap::new()),
        Err(e) => Err(e.to_string()),
    }
}

fn merge(histories: &mut Histories, key: &str, messages: &[ChatMessage]) {
    if messages.is_empty() {
        return;
    }
    let history = histories.entry(key.into()).or_default();
    for message in messages {
        if !history.iter().any(|existing| existing.id == message.id) {
            history.push(message.clone());
        }
    }
    history.sort_by(|a, b| a.ts.cmp(&b.ts).then_with(|| a.id.cmp(&b.id)));
}

impl TaskChats {
    pub fn ensure_loaded(&mut self, slug: &str) {
        if self.loaded && self.refreshed.is_some_and(|t| t.elapsed().as_secs() < 1) {
            return;
        }
        self.refreshed = Some(std::time::Instant::now());
        match read(slug) {
            Ok(mut messages) => {
                for (key, pending) in &self.pending {
                    merge(&mut messages, key, pending);
                }
                self.messages = messages;
                self.loaded = true;
                if self.pending.is_empty() {
                    self.error = None;
                }
            }
            Err(e) => self.error = Some(format!("Cannot read task conversations: {e}")),
        }
    }

    pub fn append(
        &mut self,
        slug: &str,
        key: &str,
        messages: Vec<ChatMessage>,
    ) -> Result<(), String> {
        let result = (|| {
            let dir = super::project_dir(slug);
            std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
            let lock = std::fs::OpenOptions::new()
                .create(true)
                .truncate(false)
                .write(true)
                .open(dir.join("task-conversations.lock"))
                .map_err(|e| e.to_string())?;
            lock.try_lock().map_err(|_| {
                "Another window is saving a task conversation; retry shortly.".to_string()
            })?;
            // Read under the lock: a stale window must never replace newer histories.
            let mut next = read(slug)?;
            for (pending_key, pending) in &self.pending {
                merge(&mut next, pending_key, pending);
            }
            merge(&mut next, key, &messages);
            let json = serde_json::to_string(&next).map_err(|e| e.to_string())?;
            crate::artifacts::atomic_write(&dir.join("task-conversations.json"), &json)
                .map_err(|e| e.to_string())?;
            Ok(next)
        })();
        match result {
            Ok(next) => {
                self.messages = next;
                self.loaded = true;
                self.pending.clear();
                self.refreshed = Some(std::time::Instant::now());
                self.error = None;
                Ok(())
            }
            Err(e) => {
                self.error = Some(format!("Task conversation could not be saved: {e}"));
                Err(e)
            }
        }
    }

    /// An already-completed agent reply must remain visible if storage fails.
    pub fn remember_response(&mut self, slug: &str, key: &str, messages: Vec<ChatMessage>) {
        if self.append(slug, key, messages.clone()).is_err() {
            merge(&mut self.messages, key, &messages);
            self.pending.push((key.into(), messages));
        }
    }

    pub fn retry_save(&mut self, slug: &str) {
        self.ensure_loaded(slug);
        if self.pending.is_empty() {
            return;
        }
        // The in-memory map already includes the unsaved replies.
        if self.append(slug, "", Vec::new()).is_ok() {
            self.pending.clear();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::ChatRole;

    #[test]
    fn stale_windows_merge_and_failed_response_retries_exactly_once() {
        let dir = std::env::temp_dir().join(format!("packet_task_merge_{}", std::process::id()));
        let slug = dir.to_str().unwrap();
        let mut a = TaskChats::default();
        let mut b = TaskChats::default();
        a.ensure_loaded(slug);
        b.ensure_loaded(slug);
        a.append(
            slug,
            "one",
            vec![ChatMessage::new(ChatRole::User, "first", None)],
        )
        .unwrap();
        b.append(
            slug,
            "two",
            vec![ChatMessage::new(ChatRole::User, "second", None)],
        )
        .unwrap();
        a.append(
            slug,
            "one",
            vec![ChatMessage::new(ChatRole::User, "third", None)],
        )
        .unwrap();
        let lock = std::fs::OpenOptions::new()
            .write(true)
            .open(dir.join("task-conversations.lock"))
            .unwrap();
        // Establish the deliberately held lock before testing the nonblocking
        // save path; this setup must not itself depend on scheduling.
        lock.lock().unwrap();
        a.remember_response(
            slug,
            "one",
            vec![ChatMessage::new(ChatRole::Agent, "answer", None)],
        );
        assert!(a.error.is_some());
        assert_eq!(a.messages["one"].len(), 3);
        assert_eq!(read(slug).unwrap()["one"].len(), 2);
        drop(lock);
        b.append(
            slug,
            "two",
            vec![ChatMessage::new(ChatRole::Agent, "another answer", None)],
        )
        .unwrap();
        a.retry_save(slug);
        a.retry_save(slug);
        assert!(a.error.is_none());
        let persisted = read(slug).unwrap();
        assert_eq!(persisted["one"].len(), 3);
        assert_eq!(persisted["two"].len(), 2);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn corrupt_store_is_never_replaced_by_stale_memory() {
        let dir = std::env::temp_dir().join(format!("packet_task_corrupt_{}", std::process::id()));
        let slug = dir.to_str().unwrap();
        let mut chats = TaskChats::default();
        chats
            .append(
                slug,
                "one",
                vec![ChatMessage::new(ChatRole::User, "first", None)],
            )
            .unwrap();
        let path = dir.join("task-conversations.json");
        let previous = std::fs::read(&path).unwrap();
        std::fs::write(&path, "broken").unwrap();
        chats.remember_response(
            slug,
            "one",
            vec![ChatMessage::new(ChatRole::Agent, "answer", None)],
        );
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "broken");
        assert_eq!(chats.messages["one"].len(), 2);
        std::fs::write(&path, previous).unwrap();
        chats.retry_save(slug);
        assert!(chats.error.is_none());
        assert_eq!(read(slug).unwrap()["one"].len(), 2);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn independent_streams_survive_restart_without_compaction_or_main_chat() {
        let dir = std::env::temp_dir().join(format!("packet_task_chats_{}", std::process::id()));
        let slug = dir.to_str().unwrap();
        let mut chats = TaskChats::default();
        let first = (0..510)
            .map(|i| ChatMessage::new(ChatRole::User, format!("reply {i}"), None))
            .collect::<Vec<_>>();
        chats.append(slug, "CLR-001", first.clone()).unwrap();
        chats
            .append(
                slug,
                "planning/tasks/002.md",
                vec![ChatMessage::new(ChatRole::User, "other task", None)],
            )
            .unwrap();
        let mut reopened = TaskChats::default();
        reopened.ensure_loaded(slug);
        assert_eq!(reopened.messages["CLR-001"], first);
        assert_eq!(reopened.messages["planning/tasks/002.md"].len(), 1);
        assert!(!dir.join("chat.jsonl").exists());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
