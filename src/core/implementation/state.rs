use super::{Implementation, ImplementationStatus, PullRequestState};
use std::{fs, path::Path};

/// Load and one-time migrate v0 state strings to versioned enum values.
pub(crate) fn read_state_file(path: &Path) -> anyhow::Result<Implementation> {
    let bytes = fs::read(path)?;
    let (state, migrated) = decode(&bytes)?;
    if migrated {
        let dir = path
            .parent()
            .ok_or_else(|| anyhow::anyhow!("Implementation state has no parent directory"))?;
        save(dir, &state)?;
    }
    Ok(state)
}

pub(crate) fn decode_state_bytes(bytes: &[u8]) -> anyhow::Result<Implementation> {
    decode(bytes).map(|(state, _)| state)
}

fn decode(bytes: &[u8]) -> anyhow::Result<(Implementation, bool)> {
    let mut value: serde_json::Value = serde_json::from_slice(bytes)?;
    let object = value
        .as_object_mut()
        .ok_or_else(|| anyhow::anyhow!("Implementation state must be a JSON object"))?;
    let version = match object.remove("schemaVersion") {
        None => 0,
        Some(version) => version
            .as_u64()
            .ok_or_else(|| anyhow::anyhow!("Implementation state version must be an integer"))?,
    };
    anyhow::ensure!(
        version <= 1,
        "Unsupported implementation state version {version}"
    );
    let legacy = version == 0;
    let raw_status = object
        .get("status")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    let parsed_status = if legacy {
        ImplementationStatus::parse_legacy_label(raw_status)
    } else {
        ImplementationStatus::parse_wire(raw_status)
    };
    let mut invalid = Vec::new();
    let status = parsed_status.unwrap_or_else(|| {
        invalid.push(format!(
            "Unknown saved implementation status {raw_status:?}."
        ));
        ImplementationStatus::Blocked
    });
    object.insert("status".into(), serde_json::to_value(status)?);
    if let Some(raw_pr_state) = object
        .get("pr_state")
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned)
    {
        let parsed = if legacy {
            PullRequestState::parse_legacy(&raw_pr_state)
        } else {
            PullRequestState::parse_wire(&raw_pr_state)
        };
        if let Some(pr_state) = parsed {
            object.insert("pr_state".into(), serde_json::to_value(pr_state)?);
        } else {
            object.insert("pr_state".into(), serde_json::Value::Null);
            invalid.push(format!(
                "Unknown saved pull request state {raw_pr_state:?}."
            ));
        }
    }
    let had_invalid = !invalid.is_empty();
    if had_invalid {
        object.insert(
            "status".into(),
            serde_json::to_value(ImplementationStatus::Blocked)?,
        );
        let prior = object
            .get("detail")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default();
        let message = format!(
            "{} The task was moved to Needs attention; inspect its saved state before resuming.",
            invalid.join(" ")
        );
        object.insert(
            "detail".into(),
            serde_json::Value::String(if prior.is_empty() {
                message
            } else {
                format!("{message}\n\n{prior}")
            }),
        );
    }
    let migrated = legacy || had_invalid;
    Ok((serde_json::from_value(value)?, migrated))
}

pub(crate) fn save(dir: &Path, state: &Implementation) -> anyhow::Result<()> {
    crate::artifacts::atomic_write_bytes(&dir.join("state.json"), &serialize_state(state)?)
}

pub(crate) fn serialize_state(state: &Implementation) -> anyhow::Result<Vec<u8>> {
    let mut value = serde_json::to_value(state)?;
    value
        .as_object_mut()
        .expect("Implementation serializes as an object")
        .insert("schemaVersion".into(), serde_json::json!(1));
    Ok(serde_json::to_vec_pretty(&value)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::implementation::ImplementationStatus;

    fn temporary_state(name: &str, bytes: &[u8]) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "koolade-implementation-state-{name}-{}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("state.json");
        fs::write(&path, bytes).unwrap();
        path
    }

    #[test]
    fn legacy_labels_are_migrated_and_invalid_saved_values_are_visible() {
        let legacy = serde_json::json!({
            "ticket":"task.md", "ticket_text":"task", "branch":"koolade/task",
            "base":"main", "base_commit":"abc", "worktree":"/tmp/task",
            "status":"PR created", "detail":"Previously verified.", "pr_url":null,
            "verified_head":null, "pr_state":"OPEN"
        });
        let path = temporary_state("legacy", serde_json::to_string(&legacy).unwrap().as_bytes());
        let loaded = read_state_file(&path).unwrap();
        assert_eq!(loaded.status, ImplementationStatus::AwaitingReview);
        assert_eq!(loaded.pr_state, Some(PullRequestState::Open));
        let saved: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(saved["schemaVersion"], 1);
        assert_eq!(saved["status"], "awaiting_review");
        assert_eq!(saved["pr_state"], "open");

        let mut invalid_pr = saved.clone();
        invalid_pr["pr_state"] = serde_json::Value::String("PENDING".into());
        fs::write(&path, serde_json::to_vec(&invalid_pr).unwrap()).unwrap();
        let loaded = read_state_file(&path).unwrap();
        assert_eq!(loaded.status, ImplementationStatus::Blocked);
        assert_eq!(loaded.pr_state, None);
        assert!(loaded.detail.contains("PENDING"));

        let mut invalid = saved;
        invalid["status"] = serde_json::Value::String("future_status".into());
        fs::write(&path, serde_json::to_vec(&invalid).unwrap()).unwrap();
        let loaded = read_state_file(&path).unwrap();
        assert_eq!(loaded.status, ImplementationStatus::Blocked);
        assert_eq!(loaded.pr_state, Some(PullRequestState::Open));
        assert!(loaded.detail.contains("future_status"));
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }
}
