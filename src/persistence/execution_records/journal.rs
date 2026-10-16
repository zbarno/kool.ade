use std::{collections::BTreeMap, fs, path::PathBuf};

use crate::execution::contract::{EventPageRequestV1, EventStreamValidator, ExecutionEventV1};

const MAX_JOURNAL_EVENTS: usize = 65_536;
const MAX_JOURNAL_BYTES: u64 = 1024 * 1024 * 1024;
const MAX_EVENT_RECORD_BYTES: usize = 512 * 1024;

fn journal_path(task_id: &str, execution_id: &str) -> Result<PathBuf, String> {
    if !super::record::is_canonical_uuid(task_id) || !super::record::is_canonical_uuid(execution_id)
    {
        return Err("execution journal identity is invalid".into());
    }
    Ok(task_directory(task_id, false)?.join(format!("{execution_id}.events")))
}

fn task_directory(task_id: &str, create: bool) -> Result<PathBuf, String> {
    if !super::record::is_canonical_uuid(task_id) {
        return Err("execution journal task identity is invalid".into());
    }
    let root = crate::persistence::state_root();
    let executions = root.join("executions");
    let task = executions.join(task_id);
    for directory in [&root, &executions, &task] {
        if create {
            fs::create_dir_all(directory)
                .map_err(|_| "execution journal directory is unavailable")?;
        }
        match fs::symlink_metadata(directory) {
            Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {}
            Ok(_) => return Err("execution journal directory is not trusted".into()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound && !create => {
                return Ok(task.clone());
            }
            Err(_) => return Err("execution journal directory is unavailable".into()),
        }
    }
    Ok(task)
}

/// Append one immutable event. Repeating the same sequence and content is
/// idempotent; a conflicting sequence or gap is rejected.
pub fn append_event(task_id: &str, event: &ExecutionEventV1) -> Result<(), String> {
    event.validate().map_err(|_| "execution event is invalid")?;
    let path = journal_path(task_id, &event.execution_id)?;
    let task_id = path
        .parent()
        .and_then(|path| path.file_name())
        .and_then(|value| value.to_str())
        .ok_or_else(|| "execution journal task identity is invalid".to_owned())?;
    let parent = task_directory(task_id, true)?;
    if path.parent() != Some(parent.as_path()) {
        return Err("execution journal parent changed".into());
    }
    if fs::symlink_metadata(&path).is_err() {
        fs::create_dir(&path).map_err(|_| "execution journal directory could not be created")?;
    }
    let directory_metadata =
        fs::symlink_metadata(&path).map_err(|_| "execution journal directory is unavailable")?;
    if !directory_metadata.is_dir() || directory_metadata.file_type().is_symlink() {
        return Err("execution journal path is not a real directory".into());
    }

    let mut entries = BTreeMap::new();
    let mut total_bytes = 0_u64;
    for entry in fs::read_dir(&path).map_err(|_| "execution journal could not be scanned")? {
        let entry = entry.map_err(|_| "execution journal could not be scanned")?;
        let metadata = fs::symlink_metadata(entry.path())
            .map_err(|_| "execution journal entry could not be inspected")?;
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| "execution journal filename is invalid")?;
        if is_atomic_temp_name(&name) {
            continue;
        }
        let sequence = parse_event_filename(&name)?;
        if !metadata.is_file() || metadata.file_type().is_symlink() {
            return Err("execution journal entry is not a regular file".into());
        }
        if metadata.len() > MAX_EVENT_RECORD_BYTES as u64 {
            return Err("execution journal event exceeds its size limit".into());
        }
        total_bytes = total_bytes
            .checked_add(metadata.len())
            .ok_or_else(|| "execution journal size overflowed".to_owned())?;
        entries.insert(sequence, entry.path());
    }
    if entries.len() > MAX_JOURNAL_EVENTS || total_bytes > MAX_JOURNAL_BYTES {
        return Err("execution journal exceeds its retention limit".into());
    }
    let event_path = path.join(event_filename(event.sequence));
    if let Some(existing) = entries.get(&event.sequence) {
        let saved: ExecutionEventV1 = serde_json::from_slice(
            &fs::read(existing).map_err(|_| "execution journal event could not be read")?,
        )
        .map_err(|_| "execution journal event is malformed")?;
        return if saved == *event {
            Ok(())
        } else {
            Err("execution journal sequence conflicts with saved content".into())
        };
    }
    let next_sequence = entries
        .last_key_value()
        .map_or(1, |(sequence, _)| sequence.saturating_add(1));
    if event.sequence != next_sequence {
        return Err("execution journal sequence is not contiguous".into());
    }
    let bytes = serde_json::to_vec(event).map_err(|_| "execution event could not be encoded")?;
    if bytes.len() > MAX_EVENT_RECORD_BYTES
        || total_bytes.saturating_add(bytes.len() as u64) > MAX_JOURNAL_BYTES
    {
        return Err("execution journal exceeds its retention limit".into());
    }
    crate::artifacts::atomic_create_bytes(&event_path, &bytes)
        .map_err(|_| "execution event could not be committed to the journal".to_owned())
}

/// Read a bounded page after validating the entire journal's identity, size,
/// ordering and event contents.
pub fn load_events(
    task_id: &str,
    execution_id: &str,
    page: EventPageRequestV1,
) -> Result<Vec<ExecutionEventV1>, String> {
    if !(1..=256).contains(&page.max_events) {
        return Err("execution journal page size is invalid".into());
    }
    let path = journal_path(task_id, execution_id)?;
    let metadata = match fs::symlink_metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(_) => return Err("execution journal path could not be inspected".into()),
    };
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err("execution journal path is not a real directory".into());
    }
    let mut entries = BTreeMap::new();
    let mut total_bytes = 0_u64;
    for entry in fs::read_dir(path).map_err(|_| "execution journal could not be scanned")? {
        let entry = entry.map_err(|_| "execution journal could not be scanned")?;
        let metadata = fs::symlink_metadata(entry.path())
            .map_err(|_| "execution journal entry could not be inspected")?;
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| "execution journal filename is invalid")?;
        if is_atomic_temp_name(&name) {
            continue;
        }
        let sequence = parse_event_filename(&name)?;
        if !metadata.is_file() || metadata.file_type().is_symlink() {
            return Err("execution journal entry is not a regular file".into());
        }
        if metadata.len() > MAX_EVENT_RECORD_BYTES as u64 {
            return Err("execution journal event exceeds its size limit".into());
        }
        total_bytes = total_bytes
            .checked_add(metadata.len())
            .ok_or_else(|| "execution journal size overflowed".to_owned())?;
        entries.insert(sequence, entry.path());
    }
    if entries.len() > MAX_JOURNAL_EVENTS || total_bytes > MAX_JOURNAL_BYTES {
        return Err("execution journal exceeds its retention limit".into());
    }
    let mut events = Vec::new();
    let mut validator = EventStreamValidator::default();
    for (expected, (sequence, file)) in entries.iter().enumerate() {
        if *sequence != expected as u64 + 1 {
            return Err("execution journal sequence has a gap".into());
        }
        let event: ExecutionEventV1 = serde_json::from_slice(
            &fs::read(file).map_err(|_| "execution journal event could not be read")?,
        )
        .map_err(|_| "execution journal event is malformed")?;
        event
            .validate()
            .map_err(|_| "execution journal event is invalid")?;
        validator
            .accept(&event)
            .map_err(|error| format!("execution journal event order is invalid: {error}"))?;
        if event.sequence != *sequence || event.execution_id != execution_id {
            return Err("execution journal event identity does not match its path".into());
        }
        if event.sequence > page.after_sequence && events.len() < usize::from(page.max_events) {
            events.push(event);
        }
    }
    Ok(events)
}

pub(super) fn validate_journal(task_id: &str, execution_id: &str) -> Result<(), String> {
    let _ = load_events(
        task_id,
        execution_id,
        EventPageRequestV1 {
            after_sequence: 0,
            max_events: 1,
        },
    )?;
    Ok(())
}

pub fn journal_cursor(task_id: &str, execution_id: &str) -> Result<u64, String> {
    let _ = load_events(
        task_id,
        execution_id,
        EventPageRequestV1 {
            after_sequence: 0,
            max_events: 1,
        },
    )?;
    let path = journal_path(task_id, execution_id)?;
    let mut last = 0_u64;
    let entries = match fs::read_dir(path) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(0),
        Err(_) => return Err("execution journal could not be scanned".into()),
    };
    for entry in entries {
        let entry = entry.map_err(|_| "execution journal could not be scanned")?;
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| "execution journal filename is invalid")?;
        if is_atomic_temp_name(&name) {
            continue;
        }
        last = last.max(parse_event_filename(&name)?);
    }
    Ok(last)
}

fn event_filename(sequence: u64) -> String {
    format!("{sequence:020}.json")
}

fn parse_event_filename(value: &str) -> Result<u64, String> {
    let stem = value
        .strip_suffix(".json")
        .ok_or_else(|| "execution journal filename is invalid".to_owned())?;
    let sequence = stem
        .parse::<u64>()
        .map_err(|_| "execution journal filename is invalid".to_owned())?;
    if sequence == 0 || event_filename(sequence) != value {
        return Err("execution journal filename is invalid".into());
    }
    Ok(sequence)
}

fn is_atomic_temp_name(value: &str) -> bool {
    let Some(value) = value.strip_prefix('.') else {
        return false;
    };
    let Some(value) = value.strip_suffix(".koolade.tmp") else {
        return false;
    };
    let Some((event_name, writer)) = value.rsplit_once('.') else {
        return false;
    };
    let Some((pid, sequence)) = writer.split_once('-') else {
        return false;
    };
    parse_event_filename(event_name).is_ok()
        && pid.bytes().all(|byte| byte.is_ascii_digit())
        && sequence.bytes().all(|byte| byte.is_ascii_digit())
}
