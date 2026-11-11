use std::{
    fs::{self, File},
    io::{Read, Seek, SeekFrom},
    path::Path,
};

use super::{SessionEventKindV1, SessionJournalEventV1};

pub(super) const MAX_JOURNAL_BYTES: usize = 16 * 1024 * 1024;
pub(super) const MAX_JOURNAL_EVENTS: usize = 65_536;

pub(super) fn read_events(file: &mut File) -> Result<Vec<SessionJournalEventV1>, &'static str> {
    file.seek(SeekFrom::Start(0))
        .map_err(|_| "session_journal_unavailable")?;
    let mut bytes = Vec::new();
    file.take((MAX_JOURNAL_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| "session_journal_unavailable")?;
    if bytes.len() > MAX_JOURNAL_BYTES {
        return Err("session_journal_capacity_exceeded");
    }
    if !bytes.is_empty() && bytes.last() != Some(&b'\n') {
        return Err("session_journal_corrupt");
    }
    let mut events = Vec::new();
    for (index, line) in bytes.split(|byte| *byte == b'\n').enumerate() {
        if line.is_empty() {
            continue;
        }
        let event: SessionJournalEventV1 =
            serde_json::from_slice(line).map_err(|_| "session_journal_corrupt")?;
        validate_event(&event)?;
        if event.sequence != index as u64 + 1 || events.len() >= MAX_JOURNAL_EVENTS {
            return Err("session_journal_corrupt");
        }
        events.push(event);
    }
    Ok(events)
}

pub(super) fn validate_event(event: &SessionJournalEventV1) -> Result<(), &'static str> {
    if event.schema_version != 1
        || event.sequence > MAX_JOURNAL_EVENTS as u64
        || event.identity.validate().is_err()
        || !canonical_uuid(&event.event_id)
        || !canonical_uuid(&event.lease_id)
        || event.lease_generation == 0
        || event.approval_generation == 0
        || event.cancellation_generation == 0
        || event.agent_role.is_empty()
        || event.agent_role.len() > 64
        || !valid_digest(&event.image_digest)
        || !valid_digest(&event.policy_sha256)
        || event
            .boot_fingerprint
            .as_ref()
            .is_some_and(|fingerprint| !valid_digest(fingerprint))
        || event.allowed_apps.len() > 32
        || event.allowed_apps.iter().any(|app| {
            app.is_empty()
                || app.len() > 64
                || !app
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        })
        || event.expires_at_unix_ms == 0
        || event
            .action_id
            .as_ref()
            .is_some_and(|id| !canonical_uuid(id))
        || event.action_sequence.is_some_and(|sequence| sequence == 0)
        || event.action_id.is_some() != event.action_sequence.is_some()
        || event.action_actor.is_some() != event.action_operation.is_some()
        || event.action_sequence.is_some() != event.action_operation.is_some()
        || event
            .action_intent_id
            .as_ref()
            .is_some_and(|id| !canonical_uuid(id))
        || event
            .action_intent_sha256
            .as_ref()
            .is_some_and(|digest| !valid_digest(digest))
        || event.action_intent_id.is_some() != event.action_intent_sha256.is_some()
        || event.action_intent_id.is_some() != event.action_intent_state.is_some()
        || (event.kind == SessionEventKindV1::ActionIntentStateChanged)
            != event.action_intent_state.is_some()
        || (event.reconciled_from.is_some() && event.kind != SessionEventKindV1::SessionInterrupted)
    {
        return Err("session_journal_invalid_event");
    }
    Ok(())
}

fn canonical_uuid(value: &str) -> bool {
    uuid::Uuid::parse_str(value).is_ok_and(|id| id.hyphenated().to_string() == value)
}

fn valid_digest(value: &str) -> bool {
    value
        .strip_prefix("sha256:")
        .is_some_and(|hash| hash.len() == 64 && hash.bytes().all(|byte| byte.is_ascii_hexdigit()))
}

pub(super) fn verify_private_dir(path: &Path) -> Result<(), &'static str> {
    let metadata = fs::symlink_metadata(path).map_err(|_| "session_journal_unavailable")?;
    if !metadata.file_type().is_dir() {
        return Err("session_journal_unavailable");
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        if metadata.uid() != unsafe { libc::geteuid() } {
            return Err("session_journal_unavailable");
        }
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))
            .map_err(|_| "session_journal_unavailable")?;
    }
    Ok(())
}

pub(super) fn verify_private_file(file: &File) -> Result<(), &'static str> {
    let metadata = file.metadata().map_err(|_| "session_journal_unavailable")?;
    if !metadata.is_file() {
        return Err("session_journal_unavailable");
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        if metadata.uid() != unsafe { libc::geteuid() } {
            return Err("session_journal_unavailable");
        }
        file.set_permissions(fs::Permissions::from_mode(0o600))
            .map_err(|_| "session_journal_unavailable")?;
    }
    Ok(())
}
