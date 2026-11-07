//! Private, append-only Manager activity history outside project repositories.

use std::{fs, fs::OpenOptions, io::Write as _, path::PathBuf};

use crate::domain::ChatMessage;

const MAX_BYTES: u64 = 1_000_000;
const KEEP_MESSAGES: usize = 300;

pub fn path(slug: &str) -> PathBuf {
    crate::persistence::project_dir(slug).join("manager-activity.jsonl")
}

pub fn load(slug: &str) -> Vec<ChatMessage> {
    let Ok(bytes) = fs::read(path(slug)) else {
        return Vec::new();
    };
    String::from_utf8_lossy(&bytes)
        .lines()
        .filter_map(|line| serde_json::from_str(line.trim()).ok())
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .take(KEEP_MESSAGES)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect()
}

pub fn append(slug: &str, messages: &[ChatMessage]) -> std::io::Result<()> {
    if messages.is_empty() {
        return Ok(());
    }
    let path = path(slug);
    let parent = path.parent().expect("activity path has a parent");
    fs::create_dir_all(parent)?;
    let mut file = OpenOptions::new().create(true).append(true).open(&path)?;
    for message in messages {
        serde_json::to_writer(&mut file, message)?;
        file.write_all(b"\n")?;
    }
    file.flush()?;
    drop(file);
    if fs::metadata(&path)?.len() > MAX_BYTES {
        let mut retained = load(slug);
        retained.truncate(KEEP_MESSAGES);
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
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn activity_history_round_trips_in_its_own_store() {
        let mut environment = crate::test_support::ProcessEnvironmentGuard::new();
        let home =
            std::env::temp_dir().join(format!("manager-activity-home-{}", std::process::id()));
        environment.set("KOOLADE_HOME", &home);
        let slug = format!("manager-activity-{}", std::process::id());
        let message = ChatMessage::new(crate::domain::ChatRole::Agent, "Board checked.", None);
        append(&slug, std::slice::from_ref(&message)).unwrap();
        assert_eq!(load(&slug), vec![message]);
        assert!(path(&slug).ends_with("manager-activity.jsonl"));
    }
}
