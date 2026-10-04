//! JSONL chat history under `~/.koolade-packet/projects/<slug>/chat.jsonl`.
//!
//! Design notes:
//! * one JSON object per line (append-friendly, crash-resilient)
//! * corrupted trailing lines are quarantined instead of losing history
//! * gentle size bound: when the file grows past [`MAX_BYTES`] it is compacted
//!   to the most recent [`KEEP_MESSAGES`] lines

use std::fs::{self, OpenOptions};
use std::io::Write as _;
use std::path::PathBuf;

use crate::domain::ChatMessage;

const MAX_BYTES: u64 = 2_000_000;
const KEEP_MESSAGES: usize = 500;
const QUARANTINE_NAME: &str = "chat.quarantine.jsonl";

/// Full chat file location for a project slug.
pub fn chat_path(slug: &str) -> PathBuf {
    crate::persistence::project_dir(slug).join("chat.jsonl")
}

/// Load history for a project. Corrupt lines are skipped (counted) so a
/// torn last write never bricks the conversation.
pub fn load(slug: &str) -> (Vec<ChatMessage>, usize) {
    let path = chat_path(slug);
    let Ok(bytes) = fs::read(&path) else {
        return (Vec::new(), 0);
    };
    let mut out: Vec<ChatMessage> = Vec::new();
    let mut skipped = 0usize;
    let text = String::from_utf8_lossy(&bytes);
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        match serde_json::from_str::<ChatMessage>(line) {
            Ok(m) => out.push(m),
            Err(_) => skipped += 1,
        }
    }
    (out, skipped)
}

/// Append messages; ensures parent dirs. Compacts if the file crosses bounds.
pub fn append(slug: &str, msgs: &[ChatMessage]) -> std::io::Result<()> {
    if msgs.is_empty() {
        return Ok(());
    }
    let path = chat_path(slug);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut f = OpenOptions::new().create(true).append(true).open(&path)?;
    for m in msgs {
        let mut line = serde_json::to_string(m)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string()))?;
        line.push('\n');
        f.write_all(line.as_bytes())?;
    }
    let len = fs::metadata(&path).ok().map(|m| m.len()).unwrap_or(0);
    if len > MAX_BYTES {
        compact(slug)?;
    }
    Ok(())
}

/// Rewrite the store keeping only the newest `KEEP_MESSAGES` entries.
fn compact(slug: &str) -> std::io::Result<()> {
    let (msgs, _) = load(slug);
    let start = msgs.len().saturating_sub(KEEP_MESSAGES);
    let kept = &msgs[start..];
    let mut text = String::new();
    for m in kept {
        if let Ok(line) = serde_json::to_string(m) {
            text.push_str(&line);
            text.push('\n');
        }
    }
    crate::artifacts::atomic_write(&chat_path(slug), &text).map_err(std::io::Error::other)
}

/// Quarantine unrecoverable lines (best-effort diagnostic aid).
pub fn quarantine(slug: &str, corrupt_lines: &[String]) -> std::io::Result<()> {
    if corrupt_lines.is_empty() {
        return Ok(());
    }
    let dir = crate::persistence::project_dir(slug);
    fs::create_dir_all(&dir)?;
    let mut f = OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join(QUARANTINE_NAME))?;
    for l in corrupt_lines {
        writeln!(f, "{l}")?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::ChatRole;
    use std::env;

    // KOOLADE_HOME is process-global state: whichever test flips it must hold
    // this for its WHOLE body, or siblings' reads land on the wrong home.
    static ENV_HOME_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn use_tmp_home(tag: &str) -> (PathBuf, std::sync::MutexGuard<'static, ()>) {
        let lock = ENV_HOME_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let home = env::temp_dir().join(format!("koolade_home_{tag}_{}", std::process::id()));
        let _ = fs::remove_dir_all(&home);
        // SAFETY: guarded by ENV_HOME_LOCK; no other test mutates or depends
        // on KOOLADE_HOME while the guard is held.
        unsafe { env::set_var("KOOLADE_HOME", &home) };
        (home, lock)
    }

    #[test]
    fn append_then_load_round_trips() {
        let (_h, _env) = use_tmp_home("rt");
        let a = ChatMessage::new(ChatRole::User, "one", None);
        let b = ChatMessage::new(ChatRole::Agent, "two", Some("CLR-001".into()));
        append("proj-a", &[a.clone(), b.clone()]).unwrap();
        let (msgs, skipped) = load("proj-a");
        assert_eq!((msgs, skipped), (vec![a, b], 0));
    }

    #[test]
    fn torn_trailing_line_is_skipped_not_fatal() {
        let (_h, _env) = use_tmp_home("torn");
        let m = ChatMessage::new(ChatRole::User, "good", None);
        append("proj-b", std::slice::from_ref(&m)).unwrap();
        let path = chat_path("proj-b");
        let mut f = OpenOptions::new().append(true).open(&path).unwrap();
        write!(f, "{{\"id\":\"x\",\"role\":").unwrap(); // torn write
        let (msgs, skipped) = load("proj-b");
        assert_eq!(msgs.len(), 1);
        assert_eq!(skipped, 1);
    }
}
