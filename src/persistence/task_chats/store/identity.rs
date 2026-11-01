use super::{TaskChats, canonicalize, display_histories, merge_identity};
use crate::artifacts::task_docs::TaskDocument;
use std::collections::BTreeMap;

impl TaskChats {
    /// Refresh the runtime path projection. Histories themselves are keyed by
    /// task UID on disk, so a moved story immediately keeps its conversation.
    pub fn bind_task_documents(&mut self, docs: &[TaskDocument]) -> Result<(), String> {
        let aliases = docs
            .iter()
            .filter_map(|doc| {
                doc.identity
                    .as_ref()
                    .map(|identity| (doc.path.clone(), identity.uid.clone()))
            })
            .collect::<Vec<_>>();
        let legacy_migration_keys = aliases
            .iter()
            .map(|(path, _)| path.clone())
            .collect::<Vec<_>>();
        self.bind_conversation_identities(&aliases, &legacy_migration_keys)
    }

    /// Bind durable board conversations to immutable artifact identities.
    /// Display keys such as feature IDs and CLR numbers can be reused, so
    /// stored histories must follow the artifact UID instead of the key.
    pub fn bind_conversation_identities(
        &mut self,
        aliases: &[(String, String)],
        legacy_migration_keys: &[String],
    ) -> Result<(), String> {
        let mut identities: BTreeMap<String, String> = BTreeMap::new();
        let mut paths: BTreeMap<String, String> = BTreeMap::new();
        let mut ambiguous_paths = std::collections::BTreeSet::new();
        let mut ambiguous_uids = std::collections::HashSet::new();
        let mut conflict = None;
        for (path, uid) in aliases {
            if ambiguous_paths.contains(path) {
                continue;
            }
            if ambiguous_uids.contains(uid) {
                ambiguous_paths.insert(path.clone());
                conflict.get_or_insert_with(|| {
                    format!(
                        "Conversation identity {uid} appears at multiple board paths; histories remain unlinked until the board identity is repaired."
                    )
                });
                continue;
            }
            if let Some(previous_uid) = identities.get(path).cloned() {
                if previous_uid != *uid {
                    identities.remove(path);
                    paths.remove(&previous_uid);
                    paths.remove(uid);
                    identities.retain(|_, value| value != &previous_uid && value != uid);
                    ambiguous_paths.insert(path.clone());
                    ambiguous_uids.insert(previous_uid.clone());
                    ambiguous_uids.insert(uid.clone());
                    conflict.get_or_insert_with(|| {
                        format!(
                            "Conversation key {path} refers to multiple immutable identities ({previous_uid}, {uid}); histories remain unlinked until the board identity is repaired."
                        )
                    });
                }
                continue;
            }
            if let Some(previous_path) = paths.get(uid).cloned() {
                if previous_path != *path {
                    identities.remove(&previous_path);
                    paths.remove(uid);
                    identities.retain(|_, value| value != uid);
                    ambiguous_paths.insert(previous_path.clone());
                    ambiguous_paths.insert(path.clone());
                    ambiguous_uids.insert(uid.clone());
                    conflict.get_or_insert_with(|| {
                        format!(
                            "Conversation identity {uid} appears at both {previous_path} and {path}; histories remain unlinked until the board identity is repaired."
                        )
                    });
                }
                continue;
            }
            identities.insert(path.clone(), uid.clone());
            paths.insert(uid.clone(), path.clone());
        }
        self.identities = identities;
        self.paths = paths;
        self.ambiguous_paths = ambiguous_paths;
        self.legacy_migration_keys = legacy_migration_keys.iter().cloned().collect();
        if self.loaded {
            for path in &self.ambiguous_paths {
                super::detach_other_history(&mut self.stored, path, "legacy");
            }
            self.add_current_aliases()?;
            canonicalize(&mut self.stored);
            self.messages = display_histories(&self.stored, &self.paths);
        }
        conflict.map_or(Ok(()), Err)
    }

    pub(super) fn add_current_aliases(&mut self) -> Result<bool, String> {
        let mut changed = false;
        for (path, uid) in &self.identities {
            changed |= merge_identity(
                &mut self.stored,
                path,
                uid,
                self.legacy_migration_keys.contains(path),
            )?;
        }
        Ok(changed)
    }

    pub fn has_pending_user_reply(&self, key: &str) -> bool {
        let Some(messages) = self.messages.get(key) else {
            return false;
        };
        let latest_user = messages.iter().rposition(|message| {
            message.role == crate::domain::ChatRole::User && !message.clarification
        });
        let latest_agent = messages.iter().rposition(|message| {
            message.role == crate::domain::ChatRole::Agent && !message.clarification
        });
        let after = latest_user
            .map(|user| &messages[user + 1..])
            .unwrap_or_default();
        if super::retry::has_task_reply_outcome(after) {
            return false;
        }
        latest_user.is_some_and(|user| latest_agent.is_none_or(|agent| user > agent))
    }

    pub fn has_failed_task_reply(&self, key: &str) -> bool {
        self.messages
            .get(key)
            .is_some_and(|messages| super::retry::has_failed_task_reply(messages))
    }

    pub fn has_active_reply_claim(&self, key: &str) -> bool {
        self.messages
            .get(key)
            .is_some_and(|messages| super::retry::has_active_reply_claim(messages))
    }

    pub fn has_ambiguous_identity(&self, key: &str) -> bool {
        self.ambiguous_paths.contains(key)
    }
}
