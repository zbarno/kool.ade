//! Private, append-only Manager activity history outside project repositories.

use std::{fs, fs::OpenOptions, io::Write as _, path::PathBuf};

use crate::domain::ChatMessage;

const MAX_BYTES: u64 = 1_000_000;
const KEEP_MESSAGES: usize = 300;

#[derive(Debug, Default, Eq, PartialEq)]
pub struct LoadDiagnostics {
    pub dropped_records: usize,
    pub read_error: Option<String>,
}

pub fn path(slug: &str) -> PathBuf {
    crate::persistence::project_dir(slug).join("manager-activity.jsonl")
}

pub fn load(slug: &str) -> Vec<ChatMessage> {
    load_with_diagnostics(slug).0
}

pub fn load_with_diagnostics(slug: &str) -> (Vec<ChatMessage>, LoadDiagnostics) {
    let bytes = match fs::read(path(slug)) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return (Vec::new(), LoadDiagnostics::default());
        }
        Err(error) => {
            return (
                Vec::new(),
                LoadDiagnostics {
                    read_error: Some(error.to_string()),
                    ..Default::default()
                },
            );
        }
    };
    let mut diagnostics = LoadDiagnostics::default();
    let mut messages = Vec::new();
    for line in String::from_utf8_lossy(&bytes).lines() {
        if line.trim().is_empty() {
            continue;
        }
        match serde_json::from_str::<ChatMessage>(line.trim()) {
            Ok(message) => {
                if message
                    .manager_run_event
                    .as_ref()
                    .is_some_and(|event| event.validate().is_err())
                {
                    diagnostics.dropped_records = diagnostics.dropped_records.saturating_add(1);
                }
                messages.push(message);
            }
            Err(_) => {
                diagnostics.dropped_records = diagnostics.dropped_records.saturating_add(1);
            }
        }
    }
    (retain_recent_and_pending(messages), diagnostics)
}

fn retain_recent_and_pending(messages: Vec<ChatMessage>) -> Vec<ChatMessage> {
    use crate::domain::ManagerRunOutcomeV1 as Outcome;
    let mut latest = std::collections::BTreeMap::<String, Outcome>::new();
    let mut checkpoints = std::collections::BTreeMap::<String, (usize, ChatMessage)>::new();
    let mut invalid_records = Vec::new();
    for (index, message) in messages.iter().enumerate() {
        let Some(event) = &message.manager_run_event else {
            continue;
        };
        if event.validate().is_err() {
            invalid_records.push((index, message.clone()));
            continue;
        }
        let digest = event.checkpoint_sha256.clone();
        latest.insert(digest.clone(), event.outcome);
        if event.checkpoint.is_some() {
            checkpoints.insert(digest, (index, message.clone()));
        }
    }
    let pending = latest
        .into_iter()
        .filter_map(|(digest, outcome)| {
            matches!(
                outcome,
                Outcome::Queued | Outcome::Started | Outcome::Retried
            )
            .then(|| checkpoints.remove(&digest))
            .flatten()
        })
        .collect::<Vec<_>>();
    let first_recent = messages.len().saturating_sub(KEEP_MESSAGES);
    let recent_run_ids = messages
        .iter()
        .skip(first_recent)
        .filter_map(|message| message.manager_run_event.as_ref())
        .map(|event| event.run_id.clone())
        .collect::<std::collections::BTreeSet<_>>();
    let retained_run_ids = messages
        .iter()
        .filter_map(|message| message.manager_run_event.as_ref())
        .filter(|event| recent_run_ids.contains(&event.run_id))
        .map(|event| event.run_id.clone())
        .collect::<std::collections::BTreeSet<_>>();
    let run_history = messages
        .iter()
        .enumerate()
        .filter(|(index, message)| {
            *index < first_recent
                && message
                    .manager_run_event
                    .as_ref()
                    .is_some_and(|event| retained_run_ids.contains(&event.run_id))
        })
        .map(|(index, message)| (index, message.clone()))
        .collect::<Vec<_>>();
    let mut retained = messages
        .into_iter()
        .enumerate()
        .filter(|(index, _)| *index >= first_recent)
        .collect::<Vec<_>>();
    let retained_ids = retained
        .iter()
        .map(|(_, message)| message.id.clone())
        .collect::<std::collections::BTreeSet<_>>();
    retained.extend(
        pending.into_iter().filter(|(index, message)| {
            *index < first_recent && !retained_ids.contains(&message.id)
        }),
    );
    retained.extend(
        run_history
            .into_iter()
            .filter(|(_, message)| !retained_ids.contains(&message.id)),
    );
    retained.extend(
        invalid_records
            .into_iter()
            .filter(|(_, message)| !retained_ids.contains(&message.id)),
    );
    let mut seen = std::collections::BTreeSet::new();
    retained.retain(|(_, message)| seen.insert(message.id.clone()));
    retained.sort_by_key(|(index, _)| *index);
    retained.into_iter().map(|(_, message)| message).collect()
}

fn unresolved_run_count(messages: &[ChatMessage]) -> usize {
    let mut seen = std::collections::BTreeMap::new();
    for message in messages {
        if let Some(event) = &message.manager_run_event {
            seen.insert(event.checkpoint_sha256.clone(), event.outcome);
        }
    }
    seen.values()
        .filter(|outcome| {
            matches!(
                outcome,
                crate::domain::ManagerRunOutcomeV1::Started
                    | crate::domain::ManagerRunOutcomeV1::Queued
                    | crate::domain::ManagerRunOutcomeV1::Retried
            )
        })
        .count()
}

pub fn append(slug: &str, messages: &[ChatMessage]) -> std::io::Result<()> {
    if messages.is_empty() {
        return Ok(());
    }
    let (current, diagnostics) = load_with_diagnostics(slug);
    if diagnostics.dropped_records > 0 || diagnostics.read_error.is_some() {
        return Err(std::io::Error::other(format!(
            "Manager activity history has unreadable records and must be reviewed before append: {} dropped record(s), read error: {}",
            diagnostics.dropped_records,
            diagnostics.read_error.as_deref().unwrap_or("none")
        )));
    }
    for message in messages {
        if message.manager_run_event.as_ref().is_some_and(|event| {
            event.outcome == crate::domain::ManagerRunOutcomeV1::Started
                && unresolved_run_count(&current) >= 2
        }) {
            return Err(std::io::Error::other(
                "Manager activity has too many unresolved checkpoints",
            ));
        }
    }
    let path = path(slug);
    let parent = path.parent().expect("activity path has a parent");
    fs::create_dir_all(parent)?;
    let retained = retain_recent_and_pending(
        current
            .iter()
            .cloned()
            .chain(messages.iter().cloned())
            .collect(),
    );
    let encoded_size = retained
        .iter()
        .map(|message| serde_json::to_vec(message).map_or(0, |line| line.len() + 1))
        .sum::<usize>();
    let incoming_size = messages
        .iter()
        .map(|message| serde_json::to_vec(message).map_or(0, |line| line.len() + 1))
        .sum::<usize>();
    let current_size = fs::metadata(&path).map_or(0, |metadata| metadata.len());
    if current_size.saturating_add(incoming_size as u64) > MAX_BYTES {
        if encoded_size > MAX_BYTES as usize {
            return Err(std::io::Error::other(
                "Manager activity retention cannot preserve unresolved checkpoints within its size limit",
            ));
        }
        let temporary = parent.join("manager-activity.jsonl.tmp");
        let mut file = OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(&temporary)?;
        for message in retained {
            serde_json::to_writer(&mut file, &message)?;
            file.write_all(b"\n")?;
        }
        file.flush()?;
        fs::rename(temporary, path)?;
        return Ok(());
    }
    let mut file = OpenOptions::new().create(true).append(true).open(&path)?;
    for message in messages {
        serde_json::to_writer(&mut file, message)?;
        file.write_all(b"\n")?;
    }
    file.flush()?;
    drop(file);
    Ok(())
}

#[cfg(test)]
mod tests;
